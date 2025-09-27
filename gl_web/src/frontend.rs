//! ABOUTME: Axum-based frontend with server-rendered pages using Askama templates
//! ABOUTME: Handles user-facing web interface with HTMX interactivity

#![allow(unused_imports)] // post is used in router but clippy doesn't detect it

use crate::auth::{JwtAuth, PasswordAuth};

/// HTML escape utility to prevent XSS attacks
pub fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Generate ETag from image bytes for caching
fn generate_etag(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    format!("\"{}\"", hex::encode(&hash[..8])) // Use first 8 bytes of SHA-256 for ETag
}

/// Log sampling utility to reduce noise from repetitive MJPEG lag warnings
static MJPEG_LAG_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Sample MJPEG lag warnings - only log every Nth occurrence to reduce noise
fn should_log_mjpeg_lag() -> bool {
    let count = MJPEG_LAG_COUNTER.fetch_add(1, Ordering::Relaxed);
    count % 10 == 0 // Log every 10th lag warning
}
use askama::Template;
use axum::{
    body::Body,
    extract::{Form, FromRef, Path, State},
    http::{
        header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH, LOCATION, SET_COOKIE},
        HeaderMap, StatusCode,
    },
    response::{Html, IntoResponse, Json, Redirect, Response},
    routing::{get, post},
    Router,
};
use gl_capture::{CaptureSource, FileSource};
use gl_core::Error;
use gl_db::{StreamRepository, UserRepository};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tracing::{debug, warn};

use crate::{routes::ai_axum, AppState};

/// Authenticated user for Axum extractors
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: String,
    pub email: String,
}

/// Axum extractor for authenticated user
#[axum::async_trait]
impl<S> axum::extract::FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    FrontendState: axum::extract::FromRef<S>,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        use axum::http::header::COOKIE;

        let frontend_state = FrontendState::from_ref(state);

        // Extract auth token from cookie with proper parsing
        let auth_token = parts
            .headers
            .get(COOKIE)
            .and_then(|header| header.to_str().ok())
            .and_then(|cookies| {
                for cookie in cookies.split(';') {
                    let cookie = cookie.trim();
                    if let Some(token_part) = cookie.strip_prefix("auth_token=") {
                        // Handle URL-decoded and quoted values
                        let token_value =
                            if token_part.starts_with('"') && token_part.ends_with('"') {
                                &token_part[1..token_part.len() - 1] // Remove quotes
                            } else {
                                token_part
                            };
                        // Basic validation - JWT tokens should be alphanumeric + dots and dashes
                        if token_value
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
                            && token_value.len() > 10
                        {
                            return Some(token_value);
                        }
                    }
                }
                None
            });

        let token = match auth_token {
            Some(token) => token,
            None => {
                debug!("No auth token found in cookies");
                return Err(Redirect::temporary("/login").into_response());
            }
        };

        // Verify the JWT token
        match crate::auth::JwtAuth::verify_token(
            token,
            &frontend_state.app_state.security_config.jwt_secret,
            &frontend_state.app_state.security_config.jwt_issuer,
        ) {
            Ok(claims) => {
                debug!("JWT token verified for user: {}", claims.sub);
                Ok(AuthenticatedUser {
                    id: claims.sub,
                    email: claims.email,
                })
            }
            Err(e) => {
                warn!("JWT token verification failed: {}", e);
                return Err(Redirect::temporary("/login").into_response());
            }
        }
    }
}

/// Frontend-specific state wrapper for Axum
#[derive(Clone)]
pub struct FrontendState {
    pub app_state: AppState,
}

impl From<AppState> for FrontendState {
    fn from(app_state: AppState) -> Self {
        Self { app_state }
    }
}

// Note: Base template functionality will be handled by template inheritance

/// Login page template
#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub error_message: String,
    pub user: UserInfo,  // Empty user for login page
    pub logged_in: bool, // false for login page
}

/// Setup page template for initial admin creation
#[derive(Template)]
#[template(path = "setup.html")]
pub struct SetupTemplate {
    pub user: UserInfo,  // Empty user for setup page
    pub logged_in: bool, // false for setup page
}

/// Dashboard template
#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub user: UserInfo,  // Real user for authenticated pages
    pub logged_in: bool, // true for authenticated pages
    pub stream_count: usize,
}

// Streams list template disabled - generating full page HTML directly
// until template character issues are resolved

/// Streams grid fragment for HTMX updates
#[derive(Template)]
#[template(path = "streams_simple.html")]
pub struct StreamsGridFragment {
    pub streams: Vec<StreamInfo>,
}

/// Streams error fragment
#[derive(Template)]
#[template(path = "streams_error.html")]
pub struct StreamsErrorFragment {
    pub error_message: String,
}

/// Streams loading fragment
#[derive(Template)]
#[template(path = "streams_loading.html")]
pub struct StreamsLoadingFragment;

/// Stream detail template
#[derive(Template)]
#[template(path = "stream_detail.html")]
pub struct StreamDetailTemplate {
    pub stream: StreamInfo,
    pub user: UserInfo,
    pub logged_in: bool,
}

/// Individual stream card component for HTMX
#[derive(Template)]
#[template(path = "card_simple.html")]
pub struct StreamCard {
    pub stream: StreamInfo,
}

// Admin page template temporarily disabled due to character issues
// Will fix the template character problems

/// Admin user info
#[derive(Serialize, Deserialize, Clone)]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: String,
}

/// Admin API key info
#[derive(Serialize, Deserialize, Clone)]
pub struct AdminApiKey {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

/// User information for templates
#[derive(Serialize, Deserialize, Clone)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
}

/// Stream information for templates
#[derive(Serialize, Deserialize, Clone)]
pub struct StreamInfo {
    pub id: String,
    pub stream_id: String, // Never optional in templates
    pub name: String,
    pub status: String,        // "active" or "inactive"
    pub last_frame_at: String, // Never optional, use "Never" for None
}

/// Login form data from HTMX submission
#[derive(Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

/// Stream creation form data
#[derive(Deserialize)]
pub struct StreamCreateForm {
    pub name: String,
    pub description: Option<String>,
    pub config_kind: String,
    pub is_default: Option<String>, // Checkbox comes as Option<String>

    // Common fields for all stream types
    pub snapshot_interval: Option<u32>, // How often to take snapshots

    // RTSP fields
    pub rtsp_url: Option<String>,
    pub rtsp_width: Option<u32>,
    pub rtsp_height: Option<u32>,

    // FFmpeg fields
    pub ffmpeg_source: Option<String>,
    pub ffmpeg_args: Option<String>,
    pub ffmpeg_width: Option<u32>,
    pub ffmpeg_height: Option<u32>,

    // File source fields
    pub file_path: Option<String>,

    // Website capture fields
    pub website_url: Option<String>,
    pub website_width: Option<u32>,
    pub website_height: Option<u32>,
    pub website_timeout: Option<u32>,
    pub element_selector: Option<String>,
    pub selector_type: Option<String>, // "css" or "xpath"
    pub headless: Option<String>,
    pub stealth: Option<String>,
    pub auth_username: Option<String>, // Basic auth username
    pub auth_password: Option<String>, // Basic auth password

    // YouTube/yt-dlp fields
    pub youtube_url: Option<String>,
    pub youtube_format: Option<String>,  // Quality/format
    pub youtube_is_live: Option<String>, // Checkbox for live stream

    // Legacy fields for backward compatibility
    pub capture_interval: Option<u32>, // Legacy field - maps to snapshot_interval
}

/// Shared stream form template for both create and edit
#[derive(Template)]
#[template(path = "stream_form.html")]
pub struct StreamFormTemplate {
    pub user: UserInfo,
    pub logged_in: bool,
    pub error_message: String,
    pub form_title: String,
    pub form_action: String,
    pub submit_button_text: String,
    pub stream_data: StreamConfigForEdit,
}

/// Stream config data for edit form
#[derive(Serialize, Deserialize, Clone)]
pub struct StreamConfigForEdit {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config_kind: String,
    pub is_default: bool,

    // Common fields
    pub snapshot_interval: u32, // How often to take snapshots
    pub width: u32,             // Default width for streams that support it
    pub height: u32,            // Default height for streams that support it

    // RTSP fields
    pub rtsp_url: String,

    // FFmpeg fields
    pub ffmpeg_source: String,
    pub ffmpeg_args: String,

    // File fields
    pub file_path: String,

    // Website fields
    pub website_url: String,
    pub website_width: u32,
    pub website_height: u32,
    pub website_timeout: u32,
    pub element_selector: String,
    pub selector_type: String, // "css" or "xpath"
    pub headless: bool,
    pub stealth: bool,
    pub auth_username: String, // Basic auth username
    pub auth_password: String, // Basic auth password

    // YouTube fields
    pub youtube_url: String,
    pub youtube_format: String,
    pub youtube_is_live: bool,

    // Legacy field for backward compatibility
    pub capture_interval: u32, // Maps to snapshot_interval
}

impl StreamConfigForEdit {
    /// Create empty config for new stream form
    pub fn empty() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            config_kind: String::new(),
            is_default: false,

            // Common fields
            snapshot_interval: 30,
            width: 1920,
            height: 1080,

            // RTSP fields
            rtsp_url: String::new(),

            // FFmpeg fields
            ffmpeg_source: String::new(),
            ffmpeg_args: String::new(),

            // File fields
            file_path: String::new(),

            // Website fields
            website_url: String::new(),
            website_width: 1920,
            website_height: 1080,
            website_timeout: 30,
            element_selector: String::new(),
            selector_type: "css".to_string(),
            headless: true,
            stealth: false,
            auth_username: String::new(),
            auth_password: String::new(),

            // YouTube fields
            youtube_url: String::new(),
            youtube_format: "best".to_string(),
            youtube_is_live: false,

            // Legacy field
            capture_interval: 5, // Same as snapshot_interval for compatibility
        }
    }
}

/// Export stream configuration
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct StreamExport {
    pub name: String,
    pub description: Option<String>,
    pub config: serde_json::Value,
    pub is_default: bool,
}

/// Import stream configuration request
#[derive(Debug, serde::Deserialize)]
pub struct StreamImportRequest {
    pub streams: Vec<StreamExport>,
    pub overwrite_mode: Option<String>, // "skip", "overwrite", or "create_new"
}

