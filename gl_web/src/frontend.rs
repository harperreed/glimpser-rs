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

/// Streams list template
#[derive(Template)]
#[template(path = "streams_ultra_simple.html")]
pub struct StreamsListTemplate {
    pub user: UserInfo,
    pub logged_in: bool,
    pub streams: Vec<StreamInfo>,
    pub filter: String,
    pub error_message: String, // Use empty string for no error
    pub has_error: bool,       // Boolean flag for template logic
}

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
#[template(path = "stream_simple.html")]
pub struct StreamDetailTemplate {
    pub stream: StreamInfo,
    pub user: UserInfo,
}

/// Individual stream card component for HTMX
#[derive(Template)]
#[template(path = "card_simple.html")]
pub struct StreamCard {
    pub stream: StreamInfo,
}

/// Admin page template - step 1
#[derive(Template)]
#[template(path = "admin_step1.html")]
pub struct AdminTemplate {
    pub user: UserInfo,
    pub logged_in: bool,
}

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

/// Create the Axum router for frontend pages
pub fn create_frontend_router() -> Router<FrontendState> {
    Router::new()
        .route("/", get(root_handler))
        .route("/login", get(login_page_handler).post(login_handler))
        .route("/dashboard", get(dashboard_handler))
        .route("/streams", get(streams_list_handler))
        .route("/streams/:id", get(stream_detail_handler))
        .route("/admin", get(admin_handler))
        // HTMX endpoints for dynamic updates
        .route("/api/htmx/streams-list", get(htmx_streams_fragment))
        .route("/api/htmx/stream-card/:id", get(htmx_stream_card_handler))
        .route(
            "/api/htmx/stream/:id/status",
            get(htmx_stream_status_fragment),
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
            let template = StreamsListTemplate {
                user,
                logged_in: true,
                streams,
                filter: String::new(),
                error_message: String::new(),
                has_error: false,
            };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    warn!("Template render error: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
                }
            }
        }
        Err(e) => {
            warn!("Failed to fetch streams: {}", e);
            let template = StreamsListTemplate {
                user,
                logged_in: true,
                streams: vec![],
                filter: String::new(),
                error_message: format!("Failed to load streams: {}", e),
                has_error: true,
            };

            match template.render() {
                Ok(html) => Html(html).into_response(),
                Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response(),
            }
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
            let template = StreamDetailTemplate { stream, user };

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

    let template = AdminTemplate {
        user,
        logged_in: true,
    };

    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            warn!("Admin template render error: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
        }
    }
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

/// Helper function to fetch admin users from database
async fn fetch_admin_users(frontend_state: &FrontendState) -> Result<Vec<AdminUser>, Error> {
    let user_repo = UserRepository::new(frontend_state.app_state.db.pool());

    // Fetch all users (in the future we can add pagination)
    let db_users = user_repo
        .list_active()
        .await
        .map_err(|e| Error::Database(format!("Failed to fetch users: {}", e)))?;

    // Convert to admin user format
    let admin_users: Vec<AdminUser> = db_users
        .into_iter()
        .map(|user| AdminUser {
            id: user.id,
            username: user.username,
            email: user.email,
            created_at: user.created_at,
        })
        .collect();

    Ok(admin_users)
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
