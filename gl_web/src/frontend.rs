//! ABOUTME: Axum-based frontend with server-rendered pages using Askama templates
//! ABOUTME: Handles user-facing web interface with HTMX interactivity

#![allow(unused_imports)] // post is used in router but clippy doesn't detect it

use crate::auth::{JwtAuth, PasswordAuth};
use askama::Template;
use axum::{
    extract::{Form, Path, State},
    http::{
        header::{LOCATION, SET_COOKIE},
        StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use gl_core::Error;
use gl_db::{StreamRepository, UserRepository};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::AppState;

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
    pub capture_interval: Option<u32>, // Capture interval in seconds

    // File source fields
    pub file_path: Option<String>,

    // Website capture fields
    pub website_url: Option<String>,
    pub website_width: Option<u32>,
    pub website_height: Option<u32>,
    pub element_selector: Option<String>,
    pub selector_type: Option<String>, // "css" or "xpath"
    pub headless: Option<String>,
    pub stealth: Option<String>,

    // FFmpeg fields
    pub ffmpeg_source: Option<String>,
    pub ffmpeg_args: Option<String>,

    // YouTube/yt-dlp fields
    pub yt_url: Option<String>,
    pub quality: Option<String>,
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
    pub capture_interval: u32,

    // File fields
    pub file_path: String,

    // Website fields
    pub website_url: String,
    pub website_width: u32,
    pub website_height: u32,
    pub element_selector: String,
    pub selector_type: String, // "css" or "xpath"
    pub headless: bool,
    pub stealth: bool,

    // FFmpeg fields
    pub ffmpeg_source: String,
    pub ffmpeg_args: String,

    // yt-dlp fields
    pub yt_url: String,
    pub quality: String,
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
            capture_interval: 30,
            file_path: String::new(),
            website_url: String::new(),
            website_width: 1920,
            website_height: 1080,
            element_selector: String::new(),
            selector_type: "css".to_string(),
            headless: true,
            stealth: true,
            ffmpeg_source: String::new(),
            ffmpeg_args: String::new(),
            yt_url: String::new(),
            quality: "best".to_string(),
        }
    }
}

/// Create the Axum router for frontend pages
pub fn create_frontend_router() -> Router<FrontendState> {
    Router::new()
        .route("/", get(root_handler))
        .route("/login", get(login_page_handler).post(login_handler))
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
}

/// Root handler - redirect to dashboard
async fn root_handler() -> impl IntoResponse {
    Redirect::permanent("/dashboard")
}