/// Create the Axum router for frontend pages
pub fn create_frontend_router() -> Router<FrontendState> {
    Router::new()
        .route("/", get(root_handler))
        .route("/login", get(login_page_handler).post(login_handler))
        .route("/logout", get(logout_handler))
        .route("/setup", get(setup_page_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/streams", get(streams_list_handler))
        .route("/streams/:id", get(stream_detail_handler))
        .route("/settings", get(admin_handler))
        .route(
            "/settings/streams/new",
            get(admin_stream_new_page).post(admin_stream_create),
        )
        .route(
            "/settings/streams/:id/edit",
            get(admin_stream_edit_page).post(admin_stream_update),
        )
        // HTMX endpoints for dynamic updates
        .route("/api/htmx/streams-list", get(htmx_streams_fragment))
        .route("/api/htmx/stream-card/:id", get(htmx_stream_card_handler))
        .route(
            "/api/htmx/stream/:id/status",
            get(htmx_stream_status_fragment),
        )
        // Settings CRUD endpoints
        .route(
            "/api/settings/streams/:id",
            axum::routing::delete(admin_delete_stream),
        )
        .route(
            "/api/settings/streams/:id/start",
            axum::routing::post(admin_start_stream),
        )
        .route(
            "/api/settings/streams/:id/stop",
            axum::routing::post(admin_stop_stream),
        )
        // Stream Export/Import endpoints
        .route("/api/settings/streams/export", get(api_export_streams))
        .route(
            "/api/settings/streams/import",
            axum::routing::post(api_import_streams),
        )
        // System settings endpoints
        .route(
            "/api/settings/config",
            get(api_get_settings).put(api_update_setting),
        )
        // Stream API endpoints
        .route("/api/stream/:id/snapshot", get(stream_snapshot))
        .route("/api/stream/:id/thumbnail", get(stream_thumbnail))
        .route("/api/stream/:id/mjpeg", get(stream_mjpeg))
        .route("/api/stream/:id/start", axum::routing::post(stream_start))
        .route("/api/stream/:id/stop", axum::routing::post(stream_stop))
        // Auth API endpoints
        .route("/api/auth/setup/needed", get(auth_setup_needed))
        .route(
            "/api/auth/setup/signup",
            axum::routing::post(auth_setup_signup),
        )
        // Merge AI routes
        .merge(ai_axum::ai_routes())
}

/// Root handler - redirect to setup if needed, otherwise dashboard
async fn root_handler(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    match user_repo.has_any_users().await {
        Ok(false) => {
            // No users exist, redirect to setup
            Redirect::temporary("/setup")
        }
        Ok(true) => {
            // Users exist, redirect to dashboard
            Redirect::temporary("/dashboard")
        }
        Err(e) => {
            warn!("Failed to check for users during root handler: {}", e);
            // On error, assume setup is needed for safety
            Redirect::temporary("/setup")
        }
    }
}

/// Dashboard page handler
async fn dashboard_handler(
    authenticated_user: AuthenticatedUser,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    // Get actual user info from the database
    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());
    let user = match user_repo.find_by_id(&authenticated_user.id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            warn!(
                "Authenticated user not found in database: {}",
                authenticated_user.id
            );
            return Redirect::temporary("/login").into_response();
        }
        Err(e) => {
            warn!("Failed to load user from database: {}", e);
            return Redirect::temporary("/login").into_response();
        }
    };

    // Get stream count
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());
    let stream_count = match stream_repo.list(Some(&user.id), 0, 1000).await {
        Ok(streams) => streams.len(),
        Err(e) => {
            warn!("Failed to load user streams: {}", e);
            0
        }
    };

    let template = DashboardTemplate {
        user: UserInfo {
            id: user.id,
            username: user.username,
            is_admin: true, // All users are admin in this system
        },
        logged_in: true,
        stream_count,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response(),
    }
}

/// Login page handler
async fn login_page_handler() -> impl IntoResponse {
    let template = LoginTemplate {
        error_message: String::new(),
        user: UserInfo {
            id: String::new(),
            username: String::new(),
            is_admin: false,
        },
        logged_in: false, // Not logged in
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response(),
    }
}

/// Setup page handler for first admin user creation
async fn setup_page_handler() -> impl IntoResponse {
    let template = SetupTemplate {
        user: UserInfo {
            id: String::new(),
            username: String::new(),
            is_admin: false,
        },
        logged_in: false, // Not logged in
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response(),
    }
}

