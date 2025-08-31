//! ABOUTME: Public endpoints for authenticated users (any role)
//! ABOUTME: Provides endpoints accessible to all authenticated users

use crate::{
    middleware::auth::get_http_auth_user,
    models::{ErrorResponse, StreamInfo, StreamStatus, UserInfo},
    AppState,
};
use actix_web::{get, web, HttpRequest, HttpResponse, Result};
use gl_db::{TemplateRepository, UserRepository};
use serde_json::{json, Value};
use tracing::{debug, error, warn};

/// Get current user information
#[utoipa::path(
    get,
    path = "/api/me",
    tag = "public",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Current user information", body = UserInfo),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
    )
)]
#[get("/me")]
pub async fn me(state: web::Data<AppState>, req: HttpRequest) -> Result<HttpResponse> {
    // Get authenticated user from middleware
    let auth_user = match get_http_auth_user(&req) {
        Some(user) => user,
        None => {
            warn!("Authenticated user not found in request");
            return Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                "authentication_required",
                "Authentication required",
            )));
        }
    };

    debug!("Getting user info for: {}", auth_user.id);

    let user_repo = UserRepository::new(state.db.pool());

    // Fetch fresh user data from database
    match user_repo.find_by_id(&auth_user.id).await {
        Ok(Some(user)) => {
            if !user.is_active {
                warn!("Inactive user attempted to access /me: {}", user.id);
                return Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                    "account_disabled",
                    "Account is disabled",
                )));
            }

            let user_info = UserInfo {
                id: user.id,
                username: user.username,
                email: user.email,
                role: user.role,
                is_active: user.is_active,
                created_at: user.created_at,
            };

            debug!("User info retrieved successfully for: {}", user_info.id);
            Ok(HttpResponse::Ok().json(user_info))
        }
        Ok(None) => {
            warn!("User not found in database: {}", auth_user.id);
            Ok(HttpResponse::NotFound()
                .json(ErrorResponse::new("user_not_found", "User not found")))
        }
        Err(e) => {
            warn!("Database error getting user info: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Error retrieving user information",
            )))
        }
    }
}

/// Health check endpoint
#[get("/health")]
pub async fn health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION")
    })))
}

/// Get streams endpoint - transforms templates into active streams
#[utoipa::path(
    get,
    path = "/api/streams",
    tag = "public",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "List of active streams", body = Vec<StreamInfo>),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
#[get("/streams")]
pub async fn streams(state: web::Data<AppState>) -> Result<HttpResponse> {
    debug!("Getting streams list");

    let template_repo = TemplateRepository::new(state.db.pool());

    match template_repo.list(None, 0, 1000).await {
        Ok(templates) => {
            let template_count = templates.len();
            let streams: Vec<StreamInfo> = templates
                .into_iter()
                .filter_map(|template| {
                    // Parse the config JSON string
                    let config: Value = match serde_json::from_str(&template.config) {
                        Ok(config) => config,
                        Err(e) => {
                            error!(
                                "Failed to parse template config for template '{}' (id: {}): {}. Config: {}",
                                template.name, template.id, e, template.config
                            );
                            return None;
                        }
                    };

                    // Extract source URL from template config
                    let source = match extract_source_from_template_config(&config) {
                        Some(source) => source,
                        None => {
                            error!(
                                "Failed to extract source from template '{}' (id: {}). Config: {}",
                                template.name, template.id, serde_json::to_string_pretty(&config).unwrap_or_default()
                            );
                            return None;
                        }
                    };

                    // For now, mark all templates as inactive since we don't have execution layer yet
                    // TODO: Check actual execution status when capture manager is implemented
                    let status = StreamStatus::Inactive;

                    // Extract resolution from config or use default
                    let resolution = extract_resolution_from_config(&config)
                        .unwrap_or_else(|| "1920x1080".to_string());

                    // Set FPS based on template type
                    let fps = get_fps_for_template_type(&config);

                    Some(StreamInfo {
                        id: template.id.clone(),
                        name: template.name.clone(),
                        source,
                        status,
                        resolution,
                        fps,
                        last_frame_at: None, // TODO: Get from capture history when implemented
                        template_id: Some(template.id),
                    })
                })
                .collect();

            debug!(
                "Returning {} streams from {} templates",
                streams.len(),
                template_count
            );
            Ok(HttpResponse::Ok().json(streams))
        }
        Err(e) => {
            error!("Failed to fetch templates: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to retrieve streams",
            )))
        }
    }
}

/// Extract source URL from template configuration JSON
fn extract_source_from_template_config(config: &Value) -> Option<String> {
    // Get the template kind/type
    let kind = config.get("kind").and_then(|v| v.as_str())?;

    match kind {
        "website" => config
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "rtsp" => config
            .get("rtsp_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "file" => config
            .get("file_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "yt" | "youtube" => config
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => {
            warn!("Unknown template kind: {}", kind);
            None
        }
    }
}

/// Extract resolution from template configuration JSON
fn extract_resolution_from_config(config: &Value) -> Option<String> {
    // Website templates have width and height fields
    if let Some(width) = config.get("width").and_then(|v| v.as_u64()) {
        if let Some(height) = config.get("height").and_then(|v| v.as_u64()) {
            return Some(format!("{}x{}", width, height));
        }
    }
    None
}

/// Get appropriate FPS value based on template type
fn get_fps_for_template_type(config: &Value) -> u32 {
    let kind = config
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match kind {
        "website" => 1,         // Website captures are typically 1 frame per interval
        "rtsp" => 30,           // RTSP streams are usually 30 FPS
        "file" => 24,           // Video files often 24 FPS
        "yt" | "youtube" => 30, // YouTube streams typically 30 FPS
        _ => 1,                 // Default for unknown types
    }
}

/// Get alerts endpoint (placeholder)
#[get("/alerts")]
pub async fn alerts() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!([])))
}