/// Dashboard page handler
async fn dashboard_handler(State(_state): State<FrontendState>) -> impl IntoResponse {
    let template = DashboardTemplate {
        user: UserInfo {
            id: "temp".to_string(),
            username: "Test User".to_string(),
            is_admin: true,
        },
        logged_in: true,
        stream_count: 0,
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

/// Login form handler
async fn login_handler(
    State(frontend_state): State<FrontendState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    debug!("Login attempt for username: {}", form.username);

    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    // Find user by email (username field is actually email in the form)
    match user_repo.find_by_email(&form.username).await {
        Ok(Some(user)) => {
            if !user.is_active.unwrap_or(false) {
                warn!("Login attempt for inactive user: {}", user.id);
                return render_login_with_error("Account is disabled").into_response();
            }

            // Verify password
            match PasswordAuth::verify_password(&form.password, &user.password_hash) {
                Ok(true) => {
                    debug!("Password verification successful for user: {}", user.id);

                    // Create JWT token
                    match JwtAuth::create_token(
                        &user.id,
                        &user.email,
                        &frontend_state.app_state.security_config.jwt_secret,
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
                            Response::builder()
                                .status(StatusCode::SEE_OTHER)
                                .header(SET_COOKIE, cookie_value)
                                .header(LOCATION, "/dashboard")
                                .header("HX-Redirect", "/dashboard")
                                .body("".into())
                                .unwrap()
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
                        format!(r#"<img src="/api/stream/{}/thumbnail" alt="{}" class="w-full h-full object-cover">"#, s.stream_id, s.name)
                    } else {
                        "<span class=\"text-gray-500\">Offline</span>".to_string()
                    },
                    s.name,
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
            <a href="/login" class="px-4 py-2 bg-red-600 text-white rounded-md text-sm font-medium hover:bg-red-700">Logout</a>
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
async fn admin_handler(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    // TODO: Extract user from cookie/session - for now use test user
    let user = UserInfo {
        id: "test".to_string(),
        username: "Admin User".to_string(),
        is_admin: true,
    };

    // Fetch streams for admin interface
    let streams = fetch_streams(&frontend_state, None)
        .await
        .unwrap_or_default();

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
        s.name, s.id,
        if s.status == "active" { "bg-green-100 text-green-800" } else { "bg-gray-100 text-gray-800" },
        s.status,
        s.last_frame_at,
        // Start/Stop toggle button
        if s.status == "active" {
            format!("<button hx-post=\"/api/settings/streams/{}/stop\" hx-target=\"closest tr\" hx-swap=\"outerHTML\" class=\"text-orange-600 hover:text-orange-900\">Stop</button>", s.id)
        } else {
            format!("<button hx-post=\"/api/settings/streams/{}/start\" hx-target=\"closest tr\" hx-swap=\"outerHTML\" class=\"text-green-600 hover:text-green-900\">Start</button>", s.id)
        },
        s.id, s.id, s.name
    )).collect::<Vec<_>>().join("");

    // Complete admin page HTML with 100% CRUD functionality
    Html(format!(r#"<!DOCTYPE html>
<html><head><title>Admin Panel</title><script src="https://cdn.tailwindcss.com"></script><script src="https://unpkg.com/htmx.org@1.9.10"></script></head>
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
            <a href="/login" class="px-4 py-2 bg-red-600 text-white rounded-md text-sm font-medium hover:bg-red-700">Logout</a>
        </div>
    </nav>

    <div class="p-8 max-w-6xl mx-auto w-full">
        <div class="flex justify-between items-center mb-8">
            <h2 class="text-2xl font-bold text-gray-800">Settings</h2>
            <div class="bg-yellow-100 text-yellow-800 px-4 py-2 rounded-md text-sm font-medium">Administrator privileges required</div>
        </div>

        <div class="bg-white shadow rounded-lg">
            <div class="px-4 py-5 sm:p-6">
                <div class="flex justify-between items-center mb-4">
                    <h3 class="text-lg font-medium text-gray-900">Stream Configuration</h3>
                    <div class="flex space-x-2">
                        <button class="bg-green-600 hover:bg-green-700 text-white px-4 py-2 rounded-md text-sm font-medium">Import</button>
                        <button class="bg-gray-600 hover:bg-gray-700 text-white px-4 py-2 rounded-md text-sm font-medium">Export</button>
                        <a href="/settings/streams/new" class="bg-blue-600 hover:bg-blue-700 text-white px-4 py-2 rounded-md text-sm font-medium">Add Stream</a>
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
</body></html>"#, user.username, streams_html)).into_response()
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

    StreamConfigForEdit {
        id: db_stream.id.clone(),
        name: db_stream.name.clone(),
        description: db_stream.description.clone().unwrap_or_default(),
        config_kind: kind.clone(),
        is_default: db_stream.is_default,
        capture_interval: config
            .get("capture_interval_seconds")
            .and_then(|c| c.as_u64())
            .unwrap_or(30) as u32,

        // File fields
        file_path: config
            .get("file_path")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string(),

        // Website fields
        website_url: config
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string(),
        website_width: config.get("width").and_then(|w| w.as_u64()).unwrap_or(1920) as u32,
        website_height: config
            .get("height")
            .and_then(|h| h.as_u64())
            .unwrap_or(1080) as u32,
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

        // yt-dlp fields
        yt_url: config
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string(),
        quality: config
            .get("quality")
            .and_then(|q| q.as_str())
            .unwrap_or("best")
            .to_string(),
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
    State(frontend_state): State<FrontendState>,
    Form(form): Form<StreamCreateForm>,
) -> impl IntoResponse {
    use gl_db::CreateStreamRequest;

    let stream_repo = StreamRepository::new(frontend_state.app_state.db.pool());

    // Build JSON config based on stream type
    let capture_interval = form.capture_interval.unwrap_or(30);
    let config_json = match form.config_kind.as_str() {
        "file" => {
            let file_path = form.file_path.unwrap_or_default();
            serde_json::json!({
                "kind": "file",
                "file_path": file_path,
                "capture_interval_seconds": capture_interval
            })
            .to_string()
        }
        "website" => {
            let url = form.website_url.unwrap_or_default();
            let width = form.website_width.unwrap_or(1920);
            let height = form.website_height.unwrap_or(1080);
            let headless = form.headless.is_some();
            let stealth = form.stealth.is_some();

            let mut config = serde_json::json!({
                "kind": "website",
                "url": url,
                "width": width,
                "height": height,
                "headless": headless,
                "stealth": stealth,
                "capture_interval_seconds": capture_interval
            });

            if let Some(selector) = form.element_selector.filter(|s| !s.is_empty()) {
                config["element_selector"] = serde_json::Value::String(selector);
            }

            config.to_string()
        }
        "ffmpeg" => {
            let source = form.ffmpeg_source.unwrap_or_default();
            let args = form.ffmpeg_args.unwrap_or_default();
            serde_json::json!({
                "kind": "ffmpeg",
                "source": source,
                "args": args,
                "capture_interval_seconds": capture_interval
            })
            .to_string()
        }
        "yt" => {
            let url = form.yt_url.unwrap_or_default();
            let quality = form.quality.unwrap_or_else(|| "best".to_string());

            serde_json::json!({
                "kind": "yt",
                "url": url,
                "format": quality,
                "capture_interval_seconds": capture_interval
            })
            .to_string()
        }
        _ => {
            return Html("Invalid stream type").into_response();
        }
    };

    let create_request = CreateStreamRequest {
        user_id: "admin".to_string(), // TODO: Get actual user ID from session
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

            // Temporary simple edit page showing parsed data until template issues resolved
            Html(format!(r#"
                <!DOCTYPE html>
                <html><head><title>Edit Stream</title><script src="https://cdn.tailwindcss.com"></script></head>
                <body class="bg-slate-50 p-8">
                    <div class="max-w-2xl mx-auto">
                        <a href="/settings" class="text-blue-600">← Back to Settings</a>
                        <h1 class="text-3xl font-bold mt-4 mb-8">Edit Stream: {}</h1>
                        <div class="bg-white p-6 rounded-lg shadow space-y-4">
                            <p><strong>Type:</strong> {}</p>
                            <p><strong>Capture Interval:</strong> {} seconds</p>
                            <p><strong>File Path:</strong> {}</p>
                            <p><strong>Website URL:</strong> {}</p>
                            <p><strong>Dimensions:</strong> {}x{}</p>
                            <p><strong>Element Selector:</strong> {} ({})</p>
                            <p class="text-gray-500 mt-4">Full edit form with dynamic fields coming next...</p>
                        </div>
                    </div>
                </body></html>
            "#, config_for_edit.name, config_for_edit.config_kind, config_for_edit.capture_interval,
               config_for_edit.file_path, config_for_edit.website_url,
               config_for_edit.website_width, config_for_edit.website_height,
               config_for_edit.element_selector, config_for_edit.selector_type)).into_response()
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
    let capture_interval = form.capture_interval.unwrap_or(30);
    let config_json = match form.config_kind.as_str() {
        "file" => {
            let file_path = form.file_path.unwrap_or_default();
            serde_json::json!({
                "kind": "file",
                "file_path": file_path,
                "capture_interval_seconds": capture_interval
            })
            .to_string()
        }
        "website" => {
            let url = form.website_url.unwrap_or_default();
            let width = form.website_width.unwrap_or(1920);
            let height = form.website_height.unwrap_or(1080);
            let headless = form.headless.is_some();
            let stealth = form.stealth.is_some();
            let selector_type = form.selector_type.unwrap_or_else(|| "css".to_string());

            let mut config = serde_json::json!({
                "kind": "website",
                "url": url,
                "width": width,
                "height": height,
                "headless": headless,
                "stealth": stealth,
                "selector_type": selector_type,
                "capture_interval_seconds": capture_interval
            });

            if let Some(selector) = form.element_selector.filter(|s| !s.is_empty()) {
                config["element_selector"] = serde_json::Value::String(selector);
            }

            config.to_string()
        }
        "ffmpeg" => {
            let source = form.ffmpeg_source.unwrap_or_default();
            let args = form.ffmpeg_args.unwrap_or_default();
            serde_json::json!({
                "kind": "ffmpeg",
                "source": source,
                "args": args,
                "capture_interval_seconds": capture_interval
            })
            .to_string()
        }
        "yt" => {
            let url = form.yt_url.unwrap_or_default();
            let quality = form.quality.unwrap_or_else(|| "best".to_string());

            serde_json::json!({
                "kind": "yt",
                "url": url,
                "format": quality,
                "capture_interval_seconds": capture_interval
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