/// Login form handler
async fn login_handler(
    headers: axum::http::HeaderMap,
    State(frontend_state): State<FrontendState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    debug!("Login attempt for username: {}", form.username);

    // Check if this is an HTMX request
    let is_htmx_request = headers.get("HX-Request").is_some();

    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    // Find user by email (username field is actually email in the form)
    match user_repo.find_by_email(&form.username).await {
        Ok(Some(user)) => {
            if !user.is_active.unwrap_or(false) {
                warn!("Login attempt for inactive user: {}", user.id);
                return render_login_with_error("Account is disabled").into_response();
            }

            // Verify password
            match PasswordAuth::verify_password(
                &form.password,
                &user.password_hash,
                &frontend_state.app_state.security_config.argon2_params,
            ) {
                Ok(true) => {
                    debug!("Password verification successful for user: {}", user.id);

                    // Create JWT token
                    match JwtAuth::create_token(
                        &user.id,
                        &user.email,
                        &frontend_state.app_state.security_config.jwt_secret,
                        &frontend_state.app_state.security_config.jwt_issuer,
                    ) {
                        Ok(token) => {
                            debug!("JWT token created for user: {}", user.id);

                            // Create cookie
                            let cookie_value = format!(
                                "auth_token={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
                                token,
                                JwtAuth::token_expiration_secs(),
                                if frontend_state.app_state.security_config.secure_cookies {
                                    "; Secure"
                                } else {
                                    ""
                                }
                            );

                            // Return redirect to dashboard with cookie
                            if is_htmx_request {
                                // For HTMX requests, send redirect instruction
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header(SET_COOKIE, cookie_value)
                                    .header("HX-Redirect", "/dashboard")
                                    .header("Content-Type", "text/html")
                                    .body(r#"<div>Redirecting to dashboard...</div>"#.into())
                                    .unwrap()
                            } else {
                                // For regular form submissions, do a standard redirect
                                Response::builder()
                                    .status(StatusCode::SEE_OTHER)
                                    .header(SET_COOKIE, cookie_value)
                                    .header(LOCATION, "/dashboard")
                                    .body("".into())
                                    .unwrap()
                            }
                        }
                        Err(e) => {
                            warn!("Failed to create JWT token: {}", e);
                            render_login_with_error("Authentication system error").into_response()
                        }
                    }
                }
                Ok(false) => {
                    warn!("Invalid password for user: {}", user.email);
                    render_login_with_error("Invalid username or password").into_response()
                }
                Err(e) => {
                    warn!("Password verification error: {}", e);
                    render_login_with_error("Authentication system error").into_response()
                }
            }
        }
        Ok(None) => {
            warn!("Login attempt for non-existent email: {}", form.username);
            render_login_with_error("Invalid username or password").into_response()
        }
        Err(e) => {
            warn!("Database error during login: {}", e);
            render_login_with_error("System error during authentication").into_response()
        }
    }
}

/// Helper function to render login page with error
fn render_login_with_error(error_message: &str) -> Html<String> {
    let template = LoginTemplate {
        error_message: error_message.to_string(),
        user: UserInfo {
            id: String::new(),
            username: String::new(),
            is_admin: false,
        },
        logged_in: false, // Not logged in
    };

    match template.render() {
        Ok(html) => Html(html),
        Err(_) => Html("<html><body><h1>Template Error</h1></body></html>".to_string()),
    }
}

/// Streams list page
async fn streams_list_handler(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    // TODO: Extract user from cookie/session - for now use test user
    let user = UserInfo {
        id: "test".to_string(),
        username: "Test User".to_string(),
        is_admin: true,
    };

    // Fetch streams from database
    match fetch_streams(&frontend_state, None).await {
        Ok(streams) => {
            // Build streams grid HTML
            let streams_html = if streams.is_empty() {
                r#"<div class="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
                    <p class="text-lg mb-2">No streams found</p>
                    <p class="text-sm">Try adjusting your filter or check back later.</p>
                </div>"#.to_string()
            } else {
                let cards = streams.iter().map(|s| format!(
                    r#"<div class="bg-white rounded-lg shadow-sm border hover:shadow-md transition-shadow duration-200 overflow-hidden">
                        <div class="aspect-video bg-gray-100 flex items-center justify-center">
                            <a href="/streams/{}" class="w-full h-full flex items-center justify-center">
                                {}
                            </a>
                        </div>
                        <div class="p-4">
                            <div class="flex justify-between items-start mb-2">
                                <h3 class="font-semibold text-gray-800 truncate">{}</h3>
                                <span class="px-2 py-1 text-xs rounded-full {}">
                                    {}
                                </span>
                            </div>
                            <div class="flex justify-between items-center">
                                <p class="text-sm text-gray-500">Last seen: {}</p>
                                <a href="/streams/{}" class="text-blue-600 hover:text-blue-800 text-sm font-medium">View</a>
                            </div>
                        </div>
                    </div>"#,
                    s.id,
                    if s.status == "active" {
                        format!(r#"<img src="/api/stream/{}/thumbnail" alt="{}" class="w-full h-full object-cover">"#, s.stream_id, html_escape(&s.name))
                    } else {
                        "<span class=\"text-gray-500\">Offline</span>".to_string()
                    },
                    html_escape(&s.name),
                    if s.status == "active" { "bg-green-100 text-green-800" } else { "bg-gray-100 text-gray-600" },
                    if s.status == "active" { "Online" } else { "Offline" },
                    s.last_frame_at,
                    s.id
                )).collect::<Vec<_>>().join("");

                format!(
                    r#"<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">{}</div>"#,
                    cards
                )
            };

            // Full page HTML with navigation
            Html(format!(r#"<!DOCTYPE html>
<html><head><title>Live Streams</title><script src="https://cdn.tailwindcss.com"></script></head>
<body class="min-h-screen bg-slate-50">
    <nav class="bg-white border-b border-gray-300 px-8 py-4 flex justify-between items-center shadow-sm">
        <div class="flex items-center gap-8">
            <h1 class="text-xl font-bold text-blue-600">Glimpser</h1>
            <div class="flex items-center gap-4">
                <a href="/dashboard" class="text-sm font-medium text-gray-600 hover:text-blue-600">Dashboard</a>
                <a href="/streams" class="text-sm font-medium text-blue-600 border-b-2 border-blue-600 pb-1">Live Streams</a>
                <a href="/settings" class="text-sm font-medium text-gray-600 hover:text-blue-600">Settings</a>
            </div>
        </div>
        <div class="flex items-center gap-6">
            <span class="text-sm text-gray-500">Welcome, {}</span>
            <a href="/logout" class="px-4 py-2 bg-red-600 text-white rounded-md text-sm font-medium hover:bg-red-700">Logout</a>
        </div>
    </nav>

    <div class="p-8 max-w-6xl mx-auto w-full">
        <div class="flex flex-col md:flex-row justify-between items-start md:items-center gap-4 mb-8">
            <h2 class="text-2xl font-bold text-gray-800">Live Streams</h2>
            <div class="flex flex-col sm:flex-row items-stretch sm:items-center gap-4">
                <button class="px-6 py-3 bg-gray-500 text-white rounded-md font-medium hover:bg-gray-600">Refresh</button>
                <select class="px-3 py-3 border border-gray-300 rounded-md text-base">
                    <option value="">All Streams</option>
                    <option value="active">Active Only</option>
                    <option value="inactive">Inactive Only</option>
                </select>
            </div>
        </div>
        {}
    </div>
</body></html>"#, user.username, streams_html)).into_response()
        }
        Err(e) => {
            warn!("Failed to fetch streams: {}", e);
            Html(format!(
                r#"<!DOCTYPE html>
<html><head><title>Live Streams</title><script src="https://cdn.tailwindcss.com"></script></head>
<body class="bg-red-100 p-8">
    <h1>Error loading streams</h1>
    <p>{}</p>
    <a href="/dashboard" class="text-blue-600">Back to Dashboard</a>
</body></html>"#,
                e
            ))
            .into_response()
        }
    }
}

/// Stream detail page
async fn stream_detail_handler(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    // TODO: Extract user from cookie/session - for now use test user
    let user = UserInfo {
        id: "test".to_string(),
        username: "Test User".to_string(),
        is_admin: true,
    };

    // Fetch specific stream from database
    match fetch_single_stream(&frontend_state, &stream_id).await {
        Ok(Some(stream)) => {
            let template = StreamDetailTemplate {
                stream,
                user,
                logged_in: true,
            };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!("Template render error: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
                }
            }
        }
        Ok(None) => {
            // Stream not found - render error page
            Html(format!(r#"
                <!DOCTYPE html>
                <html><head><title>Stream Not Found</title><script src="https://cdn.tailwindcss.com"></script></head>
                <body class="min-h-screen bg-gray-900 flex items-center justify-center">
                    <div class="text-center text-white">
                        <h1 class="text-2xl font-bold mb-2">Stream Not Found</h1>
                        <p class="text-gray-400 mb-6">Stream {} could not be found.</p>
                        <a href="/streams" class="bg-blue-600 hover:bg-blue-700 text-white px-6 py-3 rounded-lg">Back to Streams</a>
                    </div>
                </body></html>
            "#, stream_id)).into_response()
        }
        Err(e) => {
            warn!("Failed to fetch stream {}: {}", stream_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load stream").into_response()
        }
    }
}

/// Admin page
async fn admin_handler(
    authenticated_user: AuthenticatedUser,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    // Get actual user info from the database
    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());
    let db_user = match user_repo.find_by_id(&authenticated_user.id).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            warn!(
                "Authenticated user not found in database: {}",
                authenticated_user.id
            );
            return Redirect::temporary("/login").into_response();
        }
        Err(e) => {
            warn!("Failed to load user from database: {}", e);
            return Redirect::temporary("/login").into_response();
        }
    };

    let user = UserInfo {
        id: db_user.id,
        username: db_user.username,
        is_admin: true,
    };

    // Fetch streams for admin interface
    let streams = fetch_streams(&frontend_state, None)
        .await
        .unwrap_or_default();

    // Fetch system settings
    use gl_db::repositories::settings::SettingsRepository;
    let settings_repo = SettingsRepository::new(frontend_state.app_state.db.pool());
    let settings = settings_repo.get_all().await.unwrap_or_default();

    // Build streams table HTML
    let streams_html = streams.iter().map(|s| format!(
        r#"<tr>
            <td class="px-6 py-4 whitespace-nowrap">
                <div class="text-sm font-medium text-gray-900">{}</div>
                <div class="text-sm text-gray-500">ID: {}</div>
            </td>
            <td class="px-6 py-4 whitespace-nowrap">
                <span class="px-2 inline-flex text-xs leading-5 font-semibold rounded-full {}">{}</span>
            </td>
            <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{}</td>
            <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium space-x-2">
                {}
                <a href="/settings/streams/{}/edit" class="text-indigo-600 hover:text-indigo-900">Edit</a>
                <button hx-delete="/api/settings/streams/{}" hx-confirm="Delete {}?" hx-target="closest tr" hx-swap="outerHTML" class="text-red-600 hover:text-red-900">Delete</button>
            </td>
        </tr>"#,
        html_escape(&s.name), s.id,
        if s.status == "active" { "bg-green-100 text-green-800" } else { "bg-gray-100 text-gray-800" },
        s.status,
        s.last_frame_at,
        // Start/Stop toggle button
        if s.status == "active" {
            format!("<button hx-post=\"/api/settings/streams/{}/stop\" hx-target=\"closest tr\" hx-swap=\"outerHTML\" class=\"text-orange-600 hover:text-orange-900\">Stop</button>", s.id)
        } else {
            format!("<button hx-post=\"/api/settings/streams/{}/start\" hx-target=\"closest tr\" hx-swap=\"outerHTML\" class=\"text-green-600 hover:text-green-900\">Start</button>", s.id)
        },
        s.id, s.id, html_escape(&s.name)
    )).collect::<Vec<_>>().join("");

    // Helper function to generate a clean settings input with HTML escaping

    let generate_setting_html = |setting: &gl_db::repositories::settings::Setting| -> String {
        let title = html_escape(setting.description.as_deref().unwrap_or(&setting.key));
        let description = html_escape(
            setting
                .description
                .as_deref()
                .unwrap_or("No description available"),
        );
        let default_text = html_escape(setting.default_value.as_deref().unwrap_or("none"));
        let escaped_key = html_escape(&setting.key);
        let escaped_value = html_escape(&setting.value);

        match setting.data_type.as_str() {
            "boolean" => {
                let is_checked = setting.value == "true";
                format!(
                    r#"
                    <div class="flex items-center justify-between py-4 border-b border-gray-200 last:border-b-0">
                        <div class="flex-1 pr-4">
                            <label for="{}" class="text-sm font-medium text-gray-900">{}</label>
                            <p class="text-xs text-gray-500 mt-1">{}</p>
                            <span class="text-xs text-gray-400">Default: {}</span>
                        </div>
                        <div class="flex items-center">
                            <input
                                type="checkbox"
                                id="{}"
                                {}
                                class="h-4 w-4 text-blue-600 focus:ring-blue-500 border-gray-300 rounded"
                                aria-label="{}"
                                onchange="updateSetting('{}', this.checked ? 'true' : 'false')"
                            />
                        </div>
                    </div>
                "#,
                    escaped_key,
                    title,
                    description,
                    default_text,
                    escaped_key,
                    if is_checked { "checked" } else { "" },
                    title,
                    escaped_key
                )
            }
            "float" => {
                let min_val = setting
                    .min_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "0".to_string());
                let max_val = setting
                    .max_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "100".to_string());
                let max_display = setting
                    .max_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "∞".to_string());
                format!(
                    r#"
                    <div class="flex items-center justify-between py-4 border-b border-gray-200 last:border-b-0">
                        <div class="flex-1 pr-4">
                            <label for="{}" class="text-sm font-medium text-gray-900">{}</label>
                            <p class="text-xs text-gray-500 mt-1">{}</p>
                            <span class="text-xs text-gray-400">Range: {} - {} | Default: {}</span>
                        </div>
                        <div class="flex items-center space-x-2">
                            <input
                                type="number"
                                id="{}"
                                value="{}"
                                min="{}"
                                max="{}"
                                step="0.01"
                                class="w-20 px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-blue-500 focus:border-blue-500"
                                aria-label="{}"
                                onchange="updateSetting('{}', this.value)"
                            />
                        </div>
                    </div>
                "#,
                    escaped_key,
                    title,
                    description,
                    min_val,
                    max_display,
                    default_text,
                    escaped_key,
                    escaped_value,
                    min_val,
                    max_val,
                    title,
                    escaped_key
                )
            }
            "integer" => {
                let min_val = setting
                    .min_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "0".to_string());
                let max_val = setting
                    .max_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "999999".to_string());
                let max_display = setting
                    .max_value
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "∞".to_string());
                format!(
                    r#"
                    <div class="flex items-center justify-between py-4 border-b border-gray-200 last:border-b-0">
                        <div class="flex-1 pr-4">
                            <label for="{}" class="text-sm font-medium text-gray-900">{}</label>
                            <p class="text-xs text-gray-500 mt-1">{}</p>
                            <span class="text-xs text-gray-400">Range: {} - {} | Default: {}</span>
                        </div>
                        <div class="flex items-center space-x-2">
                            <input
                                type="number"
                                id="{}"
                                value="{}"
                                min="{}"
                                max="{}"
                                class="w-20 px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-blue-500 focus:border-blue-500"
                                aria-label="{}"
                                onchange="updateSetting('{}', this.value)"
                            />
                        </div>
                    </div>
                "#,
                    escaped_key,
                    title,
                    description,
                    min_val,
                    max_display,
                    default_text,
                    escaped_key,
                    escaped_value,
                    min_val,
                    max_val,
                    title,
                    escaped_key
                )
            }
            _ => {
                format!(
                    r#"
                    <div class="flex items-center justify-between py-4 border-b border-gray-200 last:border-b-0">
                        <div class="flex-1 pr-4">
                            <label for="{}" class="text-sm font-medium text-gray-900">{}</label>
                            <p class="text-xs text-gray-500 mt-1">{}</p>
                            <span class="text-xs text-gray-400">Default: {}</span>
                        </div>
                        <div class="flex items-center space-x-2">
                            <input
                                type="text"
                                id="{}"
                                value="{}"
                                class="w-32 px-3 py-2 border border-gray-300 rounded-md text-sm focus:ring-blue-500 focus:border-blue-500"
                                aria-label="{}"
                                onchange="updateSetting('{}', this.value)"
                            />
                        </div>
                    </div>
                "#,
                    escaped_key,
                    title,
                    description,
                    default_text,
                    escaped_key,
                    escaped_value,
                    title,
                    escaped_key
                )
            }
        }
    };

    // Build properly categorized settings
    let image_processing_settings = settings
        .iter()
        .filter(|s| s.category == "image_processing")
        .map(generate_setting_html)
        .collect::<Vec<_>>()
        .join("");

    let storage_settings = settings
        .iter()
        .filter(|s| s.category == "storage")
        .map(generate_setting_html)
        .collect::<Vec<_>>()
        .join("");

    let capture_settings = settings
        .iter()
        .filter(|s| s.category == "capture")
        .map(generate_setting_html)
        .collect::<Vec<_>>()
        .join("");

    // Complete admin page HTML with tabbed interface
    Html(format!(r#"<!DOCTYPE html>
<html><head><title>Admin Panel</title><script src="https://cdn.tailwindcss.com"></script><script src="https://unpkg.com/htmx.org@1.9.10"></script>
<script>
// Tab functionality with validation
function showTab(tabName) {{
    const validTabs = ['image-processing', 'storage', 'capture', 'streams'];

    // Validate tab name
    if (!validTabs.includes(tabName)) {{
        console.error('Invalid tab name:', tabName);
        return;
    }}

    // Hide all tab contents
    validTabs.forEach(tab => {{
        const tabElement = document.getElementById(tab + '-tab');
        const btnElement = document.getElementById(tab + '-btn');
        if (tabElement && btnElement) {{
            tabElement.classList.add('hidden');
            btnElement.classList.remove('border-blue-500', 'text-blue-600');
            btnElement.classList.add('border-transparent', 'text-gray-500');
        }}
    }});

    // Show selected tab
    const targetTab = document.getElementById(tabName + '-tab');
    const targetBtn = document.getElementById(tabName + '-btn');
    if (targetTab && targetBtn) {{
        targetTab.classList.remove('hidden');
        targetBtn.classList.add('border-blue-500', 'text-blue-600');
        targetBtn.classList.remove('border-transparent', 'text-gray-500');
    }} else {{
        console.error('Tab elements not found for:', tabName);
    }}
}}

async function exportStreams() {{
    try {{
        const response = await fetch('/api/settings/streams/export', {{
            method: 'GET',
            headers: {{
                'Accept': 'application/json'
            }}
        }});

        if (response.ok) {{
            const data = await response.json();
            const blob = new Blob([JSON.stringify(data, null, 2)], {{ type: 'application/json' }});
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `streams-export-${{new Date().toISOString().split('T')[0]}}.json`;
            document.body.appendChild(a);
            a.click();
            document.body.removeChild(a);
            URL.revokeObjectURL(url);

            // Show success message
            showNotification('Streams exported successfully!', 'success');
        }} else {{
            showNotification('Failed to export streams: ' + response.statusText, 'error');
        }}
    }} catch (error) {{
        showNotification('Export error: ' + error.message, 'error');
    }}
}}

async function importStreams(input) {{
    const file = input.files[0];
    if (!file) return;

    try {{
        const text = await file.text();
        const data = JSON.parse(text);

        // Validate data structure
        if (!data.streams || !Array.isArray(data.streams)) {{
            showNotification('Invalid file format: missing streams array', 'error');
            return;
        }}

        const response = await fetch('/api/settings/streams/import', {{
            method: 'POST',
            headers: {{
                'Content-Type': 'application/json'
            }},
            body: JSON.stringify({{
                streams: data.streams,
                overwrite_mode: 'skip'  // Default to skip existing
            }})
        }});

        if (response.ok) {{
            const result = await response.json();
            showNotification(`Import completed! Imported: ${{result.imported}}, Skipped: ${{result.skipped}}`, 'success');
            location.reload(); // Refresh the page to show new streams
        }} else {{
            const error = await response.json();
            showNotification('Failed to import streams: ' + (error.error || response.statusText), 'error');
        }}
    }} catch (error) {{
        showNotification('Import error: ' + error.message, 'error');
    }} finally {{
        input.value = ''; // Clear the file input
    }}
}}

async function updateSetting(key, value) {{
    try {{
        const response = await fetch('/api/settings/config', {{
            method: 'PUT',
            headers: {{
                'Content-Type': 'application/json'
            }},
            body: JSON.stringify({{
                key: key,
                value: value
            }})
        }});

        if (response.ok) {{
            const result = await response.json();
            showNotification('Setting updated successfully', 'success');
            console.log('Setting updated:', result.message);
        }} else {{
            const error = await response.json();
            showNotification('Failed to update setting: ' + (error.error || response.statusText), 'error');
        }}
    }} catch (error) {{
        showNotification('Update error: ' + error.message, 'error');
    }}
}}

// Notification system with cleanup
let activeNotifications = [];

function showNotification(message, type) {{
    // Clean up old notifications if too many
    if (activeNotifications.length >= 5) {{
        const oldNotification = activeNotifications.shift();
        if (oldNotification && oldNotification.parentNode) {{
            oldNotification.remove();
        }}
    }}

    const notification = document.createElement('div');
    notification.className = `fixed top-4 right-4 px-6 py-3 rounded-lg shadow-lg z-50 ${{type === 'success' ? 'bg-green-500' : 'bg-red-500'}} text-white transition-opacity duration-300`;
    notification.textContent = message;
    notification.setAttribute('role', 'alert');
    notification.setAttribute('aria-live', 'polite');
    document.body.appendChild(notification);
    activeNotifications.push(notification);

    // Fade out and remove
    setTimeout(() => {{
        notification.style.opacity = '0';
        setTimeout(() => {{
            if (notification.parentNode) {{
                notification.remove();
                const index = activeNotifications.indexOf(notification);
                if (index > -1) {{
                    activeNotifications.splice(index, 1);
                }}
            }}
        }}, 300);
    }}, 3000);
}}
</script>
</head>
<body class="min-h-screen bg-slate-50">
    <nav class="bg-white border-b border-gray-300 px-8 py-4 flex justify-between items-center shadow-sm">
        <div class="flex items-center gap-8">
            <h1 class="text-xl font-bold text-blue-600">Glimpser</h1>
            <div class="flex items-center gap-4">
                <a href="/dashboard" class="text-sm font-medium text-gray-600 hover:text-blue-600">Dashboard</a>
                <a href="/streams" class="text-sm font-medium text-gray-600 hover:text-blue-600">Live Streams</a>
                <a href="/settings" class="text-sm font-medium text-blue-600 border-b-2 border-blue-600 pb-1">Settings</a>
            </div>
        </div>
        <div class="flex items-center gap-6">
            <span class="text-sm text-gray-500">Welcome, {}</span>
            <a href="/logout" class="px-4 py-2 bg-red-600 text-white rounded-md text-sm font-medium hover:bg-red-700">Logout</a>
        </div>
    </nav>

    <div class="p-8 max-w-6xl mx-auto w-full">
        <div class="flex justify-between items-center mb-8">
            <h2 class="text-2xl font-bold text-gray-800">Settings</h2>
            <div class="bg-yellow-100 text-yellow-800 px-4 py-2 rounded-md text-sm font-medium">Administrator privileges required</div>
        </div>

        <!-- Tab Navigation -->
        <div class="border-b border-gray-200 mb-6">
            <nav class="-mb-px flex space-x-8" role="tablist" aria-label="Settings categories">
                <button id="image-processing-btn" onclick="showTab('image-processing')" class="whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm border-blue-500 text-blue-600" role="tab" aria-selected="true" aria-controls="image-processing-tab">
                    Image Processing
                </button>
                <button id="storage-btn" onclick="showTab('storage')" class="whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300" role="tab" aria-selected="false" aria-controls="storage-tab">
                    Storage
                </button>
                <button id="capture-btn" onclick="showTab('capture')" class="whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300" role="tab" aria-selected="false" aria-controls="capture-tab">
                    Capture
                </button>
                <button id="streams-btn" onclick="showTab('streams')" class="whitespace-nowrap py-2 px-1 border-b-2 font-medium text-sm border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300" role="tab" aria-selected="false" aria-controls="streams-tab">
                    Streams
                </button>
            </nav>
        </div>

        <!-- Tab Contents -->

        <!-- Image Processing Tab -->
        <div id="image-processing-tab" class="bg-white shadow rounded-lg" role="tabpanel" aria-labelledby="image-processing-btn">
            <div class="px-6 py-8">
                <div class="mb-6">
                    <h3 class="text-lg font-medium text-gray-900 mb-2">Image Processing Settings</h3>
                    <p class="text-sm text-gray-600">Configure perceptual hash similarity detection and image analysis parameters.</p>
                </div>
                <div class="space-y-6">
                    {}
                </div>
            </div>
        </div>

        <!-- Storage Tab -->
        <div id="storage-tab" class="bg-white shadow rounded-lg hidden" role="tabpanel" aria-labelledby="storage-btn">
            <div class="px-6 py-8">
                <div class="mb-6">
                    <h3 class="text-lg font-medium text-gray-900 mb-2">Storage Settings</h3>
                    <p class="text-sm text-gray-600">Configure data retention policies and automatic cleanup processes.</p>
                </div>
                <div class="space-y-6">
                    {}
                </div>
            </div>
        </div>

        <!-- Capture Tab -->
        <div id="capture-tab" class="bg-white shadow rounded-lg hidden" role="tabpanel" aria-labelledby="capture-btn">
            <div class="px-6 py-8">
                <div class="mb-6">
                    <h3 class="text-lg font-medium text-gray-900 mb-2">Capture Settings</h3>
                    <p class="text-sm text-gray-600">Configure stream capture intervals and processing options.</p>
                </div>
                <div class="space-y-6">
                    {}
                </div>
            </div>
        </div>

        <!-- Streams Tab -->
        <div id="streams-tab" class="bg-white shadow rounded-lg hidden" role="tabpanel" aria-labelledby="streams-btn">
            <div class="px-6 py-8">
                <div class="flex justify-between items-center mb-6">
                    <div>
                        <h3 class="text-lg font-medium text-gray-900 mb-2">Stream Configuration</h3>
                        <p class="text-sm text-gray-600">Manage your surveillance streams and their capture settings.</p>
                    </div>
                    <div class="flex space-x-3">
                        <button onclick="document.getElementById('importFile').click()" class="bg-green-600 hover:bg-green-700 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors">
                            📥 Import
                        </button>
                        <input type="file" id="importFile" accept=".json" onchange="importStreams(this)" style="display: none;" />
                        <button onclick="exportStreams()" class="bg-gray-600 hover:bg-gray-700 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors">
                            📤 Export
                        </button>
                        <a href="/settings/streams/new" class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors">
                            ➕ Add Stream
                        </a>
                    </div>
                </div>
                <div class="overflow-x-auto">
                    <table class="min-w-full divide-y divide-gray-200">
                        <thead class="bg-gray-50">
                            <tr>
                                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Stream</th>
                                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Status</th>
                                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Last Execution</th>
                                <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Actions</th>
                            </tr>
                        </thead>
                        <tbody class="bg-white divide-y divide-gray-200">{}</tbody>
                    </table>
                </div>
            </div>
        </div>
    </div>
</body></html>"#, user.username, image_processing_settings, storage_settings, capture_settings, streams_html)).into_response()
}

/// HTMX fragment for streams list
async fn htmx_streams_fragment(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    match fetch_streams(&frontend_state, None).await {
        Ok(streams) => {
            let template = StreamsGridFragment { streams };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!("Template render error: {}", e);
                    let error_template = StreamsErrorFragment {
                        error_message: "Template error".to_string(),
                    };
                    Html(
                        error_template
                            .render()
                            .unwrap_or_else(|_| "Error".to_string()),
                    )
                    .into_response()
                }
            }
        }
        Err(e) => {
            warn!("Failed to fetch streams for HTMX: {}", e);
            let error_template = StreamsErrorFragment {
                error_message: e.to_string(),
            };
            Html(
                error_template
                    .render()
                    .unwrap_or_else(|_| "Error".to_string()),
            )
            .into_response()
        }
    }
}

