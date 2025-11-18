//! ABOUTME: Public endpoints for authenticated users (any role)
//! ABOUTME: Provides endpoints accessible to all authenticated users

use crate::{
    middleware::auth::get_http_auth_user,
    models::{ErrorResponse, StreamConfig, StreamInfo, StreamStatus, UserInfo},
    AppState,
};
use actix_web::{get, web, HttpRequest, HttpResponse, Result};
use gl_db::{StreamRepository, UserRepository};
use serde_json::json;
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
            if !user.is_active.unwrap_or(false) {
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
                is_active: user.is_active.unwrap_or(false),
                is_admin: true, // All users are admin in this system
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

/// Get streams endpoint - returns active streams
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

    let stream_repo = StreamRepository::new(state.db.pool());

    match stream_repo.list(None, 0, 1000).await {
        Ok(streams) => {
            let stream_count = streams.len();
            let mut stream_infos: Vec<StreamInfo> = Vec::new();

            for stream in streams {
                // Parse the config JSON string
                let config: StreamConfig = match serde_json::from_str(&stream.config) {
                    Ok(config) => config,
                    Err(e) => {
                        error!(
                            "Failed to parse stream config for stream '{}' (id: {}): {}. Config: {}",
                            stream.name, stream.id, e, stream.config
                        );
                        continue;
                    }
                };

                // Extract source URL from stream config
                let source = match extract_source_from_stream_config(&config) {
                    Some(source) => source,
                    None => {
                        error!(
                            "Failed to extract source from stream '{}' (id: {}). Config: {}",
                            stream.name,
                            stream.id,
                            serde_json::to_string_pretty(&config).unwrap_or_default()
                        );
                        continue;
                    }
                };

                // Check actual execution status from database and capture manager
                let status = match stream.execution_status.as_deref() {
                    Some("active") => {
                        // Double check with capture manager if it's really running
                        if state.capture_manager.is_stream_running(&stream.id).await {
                            StreamStatus::Active
                        } else {
                            StreamStatus::Inactive
                        }
                    }
                    Some("starting") => StreamStatus::Starting,
                    Some("stopping") => StreamStatus::Stopping,
                    Some("error") => StreamStatus::Error,
                    _ => StreamStatus::Inactive,
                };

                // Extract resolution from config or use default
                let resolution = extract_resolution_from_config(&config)
                    .unwrap_or_else(|| "1920x1080".to_string());

                // Set FPS based on stream type
                let fps = get_fps_for_stream_type(&config);

                // Get last frame time from capture manager if running
                let last_frame_at = if let Some(capture_info) =
                    state.capture_manager.get_capture_status(&stream.id).await
                {
                    capture_info.last_frame_at.map(|dt| dt.to_rfc3339())
                } else {
                    stream.last_executed_at.clone()
                };

                stream_infos.push(StreamInfo {
                    id: stream.id.clone(),
                    name: stream.name.clone(),
                    source,
                    status,
                    resolution,
                    fps,
                    last_frame_at,
                    template_id: Some(stream.id),
                });
            }

            debug!(
                "Returning {} streams from {} streams",
                stream_infos.len(),
                stream_count
            );
            Ok(HttpResponse::Ok().json(stream_infos))
        }
        Err(e) => {
            error!("Failed to fetch streams: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to retrieve streams",
            )))
        }
    }
}

/// Extract source URL from stream configuration JSON
fn extract_source_from_stream_config(config: &StreamConfig) -> Option<String> {
    match config {
        StreamConfig::Website(c) => Some(c.url.clone()),
        StreamConfig::Rtsp(c) => Some(c.url.clone()),
        StreamConfig::Ffmpeg(c) => Some(c.source_url.clone()),
        StreamConfig::File(c) => Some(c.file_path.clone()),
        StreamConfig::Yt(c) => Some(c.url.clone()),
    }
}

/// Extract resolution from stream configuration
fn extract_resolution_from_config(config: &StreamConfig) -> Option<String> {
    match config {
        StreamConfig::Website(c) => match (c.width, c.height) {
            (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
            _ => None,
        },
        _ => None,
    }
}

/// Get appropriate FPS value based on stream type
fn get_fps_for_stream_type(config: &StreamConfig) -> u32 {
    match config {
        StreamConfig::Website(_) => 1, // Website captures are typically 1 frame per interval
        StreamConfig::Rtsp(_) => 30,   // RTSP streams are usually 30 FPS
        StreamConfig::Ffmpeg(_) => 30, // FFmpeg streams are usually 30 FPS
        StreamConfig::File(_) => 24,   // Video files often 24 FPS
        StreamConfig::Yt(_) => 30,     // YouTube streams typically 30 FPS
    }
}

/// Get alerts endpoint (placeholder)
#[get("/alerts")]
pub async fn alerts() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!([])))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        FfmpegConfig, FileConfig, RtspConfig, StreamConfig, WebsiteConfig, YtConfig,
    };

    #[test]
    fn extract_source_from_rtsp_config() {
        let config = StreamConfig::Rtsp(RtspConfig {
            url: "rtsp://camera".into(),
        });
        assert_eq!(
            extract_source_from_stream_config(&config).as_deref(),
            Some("rtsp://camera")
        );
    }

    #[test]
    fn extract_source_from_file_config() {
        let config = StreamConfig::File(FileConfig {
            file_path: "/tmp/video.mp4".into(),
        });
        assert_eq!(
            extract_source_from_stream_config(&config).as_deref(),
            Some("/tmp/video.mp4")
        );
    }

    #[test]
    fn extract_source_from_ffmpeg_config() {
        let config = StreamConfig::Ffmpeg(FfmpegConfig {
            source_url: "rtsp://cam".into(),
        });
        assert_eq!(
            extract_source_from_stream_config(&config).as_deref(),
            Some("rtsp://cam")
        );
    }

    #[test]
    fn extract_source_from_website_config() {
        let config = StreamConfig::Website(WebsiteConfig {
            url: "https://example.com".into(),
            headless: None,
            stealth: None,
            width: Some(800),
            height: Some(600),
            element_selector: None,
        });
        assert_eq!(
            extract_source_from_stream_config(&config).as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            extract_resolution_from_config(&config).as_deref(),
            Some("800x600")
        );
        assert_eq!(get_fps_for_stream_type(&config), 1);
    }

    #[test]
    fn extract_source_from_yt_config() {
        let config = StreamConfig::Yt(YtConfig {
            url: "https://youtu.be/test".into(),
            format: None,
            is_live: None,
            timeout: None,
            options: None,
        });
        assert_eq!(
            extract_source_from_stream_config(&config).as_deref(),
            Some("https://youtu.be/test")
        );
        assert_eq!(get_fps_for_stream_type(&config), 30);
    }

    #[test]
    fn fps_for_file_config() {
        let config = StreamConfig::File(FileConfig {
            file_path: "/tmp/video.mp4".into(),
        });
        assert_eq!(get_fps_for_stream_type(&config), 24);
    }
}