/// Helper function to fetch streams from database
async fn fetch_streams(
    frontend_state: &FrontendState,
    filter: Option<&str>,
) -> Result<Vec<StreamInfo>, Error> {
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Fetch all streams (in the future we can filter by user)
    let db_streams = stream_repo
        .list(None, 0, 100)
        .await
        .map_err(|e| Error::Database(format!("Failed to fetch streams: {}", e)))?;

    // Convert to frontend StreamInfo format
    let streams: Vec<StreamInfo> = db_streams
        .into_iter()
        .filter_map(|stream| {
            // Determine stream status from execution_status
            let status = stream.execution_status.as_deref().unwrap_or("inactive");

            // Apply filter if provided
            if let Some(f) = filter {
                match f {
                    "active" if status != "active" => return None,
                    "inactive" if status == "active" => return None,
                    _ => {}
                }
            }

            Some(StreamInfo {
                id: stream.id.clone(),
                stream_id: stream.id, // Use stream.id as stream_id for now
                name: stream.name,
                status: status.to_string(),
                last_frame_at: stream
                    .last_executed_at
                    .unwrap_or_else(|| "Never".to_string()),
            })
        })
        .collect();

    Ok(streams)
}

// fetch_admin_users removed - not currently used in admin interface

/// Parse stream config JSON into form-friendly struct
fn parse_stream_config_for_edit(db_stream: &gl_db::Stream) -> StreamConfigForEdit {
    // Parse the JSON config
    let config: serde_json::Value = serde_json::from_str(&db_stream.config).unwrap_or_default();
    let kind = config
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("file")
        .to_string();

    // Handle legacy field names
    let snapshot_interval = config
        .get("snapshot_interval_seconds")
        .or_else(|| config.get("capture_interval_seconds"))
        .and_then(|c| c.as_u64())
        .unwrap_or(30) as u32;

    StreamConfigForEdit {
        id: db_stream.id.clone(),
        name: db_stream.name.clone(),
        description: db_stream.description.clone().unwrap_or_default(),
        config_kind: kind.clone(),
        is_default: db_stream.is_default,

        // Common fields
        snapshot_interval,
        width: config.get("width").and_then(|w| w.as_u64()).unwrap_or(1920) as u32,
        height: config
            .get("height")
            .and_then(|h| h.as_u64())
            .unwrap_or(1080) as u32,

        // RTSP fields
        rtsp_url: if kind == "rtsp" {
            config
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        },

        // File fields
        file_path: config
            .get("file_path")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string(),

        // FFmpeg fields
        ffmpeg_source: config
            .get("source")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        ffmpeg_args: config
            .get("args")
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_string(),

        // Website fields
        website_url: if kind == "website" {
            config
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        },
        website_width: config.get("width").and_then(|w| w.as_u64()).unwrap_or(1920) as u32,
        website_height: config
            .get("height")
            .and_then(|h| h.as_u64())
            .unwrap_or(1080) as u32,
        website_timeout: config.get("timeout").and_then(|t| t.as_u64()).unwrap_or(30) as u32,
        element_selector: config
            .get("element_selector")
            .and_then(|e| e.as_str())
            .unwrap_or("")
            .to_string(),
        selector_type: config
            .get("selector_type")
            .and_then(|s| s.as_str())
            .unwrap_or("css")
            .to_string(),
        headless: config
            .get("headless")
            .and_then(|h| h.as_bool())
            .unwrap_or(true),
        stealth: config
            .get("stealth")
            .and_then(|s| s.as_bool())
            .unwrap_or(true),
        auth_username: config
            .get("auth_username")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string(),
        auth_password: config
            .get("auth_password")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),

        // YouTube fields (handle both legacy "yt" and new "youtube" kinds)
        youtube_url: if kind == "youtube" || kind == "yt" {
            config
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        },
        youtube_format: config
            .get("format")
            .and_then(|q| q.as_str())
            .unwrap_or("best")
            .to_string(),
        youtube_is_live: config
            .get("is_live")
            .and_then(|l| l.as_bool())
            .unwrap_or(false),

        // Legacy field for backward compatibility
        capture_interval: snapshot_interval,
    }
}

/// Helper function to fetch a single stream from database
async fn fetch_single_stream(
    frontend_state: &FrontendState,
    stream_id: &str,
) -> Result<Option<StreamInfo>, Error> {
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Fetch specific stream
    match stream_repo.find_by_id(stream_id).await {
        Ok(Some(stream)) => {
            // Determine stream status from execution_status
            let status = stream.execution_status.as_deref().unwrap_or("inactive");

            Ok(Some(StreamInfo {
                id: stream.id.clone(),
                stream_id: stream.id, // Use stream.id as stream_id for now
                name: stream.name,
                status: status.to_string(),
                last_frame_at: stream
                    .last_executed_at
                    .unwrap_or_else(|| "Never".to_string()),
            }))
        }
        Ok(None) => Ok(None),
        Err(e) => Err(Error::Database(format!("Failed to fetch stream: {}", e))),
    }
}

/// HTMX handler for individual stream card updates
async fn htmx_stream_card_handler(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    match fetch_single_stream(&frontend_state, &stream_id).await {
        Ok(Some(stream)) => {
            let template = StreamCard { stream };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!("Template render error for stream {}: {}", stream_id, e);
                    Html(format!(
                        r#"<div class="bg-red-100 p-4 rounded">Error loading stream {}</div>"#,
                        stream_id
                    ))
                    .into_response()
                }
            }
        }
        Ok(None) => Html(format!(
            r#"<div class="bg-gray-100 p-4 rounded">Stream {} not found</div>"#,
            stream_id
        ))
        .into_response(),
        Err(e) => {
            warn!(
                "Failed to fetch stream {} for card update: {}",
                stream_id, e
            );
            Html(format!(
                r#"<div class="bg-red-100 p-4 rounded">Error: {}</div>"#,
                e
            ))
            .into_response()
        }
    }
}

/// HTMX fragment for stream status updates
async fn htmx_stream_status_fragment(
    Path(_id): Path<String>,
    State(_state): State<FrontendState>,
) -> impl IntoResponse {
    // TODO: Return HTML fragment for stream status
    Html(r#"<span class="badge badge-success">Live</span>"#).into_response()
}

/// Admin endpoint to delete a stream
async fn admin_delete_stream(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    match stream_repo.delete(&stream_id).await {
        Ok(true) => {
            debug!("Stream {} deleted successfully", stream_id);
            // Return empty response - HTMX will remove the table row
            StatusCode::OK.into_response()
        }
        Ok(false) => {
            warn!("Stream {} not found for deletion", stream_id);
            (StatusCode::NOT_FOUND, "Stream not found").into_response()
        }
        Err(e) => {
            warn!("Failed to delete stream {}: {}", stream_id, e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete stream").into_response()
        }
    }
}

/// Stream creation page
async fn admin_stream_new_page() -> impl IntoResponse {
    let template = StreamFormTemplate {
        user: UserInfo {
            id: "temp".to_string(),
            username: "Admin User".to_string(),
            is_admin: true,
        },
        logged_in: true,
        error_message: String::new(),
        form_title: "Create New Stream".to_string(),
        form_action: "/settings/streams/new".to_string(),
        submit_button_text: "Create Stream".to_string(),
        stream_data: StreamConfigForEdit::empty(),
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            warn!("Stream form template render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
}

/// Handle stream creation form submission
async fn admin_stream_create(
    authenticated_user: AuthenticatedUser,
    State(frontend_state): State<FrontendState>,
    Form(form): Form<StreamCreateForm>,
) -> impl IntoResponse {
    use gl_db::CreateStreamRequest;

    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Build JSON config based on stream type
    let snapshot_interval = form
        .snapshot_interval
        .or(form.capture_interval)
        .unwrap_or(30);
    let config_json = match form.config_kind.as_str() {
        "rtsp" => {
            let url = form.rtsp_url.unwrap_or_default();
            let width = form.rtsp_width.unwrap_or(1920);
            let height = form.rtsp_height.unwrap_or(1080);
            serde_json::json!({
                "kind": "rtsp",
                "url": url,
                "width": width,
                "height": height,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "file" => {
            let file_path = form.file_path.unwrap_or_default();
            serde_json::json!({
                "kind": "file",
                "file_path": file_path,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "website" => {
            let url = form.website_url.unwrap_or_default();
            let width = form.website_width.unwrap_or(1920);
            let height = form.website_height.unwrap_or(1080);
            let timeout = form.website_timeout.unwrap_or(30);
            let headless = form.headless.is_some();
            let stealth = form.stealth.is_some();
            let selector_type = form.selector_type.unwrap_or_else(|| "css".to_string());

            let mut config = serde_json::json!({
                "kind": "website",
                "url": url,
                "width": width,
                "height": height,
                "timeout": timeout,
                "headless": headless,
                "stealth": stealth,
                "selector_type": selector_type,
                "snapshot_interval_seconds": snapshot_interval
            });

            if let Some(selector) = form.element_selector.filter(|s| !s.is_empty()) {
                config["element_selector"] = serde_json::Value::String(selector);
            }
            if let Some(username) = form.auth_username.filter(|s| !s.is_empty()) {
                config["auth_username"] = serde_json::Value::String(username);
            }
            if let Some(password) = form.auth_password.filter(|s| !s.is_empty()) {
                config["auth_password"] = serde_json::Value::String(password);
            }

            config.to_string()
        }
        "ffmpeg" => {
            let source = form.ffmpeg_source.unwrap_or_default();
            let args = form.ffmpeg_args.unwrap_or_default();
            let width = form.ffmpeg_width.unwrap_or(1920);
            let height = form.ffmpeg_height.unwrap_or(1080);
            serde_json::json!({
                "kind": "ffmpeg",
                "source": source,
                "args": args,
                "width": width,
                "height": height,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "youtube" => {
            let url = form.youtube_url.unwrap_or_default();
            let format = form.youtube_format.unwrap_or_else(|| "best".to_string());
            let is_live = form.youtube_is_live.is_some();

            serde_json::json!({
                "kind": "youtube",
                "url": url,
                "format": format,
                "is_live": is_live,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        _ => {
            return Html("Invalid stream type").into_response();
        }
    };

    let create_request = CreateStreamRequest {
        user_id: authenticated_user.id.clone(), // Use authenticated user ID
        name: form.name,
        description: form.description.filter(|s| !s.is_empty()),
        config: config_json,
        is_default: form.is_default.is_some(),
    };

    match stream_repo.create(create_request).await {
        Ok(_stream) => {
            debug!("Stream created successfully");
            // Redirect back to settings
            axum::response::Redirect::to("/settings").into_response()
        }
        Err(e) => {
            warn!("Failed to create stream: {}", e);
            // Show error on simple create page
            Html(format!(r#"
                <!DOCTYPE html>
                <html><head><title>Create Stream</title><script src="https://cdn.tailwindcss.com"></script></head>
                <body class="bg-slate-50 p-8">
                    <div class="max-w-2xl mx-auto">
                        <a href="/settings" class="text-blue-600">← Back to Settings</a>
                        <h1 class="text-3xl font-bold mt-4 mb-8">Create New Stream</h1>
                        <div class="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-6">
                            Failed to create stream: {}
                        </div>
                        <div class="bg-white p-6 rounded-lg shadow">
                            <p class="text-gray-500">Please try again or contact support.</p>
                        </div>
                    </div>
                </body></html>
            "#, e)).into_response()
        }
    }
}

/// Stream edit page
async fn admin_stream_edit_page(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    match stream_repo.find_by_id(&stream_id).await {
        Ok(Some(db_stream)) => {
            // Parse the JSON config to populate form fields
            let config_for_edit = parse_stream_config_for_edit(&db_stream);

            let template = StreamFormTemplate {
                user: UserInfo {
                    id: "temp".to_string(),
                    username: "Admin User".to_string(),
                    is_admin: true,
                },
                logged_in: true,
                error_message: String::new(),
                form_title: format!("Edit Stream: {}", config_for_edit.name),
                form_action: format!("/settings/streams/{}/edit", stream_id),
                submit_button_text: "Update Stream".to_string(),
                stream_data: config_for_edit,
            };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!("Stream form template render error: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        Err(e) => {
            warn!("Failed to fetch stream for editing: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load stream").into_response()
        }
    }
}

/// Handle stream update form submission
async fn admin_stream_update(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
    Form(form): Form<StreamCreateForm>,
) -> impl IntoResponse {
    use gl_db::UpdateStreamRequest;

    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Build JSON config based on stream type (same logic as create)
    let snapshot_interval = form
        .snapshot_interval
        .or(form.capture_interval)
        .unwrap_or(30);
    let config_json = match form.config_kind.as_str() {
        "rtsp" => {
            let url = form.rtsp_url.unwrap_or_default();
            let width = form.rtsp_width.unwrap_or(1920);
            let height = form.rtsp_height.unwrap_or(1080);
            serde_json::json!({
                "kind": "rtsp",
                "url": url,
                "width": width,
                "height": height,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "file" => {
            let file_path = form.file_path.unwrap_or_default();
            serde_json::json!({
                "kind": "file",
                "file_path": file_path,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "website" => {
            let url = form.website_url.unwrap_or_default();
            let width = form.website_width.unwrap_or(1920);
            let height = form.website_height.unwrap_or(1080);
            let timeout = form.website_timeout.unwrap_or(30);
            let headless = form.headless.is_some();
            let stealth = form.stealth.is_some();
            let selector_type = form.selector_type.unwrap_or_else(|| "css".to_string());

            let mut config = serde_json::json!({
                "kind": "website",
                "url": url,
                "width": width,
                "height": height,
                "timeout": timeout,
                "headless": headless,
                "stealth": stealth,
                "selector_type": selector_type,
                "snapshot_interval_seconds": snapshot_interval
            });

            if let Some(selector) = form.element_selector.filter(|s| !s.is_empty()) {
                config["element_selector"] = serde_json::Value::String(selector);
            }
            if let Some(username) = form.auth_username.filter(|s| !s.is_empty()) {
                config["auth_username"] = serde_json::Value::String(username);
            }
            if let Some(password) = form.auth_password.filter(|s| !s.is_empty()) {
                config["auth_password"] = serde_json::Value::String(password);
            }

            config.to_string()
        }
        "ffmpeg" => {
            let source = form.ffmpeg_source.unwrap_or_default();
            let args = form.ffmpeg_args.unwrap_or_default();
            let width = form.ffmpeg_width.unwrap_or(1920);
            let height = form.ffmpeg_height.unwrap_or(1080);
            serde_json::json!({
                "kind": "ffmpeg",
                "source": source,
                "args": args,
                "width": width,
                "height": height,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        "youtube" => {
            let url = form.youtube_url.unwrap_or_default();
            let format = form.youtube_format.unwrap_or_else(|| "best".to_string());
            let is_live = form.youtube_is_live.is_some();

            serde_json::json!({
                "kind": "youtube",
                "url": url,
                "format": format,
                "is_live": is_live,
                "snapshot_interval_seconds": snapshot_interval
            })
            .to_string()
        }
        _ => {
            return Html("Invalid stream type").into_response();
        }
    };

    let update_request = UpdateStreamRequest {
        name: Some(form.name),
        description: form.description.filter(|s| !s.is_empty()),
        config: Some(config_json),
        is_default: Some(form.is_default.is_some()),
    };

    match stream_repo.update(&stream_id, update_request).await {
        Ok(_) => {
            debug!("Stream {} updated successfully", stream_id);
            // Redirect back to settings
            axum::response::Redirect::to("/settings").into_response()
        }
        Err(e) => {
            warn!("Failed to update stream {}: {}", stream_id, e);

            // Re-fetch the stream and show error
            if let Ok(Some(db_stream)) = stream_repo.find_by_id(&stream_id).await {
                let config_for_edit = parse_stream_config_for_edit(&db_stream);
                // Show error on edit page
                Html(format!(r#"
                    <!DOCTYPE html>
                    <html><head><title>Edit Stream</title><script src="https://cdn.tailwindcss.com"></script></head>
                    <body class="bg-slate-50 p-8">
                        <div class="max-w-2xl mx-auto">
                            <a href="/settings" class="text-blue-600">← Back to Settings</a>
                            <h1 class="text-3xl font-bold mt-4 mb-8">Edit Stream: {}</h1>
                            <div class="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-6">
                                Failed to update stream: {}
                            </div>
                            <div class="bg-white p-6 rounded-lg shadow">
                                <p class="text-gray-500">Please try again or contact support.</p>
                            </div>
                        </div>
                    </body></html>
                "#, config_for_edit.name, e)).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to update stream").into_response()
            }
        }
    }
}

/// Start a stream
async fn admin_start_stream(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    debug!("Starting stream: {}", stream_id);

    // Actually start the stream using capture manager
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Start the capture process first
    debug!("Attempting to start capture for stream: {}", stream_id);
    match frontend_state
        .app_state
        .capture_manager
        .start_stream(&stream_id)
        .await
    {
        Ok(()) => {
            debug!("✅ Capture started successfully for stream: {}", stream_id);
            // Update database status after successful capture start
            match stream_repo
                .update_execution_status(&stream_id, "active", None)
                .await
            {
                Ok(true) => {
                    // Fetch the updated stream and return the table row
                    match fetch_single_stream(&frontend_state, &stream_id).await {
                        Ok(Some(stream)) => {
                            let row_html = format!(
                                r#"<tr>
                        <td class="px-6 py-4 whitespace-nowrap">
                            <div class="text-sm font-medium text-gray-900">{}</div>
                            <div class="text-sm text-gray-500">ID: {}</div>
                        </td>
                        <td class="px-6 py-4 whitespace-nowrap">
                            <span class="px-2 inline-flex text-xs leading-5 font-semibold rounded-full bg-green-100 text-green-800">active</span>
                        </td>
                        <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{}</td>
                        <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium space-x-2">
                            <button hx-post="/api/settings/streams/{}/stop" hx-target="closest tr" hx-swap="outerHTML" class="text-orange-600 hover:text-orange-900">Stop</button>
                            <a href="/settings/streams/{}/edit" class="text-indigo-600 hover:text-indigo-900">Edit</a>
                            <button hx-delete="/api/settings/streams/{}" hx-confirm="Delete {}?" hx-target="closest tr" hx-swap="outerHTML" class="text-red-600 hover:text-red-900">Delete</button>
                        </td>
                    </tr>"#,
                                stream.name,
                                stream.id,
                                stream.last_frame_at,
                                stream.id,
                                stream.id,
                                stream.id,
                                stream.name
                            );
                            Html(row_html).into_response()
                        }
                        _ => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to fetch updated stream",
                        )
                            .into_response(),
                    }
                }
                Ok(false) => {
                    warn!("Stream {} not found for database update", stream_id);
                    (StatusCode::NOT_FOUND, "Stream not found").into_response()
                }
                Err(e) => {
                    warn!("Failed to update stream {} status: {}", stream_id, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to update stream status",
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            warn!("❌ Failed to start capture for stream {}: {}", stream_id, e);
            // Capture start failed - make sure database status reflects this
            let _ = stream_repo
                .update_execution_status(&stream_id, "inactive", None)
                .await;
            debug!(
                "Reset stream {} status to inactive due to capture start failure",
                stream_id
            );

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start capture: {}", e),
            )
                .into_response()
        }
    }
}

/// Stop a stream
async fn admin_stop_stream(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    debug!("Stopping stream: {}", stream_id);

    // Actually stop the stream using capture manager
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Stop the capture process first
    match frontend_state
        .app_state
        .capture_manager
        .stop_stream(&stream_id)
        .await
    {
        Ok(()) => {
            debug!("Capture stopped successfully for stream: {}", stream_id);
            // Update database status after successful capture stop
            match stream_repo
                .update_execution_status(&stream_id, "inactive", None)
                .await
            {
                Ok(true) => {
                    // Fetch the updated stream and return the table row
                    match fetch_single_stream(&frontend_state, &stream_id).await {
                        Ok(Some(stream)) => {
                            let row_html = format!(
                                r#"<tr>
                        <td class="px-6 py-4 whitespace-nowrap">
                            <div class="text-sm font-medium text-gray-900">{}</div>
                            <div class="text-sm text-gray-500">ID: {}</div>
                        </td>
                        <td class="px-6 py-4 whitespace-nowrap">
                            <span class="px-2 inline-flex text-xs leading-5 font-semibold rounded-full bg-gray-100 text-gray-800">inactive</span>
                        </td>
                        <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{}</td>
                        <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium space-x-2">
                            <button hx-post="/api/settings/streams/{}/start" hx-target="closest tr" hx-swap="outerHTML" class="text-green-600 hover:text-green-900">Start</button>
                            <a href="/settings/streams/{}/edit" class="text-indigo-600 hover:text-indigo-900">Edit</a>
                            <button hx-delete="/api/settings/streams/{}" hx-confirm="Delete {}?" hx-target="closest tr" hx-swap="outerHTML" class="text-red-600 hover:text-red-900">Delete</button>
                        </td>
                    </tr>"#,
                                stream.name,
                                stream.id,
                                stream.last_frame_at,
                                stream.id,
                                stream.id,
                                stream.id,
                                stream.name
                            );
                            Html(row_html).into_response()
                        }
                        _ => (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to fetch updated stream",
                        )
                            .into_response(),
                    }
                }
                Ok(false) => {
                    warn!("Stream {} not found for database update", stream_id);
                    (StatusCode::NOT_FOUND, "Stream not found").into_response()
                }
                Err(e) => {
                    warn!("Failed to update stream {} status: {}", stream_id, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to update stream status",
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            warn!("Failed to stop capture for stream {}: {}", stream_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to stop capture: {}", e),
            )
                .into_response()
        }
    }
}

/// Stream snapshot API endpoint with ETag caching support
async fn stream_snapshot(
    Path(stream_id): Path<String>,
    headers: HeaderMap,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    use axum::response::Response;

    tracing::debug!("Stream snapshot requested for: {}", stream_id);

    // Helper function to create response with ETag
    let create_response = |bytes: Vec<u8>| -> Response {
        let etag = generate_etag(&bytes);

        // Check if client has matching ETag (304 Not Modified)
        if let Some(if_none_match) = headers.get(IF_NONE_MATCH) {
            if let Ok(client_etag) = if_none_match.to_str() {
                if client_etag == etag {
                    return Response::builder()
                        .status(StatusCode::NOT_MODIFIED)
                        .header(ETAG, etag)
                        .header(CACHE_CONTROL, "private, max-age=60") // Cache for 1 minute
                        .body(Body::empty())
                        .unwrap();
                }
            }
        }

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "image/jpeg")
            .header(ETAG, etag)
            .header(CACHE_CONTROL, "private, max-age=60") // Cache for 1 minute
            .body(Body::from(bytes))
            .unwrap()
    };

    // Try to get snapshot from capture manager first
    match frontend_state
        .app_state
        .capture_manager
        .get_latest_snapshot(&stream_id)
        .await
    {
        Ok(snapshot_bytes) => create_response(snapshot_bytes.to_vec()).into_response(),
        Err(_) => {
            // Fall back to direct capture if no cached snapshot
            match take_snapshot_direct(&frontend_state, &stream_id).await {
                Ok(jpeg_bytes) => create_response(jpeg_bytes).into_response(),
                Err(e) => {
                    warn!("Failed to capture snapshot for stream {}: {}", stream_id, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to capture snapshot: {}", e),
                    )
                        .into_response()
                }
            }
        }
    }
}

/// Stream thumbnail API endpoint
async fn stream_thumbnail(
    Path(stream_id): Path<String>,
    headers: HeaderMap,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    tracing::debug!("Stream thumbnail requested for: {}", stream_id);

    // Thumbnail is just a cached snapshot - delegate to snapshot endpoint
    stream_snapshot(Path(stream_id), headers, State(frontend_state)).await
}

/// MJPEG streaming API endpoint - Real multipart streaming
async fn stream_mjpeg(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    tracing::debug!("MJPEG stream requested for: {}", stream_id);

    // Check if stream exists and is active
    match fetch_single_stream(&frontend_state, &stream_id).await {
        Ok(Some(stream)) if stream.status == "active" => {
            // Subscribe to the real-time frame broadcast
            match frontend_state
                .app_state
                .capture_manager
                .subscribe_to_stream(&stream_id)
                .await
            {
                Some(mut frame_receiver) => {
                    tracing::debug!("🎥 Starting real MJPEG stream for: {}", stream_id);

                    // Create the multipart MJPEG stream
                    let boundary = "frame";
                    let stream = async_stream::stream! {
                        // Send initial boundary
                        yield Ok::<bytes::Bytes, std::convert::Infallible>(bytes::Bytes::from(format!("\r\n--{}\r\n", boundary)));

                        loop {
                            match frame_receiver.recv().await {
                                Ok(frame_bytes) => {
                                    // Send MJPEG frame with proper headers
                                    let frame_header = format!(
                                        "Content-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                                        frame_bytes.len()
                                    );

                                    yield Ok(bytes::Bytes::from(frame_header));
                                    yield Ok(frame_bytes);
                                    yield Ok(bytes::Bytes::from(format!("\r\n--{}\r\n", boundary)));
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                                    if should_log_mjpeg_lag() {
                                        tracing::warn!("MJPEG stream {} lagged, missed {} frames (sampling 1/10)", stream_id, missed);
                                    }
                                    continue;
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                    tracing::debug!("MJPEG stream {} ended - capture stopped", stream_id);
                                    break;
                                }
                            }
                        }
                    };

                    Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            "content-type",
                            format!("multipart/x-mixed-replace; boundary={}", boundary),
                        )
                        .header("cache-control", "no-cache, no-store, must-revalidate")
                        .header("pragma", "no-cache")
                        .header("expires", "0")
                        .header("connection", "keep-alive")
                        .body(Body::from_stream(stream))
                        .unwrap()
                        .into_response()
                }
                None => {
                    tracing::warn!(
                        "Stream {} is marked active but not running in capture manager",
                        stream_id
                    );
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Stream capture not running",
                    )
                        .into_response()
                }
            }
        }
        Ok(Some(_)) => (StatusCode::SERVICE_UNAVAILABLE, "Stream is not active").into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

/// Stream start API endpoint
async fn stream_start(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    tracing::info!("Stream start requested for: {}", stream_id);

    // Use the existing start logic from admin_start_stream
    admin_start_stream(Path(stream_id), State(frontend_state)).await
}

/// Stream stop API endpoint
async fn stream_stop(
    Path(stream_id): Path<String>,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    tracing::info!("Stream stop requested for: {}", stream_id);

    // Use the existing stop logic from admin_stop_stream
    admin_stop_stream(Path(stream_id), State(frontend_state)).await
}

/// Take a direct snapshot from a stream (based on Actix-web implementation)
/// API: Get all settings
async fn api_get_settings(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    use gl_db::repositories::settings::SettingsRepository;

    let settings_repo = SettingsRepository::new(frontend_state.app_state.db.pool());

    match settings_repo.get_all().await {
        Ok(settings) => Json(settings).into_response(),
        Err(e) => {
            warn!("Failed to fetch settings: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to fetch settings"
                })),
            )
                .into_response()
        }
    }
}

/// API: Update a setting
async fn api_update_setting(
    State(frontend_state): State<FrontendState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    use gl_db::repositories::settings::{SettingsRepository, UpdateSettingRequest};

    // Extract key and value from payload
    let key = match payload.get("key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Missing 'key' field"
                })),
            )
                .into_response()
        }
    };

    let value = match payload.get("value").and_then(|v| v.as_str()) {
        Some(v) => v.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Missing 'value' field"
                })),
            )
                .into_response()
        }
    };

    let settings_repo = SettingsRepository::new(frontend_state.app_state.db.pool());
    let request = UpdateSettingRequest {
        key: key.to_string(),
        value,
    };

    match settings_repo.update(request).await {
        Ok(_) => Json(serde_json::json!({
            "success": true,
            "message": "Setting updated successfully"
        }))
        .into_response(),
        Err(e) => {
            warn!("Failed to update setting {}: {}", key, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to update setting: {}", e)
                })),
            )
                .into_response()
        }
    }
}

async fn take_snapshot_direct(
    frontend_state: &FrontendState,
    stream_id: &str,
) -> Result<Vec<u8>, gl_core::Error> {
    // Get the stream from the database
    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());
    let stream = stream_repo
        .find_by_id(stream_id)
        .await?
        .ok_or_else(|| gl_core::Error::NotFound(format!("Stream {} not found", stream_id)))?;

    // Parse the stream config to determine source type
    let config: serde_json::Value = serde_json::from_str(&stream.config)
        .map_err(|e| gl_core::Error::Config(format!("Invalid stream config JSON: {}", e)))?;

    // Determine source type from config kind field
    let kind = config
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| gl_core::Error::Config("Stream config missing 'kind' field".to_string()))?;

    let jpeg_bytes = match kind {
        "file" => {
            // File-based source
            let file_path = config
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    gl_core::Error::Config(
                        "File stream config missing 'file_path' field".to_string(),
                    )
                })?;

            use std::path::PathBuf;
            let source_path = PathBuf::from(file_path);
            let file_source = gl_capture::FileSource::new(&source_path);
            let handle = file_source.start().await?;
            handle.snapshot().await?
        }
        "website" => {
            // Website capture - try capture manager first, then trigger manual capture
            if let Ok(snapshot_bytes) = frontend_state
                .app_state
                .capture_manager
                .get_latest_snapshot(stream_id)
                .await
            {
                snapshot_bytes
            } else {
                // Trigger a manual capture for this website stream
                debug!("Attempting fallback capture for stream: {}", stream_id);
                match frontend_state
                    .app_state
                    .capture_manager
                    .take_stream_snapshot_fallback(stream_id)
                    .await
                {
                    Ok(jpeg_bytes) => {
                        debug!("✅ Fallback capture successful for stream: {}", stream_id);
                        jpeg_bytes
                    }
                    Err(e) => {
                        warn!(
                            "❌ Failed to trigger website capture for {}: {}",
                            stream_id, e
                        );
                        return Err(gl_core::Error::Config(format!(
                            "Website capture failed: {}",
                            e
                        )));
                    }
                }
            }
        }
        "rtsp" | "ffmpeg" | "yt" => {
            // For streaming sources, use the capture manager
            match frontend_state
                .app_state
                .capture_manager
                .take_stream_snapshot_fallback(stream_id)
                .await
            {
                Ok(jpeg_bytes) => jpeg_bytes,
                Err(e) => {
                    warn!(
                        "Failed to capture snapshot for {} stream {}: {}",
                        kind, stream_id, e
                    );
                    return Err(gl_core::Error::Config(format!(
                        "{} capture failed: {}",
                        kind, e
                    )));
                }
            }
        }
        _ => {
            return Err(gl_core::Error::Config(format!(
                "Unsupported stream type: {}",
                kind
            )));
        }
    };

    Ok(jpeg_bytes.to_vec())
}

/// Auth API: Check if setup is needed (Axum version)
async fn auth_setup_needed(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    match user_repo.has_any_users().await {
        Ok(has_users) => Json(serde_json::json!({
            "needs_setup": !has_users
        }))
        .into_response(),
        Err(e) => {
            warn!("Failed to check user count: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "database_error",
                    "message": "Failed to check setup status"
                })),
            )
                .into_response()
        }
    }
}

/// Auth API: First admin signup (Axum version)
async fn auth_setup_signup(
    State(frontend_state): State<FrontendState>,
    Json(payload): Json<crate::models::SignupRequest>,
) -> impl IntoResponse {
    use crate::auth::PasswordAuth;
    use gl_db::CreateUserRequest;

    debug!("First admin signup attempt for email: {}", payload.email);

    // Validate request payload
    if validator::Validate::validate(&payload).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "validation_failed",
                "message": "Invalid request data"
            })),
        )
            .into_response();
    }

    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    // Check if users already exist FIRST (race condition protection)
    match user_repo.has_any_users().await {
        Ok(true) => {
            warn!("Admin signup attempted but users already exist");
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "setup_complete",
                    "message": "Setup is already complete. Users exist in the system."
                })),
            )
                .into_response();
        }
        Ok(false) => {
            debug!("No users exist, proceeding with first admin creation");
        }
        Err(e) => {
            warn!("Failed to check user count: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "database_error",
                    "message": "Failed to check setup status"
                })),
            )
                .into_response();
        }
    }

    // Hash the password
    let password_hash = match PasswordAuth::hash_password(
        &payload.password,
        &frontend_state.app_state.security_config.argon2_params,
    ) {
        Ok(hash) => hash,
        Err(e) => {
            warn!("Failed to hash password: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "password_hash_failed",
                    "message": "Failed to secure password"
                })),
            )
                .into_response();
        }
    };

    // Create the user
    let create_request = CreateUserRequest {
        username: payload.username.clone(),
        email: payload.email.clone(),
        password_hash,
    };

    let user = match user_repo.create(create_request).await {
        Ok(user) => user,
        Err(e) => {
            // Check if this is a duplicate email error (race condition)
            if e.to_string().contains("UNIQUE constraint failed") || e.to_string().contains("email")
            {
                warn!("Admin signup failed due to duplicate email (race condition)");
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": "setup_complete",
                        "message": "Setup is already complete. An admin user already exists."
                    })),
                )
                    .into_response();
            } else {
                warn!("Failed to create first admin user: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "user_creation_failed",
                        "message": "Failed to create admin user"
                    })),
                )
                    .into_response();
            }
        }
    };

    debug!("First admin user created successfully: {}", user.id);

    // Create JWT token for immediate login
    match crate::auth::JwtAuth::create_token(
        &user.id,
        &user.email,
        &frontend_state.app_state.security_config.jwt_secret,
        &frontend_state.app_state.security_config.jwt_issuer,
    ) {
        Ok(token) => {
            debug!("JWT token created for first admin: {}", user.id);

            let response = crate::models::LoginResponse {
                access_token: token.clone(),
                token_type: "Bearer".to_string(),
                expires_in: crate::auth::JwtAuth::token_expiration_secs(),
                user: crate::models::UserInfo {
                    id: user.id,
                    username: user.username,
                    email: user.email,
                    is_active: user.is_active.unwrap_or(false),
                    is_admin: true, // First user is admin
                    created_at: user.created_at,
                },
            };

            // Set JWT token as HTTP-only cookie
            let cookie_value = format!(
                "auth_token={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
                token,
                crate::auth::JwtAuth::token_expiration_secs()
            );

            axum::response::Response::builder()
                .status(StatusCode::CREATED)
                .header(SET_COOKIE, cookie_value)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_string(&response).unwrap()))
                .unwrap()
                .into_response()
        }
        Err(e) => {
            warn!("Failed to create JWT token for first admin: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "token_creation_failed",
                    "message": "Failed to create authentication token"
                })),
            )
                .into_response()
        }
    }
}

/// Export streams API handler
async fn api_export_streams(
    authenticated_user: AuthenticatedUser,
    State(frontend_state): State<FrontendState>,
) -> impl IntoResponse {
    use tracing::{debug, error, info};

    debug!("Exporting streams for user: {}", authenticated_user.id);

    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Get all streams for the user
    match stream_repo
        .list(Some(&authenticated_user.id), 0, 1000)
        .await
    {
        Ok(streams) => {
            let exports: Vec<StreamExport> = streams
                .into_iter()
                .map(|stream| {
                    let config = serde_json::from_str(&stream.config)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    StreamExport {
                        name: stream.name,
                        description: stream.description,
                        config,
                        is_default: stream.is_default,
                    }
                })
                .collect();

            info!(
                "Exported {} streams for user {}",
                exports.len(),
                authenticated_user.id
            );
            Json(serde_json::json!({
                "streams": exports,
                "export_date": chrono::Utc::now().to_rfc3339(),
                "user_id": authenticated_user.id
            }))
            .into_response()
        }
        Err(e) => {
            error!("Failed to export streams: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to export streams",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Import streams API handler
async fn api_import_streams(
    authenticated_user: AuthenticatedUser,
    State(frontend_state): State<FrontendState>,
    Json(body): Json<StreamImportRequest>,
) -> impl IntoResponse {
    use gl_db::CreateStreamRequest;
    use tracing::{debug, error, warn};

    debug!(
        "Importing {} streams for user: {}",
        body.streams.len(),
        authenticated_user.id
    );

    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());
    let overwrite_mode = body.overwrite_mode.as_deref().unwrap_or("skip");

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for stream_export in &body.streams {
        // Check if stream with same name exists
        let existing = stream_repo
            .find_by_name_and_user(&stream_export.name, &authenticated_user.id)
            .await
            .ok()
            .flatten();

        let mut stream_name = stream_export.name.clone();

        match overwrite_mode {
            "skip" if existing.is_some() => {
                skipped += 1;
                continue;
            }
            "overwrite" if existing.is_some() => {
                // Delete existing stream first
                if let Some(existing_stream) = existing {
                    if let Err(e) = stream_repo.delete(&existing_stream.id).await {
                        errors.push(format!(
                            "Failed to delete existing stream '{}': {}",
                            stream_export.name, e
                        ));
                        continue;
                    }
                }
            }
            "create_new" if existing.is_some() => {
                // Append number to make unique name
                let mut counter = 1;
                loop {
                    let candidate = format!("{} ({})", stream_export.name, counter);
                    if stream_repo
                        .find_by_name_and_user(&candidate, &authenticated_user.id)
                        .await
                        .ok()
                        .flatten()
                        .is_none()
                    {
                        stream_name = candidate;
                        break;
                    }
                    counter += 1;
                    if counter > 100 {
                        errors.push(format!(
                            "Could not create unique name for stream '{}'",
                            stream_export.name
                        ));
                        break;
                    }
                }
            }
            _ => {} // No existing stream or mode allows creation
        }

        // Create the stream
        let create_request = CreateStreamRequest {
            name: stream_name,
            description: stream_export.description.clone(),
            config: stream_export.config.to_string(),
            user_id: authenticated_user.id.clone(),
            is_default: stream_export.is_default,
        };

        match stream_repo.create(create_request).await {
            Ok(_) => imported += 1,
            Err(e) => {
                errors.push(format!(
                    "Failed to create stream '{}': {}",
                    stream_export.name, e
                ));
            }
        }
    }

    if errors.is_empty() {
        Json(serde_json::json!({
            "success": true,
            "imported": imported,
            "skipped": skipped,
            "errors": 0
        }))
        .into_response()
    } else {
        warn!("Import completed with {} errors", errors.len());
        (
            StatusCode::PARTIAL_CONTENT,
            Json(serde_json::json!({
                "success": false,
                "imported": imported,
                "skipped": skipped,
                "errors": errors.len(),
                "error_details": errors
            })),
        )
            .into_response()
    }
}

/// Logout handler - clears auth token and redirects to login
async fn logout_handler() -> impl IntoResponse {
    use axum::http::header::SET_COOKIE;

    // Create an expired cookie to clear the auth token
    let clear_cookie = "auth_token=; Path=/; Max-Age=0; HttpOnly";

    axum::response::Response::builder()
        .status(StatusCode::FOUND)
        .header(LOCATION, "/login")
        .header(SET_COOKIE, clear_cookie)
        .body(Body::empty())
        .unwrap()
}
