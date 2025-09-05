//! ABOUTME: Stream-related API endpoints for snapshot capture
//! ABOUTME: Handles video stream snapshot generation from streams

use actix_web::{web, HttpResponse, Result as ActixResult};
use bytes::Bytes;
use futures_util::stream::StreamExt;
use gl_capture::{
    CaptureSource, FfmpegConfig, FfmpegSource, FileSource, HardwareAccel, OutputFormat,
    YtDlpConfig, YtDlpSource,
};
#[cfg(feature = "website")]
use gl_capture::{WebsiteConfig, WebsiteSource};
use gl_core::{Error, Result};
use gl_db::StreamRepository;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::IntervalStream;
use tracing::{error, info, warn};
use utoipa::OpenApi;

use crate::{models::ErrorResponse, AppState};

#[derive(OpenApi)]
#[openapi(
    paths(snapshot, mjpeg_stream, start_stream, stop_stream),
    components(schemas()),
    tags((name = "stream", description = "Stream snapshot, MJPEG streaming, and lifecycle operations"))
)]
pub struct StreamApiDoc;

/// Take a snapshot from a stream
#[utoipa::path(
    get,
    path = "/api/stream/{stream_id}/snapshot",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Snapshot taken successfully", content_type = "image/jpeg"),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}/snapshot")]
pub async fn snapshot(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    info!(stream_id = %stream_id, "Taking snapshot");

    match take_snapshot_impl(stream_id.clone(), &state).await {
        Ok(jpeg_bytes) => Ok(HttpResponse::Ok()
            .content_type("image/jpeg")
            .body(jpeg_bytes)),
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to take snapshot");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("capture_error", e.to_string())))
        }
    }
}

/// Get thumbnail for stream (alias for snapshot)
#[utoipa::path(
    get,
    path = "/api/streams/{stream_id}/thumbnail",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Thumbnail retrieved successfully", content_type = "image/jpeg"),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}/thumbnail")]
pub async fn thumbnail(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();
    info!(stream_id = %stream_id, "Taking thumbnail");

    match take_snapshot_impl(stream_id.clone(), &state).await {
        Ok(jpeg_bytes) => Ok(HttpResponse::Ok()
            .content_type("image/jpeg")
            .body(jpeg_bytes)),
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to take thumbnail");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("capture_error", e.to_string())))
        }
    }
}

/// Get individual stream details
#[utoipa::path(
    get,
    path = "/api/streams/{stream_id}",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Stream details", body = crate::models::StreamInfo),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}")]
pub async fn stream_details(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    // Get all streams and find the one with matching ID
    let stream_repo = gl_db::StreamRepository::new(state.db.pool());

    match stream_repo.find_by_id(&stream_id).await {
        Ok(Some(stream)) => {
            // Convert stream to StreamInfo format
            let config: serde_json::Value = match serde_json::from_str(&stream.config) {
                Ok(config) => config,
                Err(_) => {
                    return Ok(HttpResponse::InternalServerError()
                        .json(ErrorResponse::new("config_error", "Invalid stream config")))
                }
            };

            // Extract source from config
            let source = match config.get("kind").and_then(|v| v.as_str()) {
                Some("website") | Some("yt") | Some("youtube") => config
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                Some("ffmpeg") => config
                    .get("source_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                Some("file") => config
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                _ => "unknown".to_string(),
            };

            let status = match stream.execution_status.as_deref() {
                Some("active") => "active",
                Some("starting") => "starting",
                Some("stopping") => "stopping",
                Some("error") => "error",
                _ => "inactive",
            };

            let resolution = format!(
                "{}x{}",
                config.get("width").and_then(|v| v.as_u64()).unwrap_or(1920),
                config
                    .get("height")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1080)
            );

            let fps = match config.get("kind").and_then(|v| v.as_str()) {
                Some("website") => 1,
                Some("rtsp") | Some("yt") | Some("youtube") => 30,
                Some("file") => 24,
                _ => 1,
            };

            let stream_info = serde_json::json!({
                "id": stream.id,
                "name": stream.name,
                "source": source,
                "status": status,
                "resolution": resolution,
                "fps": fps,
                "stream_id": stream.id
            });

            Ok(HttpResponse::Ok().json(stream_info))
        }
        Ok(None) => Ok(HttpResponse::NotFound()
            .json(ErrorResponse::new("stream_not_found", "Stream not found"))),
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to get stream details");
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to retrieve stream",
            )))
        }
    }
}

/// Get live stream (alias for snapshot for now)
#[utoipa::path(
    get,
    path = "/api/streams/{stream_id}/live",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Live stream frame", content_type = "image/jpeg"),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}/live")]
pub async fn live_stream(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();
    info!(stream_id = %stream_id, "Getting live stream");

    match take_snapshot_impl(stream_id.clone(), &state).await {
        Ok(jpeg_bytes) => Ok(HttpResponse::Ok()
            .content_type("image/jpeg")
            .body(jpeg_bytes)),
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to get live stream");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("capture_error", e.to_string())))
        }
    }
}

async fn take_snapshot_impl(stream_id: String, state: &AppState) -> Result<Vec<u8>> {
    // Get the stream from the database
    let stream = {
        let repo = StreamRepository::new(state.db.pool());
        repo.find_by_id(&stream_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Stream {} not found", stream_id)))?
    };

    // Parse the stream config to determine source type
    let config: Value = serde_json::from_str(&stream.config)
        .map_err(|e| Error::Config(format!("Invalid stream config JSON: {}", e)))?;

    // Determine source type from config kind field
    let kind = config
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("Stream config missing 'kind' field".to_string()))?;

    let jpeg_bytes = match kind {
        "file" => {
            // File-based source
            let file_path = config
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::Config("File stream config missing 'file_path' field".to_string())
                })?;

            let source_path = PathBuf::from(file_path);
            let file_source = FileSource::new(&source_path);
            let handle = file_source.start().await?;
            handle.snapshot().await?
        }
        "ffmpeg" => {
            // FFmpeg-based source
            let source_url = config
                .get("source_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::Config("FFmpeg stream config missing 'source_url' field".to_string())
                })?;

            // Parse FFmpeg configuration from stream config
            let mut ffmpeg_config = FfmpegConfig {
                input_url: source_url.to_string(),
                ..Default::default()
            };

            // Parse hardware acceleration if specified
            if let Some(hw_accel) = config.get("hardware_accel").and_then(|v| v.as_str()) {
                ffmpeg_config.hardware_accel = match hw_accel.to_lowercase().as_str() {
                    "vaapi" => HardwareAccel::Vaapi,
                    "cuda" => HardwareAccel::Cuda,
                    "qsv" => HardwareAccel::Qsv,
                    "videotoolbox" => HardwareAccel::VideoToolbox,
                    _ => HardwareAccel::None,
                };
            }

            // Parse input options if provided
            if let Some(input_opts) = config.get("input_options").and_then(|v| v.as_object()) {
                for (key, value) in input_opts {
                    if let Some(value_str) = value.as_str() {
                        ffmpeg_config
                            .input_options
                            .insert(key.clone(), value_str.to_string());
                    }
                }
            }

            // Parse codec if specified
            if let Some(codec) = config.get("video_codec").and_then(|v| v.as_str()) {
                ffmpeg_config.video_codec = Some(codec.to_string());
            }

            // Parse timeout if specified
            if let Some(timeout) = config.get("timeout").and_then(|v| v.as_u64()) {
                ffmpeg_config.timeout = Some(timeout as u32);
            }

            // Parse quality settings
            if let Some(quality) = config.get("quality").and_then(|v| v.as_u64()) {
                ffmpeg_config.snapshot_config.quality = quality as u8;
            }

            let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
            let handle = ffmpeg_source.start().await?;
            handle.snapshot().await?
        }
        "website" => {
            #[cfg(feature = "website")]
            {
                // Website-based source
                let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
                    Error::Config("Website stream config missing 'url' field".to_string())
                })?;

                let mut website_config = WebsiteConfig {
                    url: url.to_string(),
                    ..Default::default()
                };

                // Parse optional fields from config
                if let Some(headless) = config.get("headless").and_then(|v| v.as_bool()) {
                    website_config.headless = headless;
                }

                if let Some(stealth) = config.get("stealth").and_then(|v| v.as_bool()) {
                    website_config.stealth = stealth;
                }

                if let Some(width) = config.get("width").and_then(|v| v.as_u64()) {
                    website_config.width = width as u32;
                }

                if let Some(height) = config.get("height").and_then(|v| v.as_u64()) {
                    website_config.height = height as u32;
                }

                if let Some(selector) = config.get("element_selector").and_then(|v| v.as_str()) {
                    website_config.element_selector = Some(selector.to_string());
                }

                if let Some(username) = config.get("basic_auth_username").and_then(|v| v.as_str()) {
                    website_config.basic_auth_username = Some(username.to_string());
                }

                if let Some(password) = config.get("basic_auth_password").and_then(|v| v.as_str()) {
                    website_config.basic_auth_password = Some(password.to_string());
                }

                // Try to use real WebDriver first, fallback to mock if not available
                #[cfg(feature = "website")]
                let client = {
                    match gl_capture::website_source::ThirtyfourClient::new(None).await {
                        Ok(real_client) => {
                            info!("Using real ThirtyfourClient for stream snapshot");
                            Box::new(real_client)
                                as Box<dyn gl_capture::website_source::WebDriverClient>
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to create real WebDriver client, falling back to mock");
                            gl_capture::website_source::MockWebDriverClient::new_boxed()
                        }
                    }
                };

                #[cfg(not(feature = "website"))]
                let client = {
                    warn!("Website feature not enabled, using mock WebDriver client");
                    gl_capture::website_source::MockWebDriverClient::new_boxed()
                };

                let website_source = WebsiteSource::new(website_config, client);
                let handle = website_source.start().await?;
                handle.snapshot().await?
            }
            #[cfg(not(feature = "website"))]
            {
                return Err(Error::Config(
                    "Website capture not enabled - compile with 'website' feature".to_string(),
                ));
            }
        }
        "yt" => {
            // yt-dlp-based source
            let url = config
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Config("yt stream config missing 'url' field".to_string()))?;

            let mut ytdlp_config = YtDlpConfig {
                url: url.to_string(),
                ..Default::default()
            };

            // Parse optional fields from config
            if let Some(format) = config.get("format").and_then(|v| v.as_str()) {
                ytdlp_config.format = match format {
                    "best" => OutputFormat::Best,
                    "worst" => OutputFormat::Worst,
                    format_id if format_id.chars().all(|c| c.is_numeric() || c == '+') => {
                        OutputFormat::FormatId(format_id.to_string())
                    }
                    height_str
                        if height_str.starts_with("best[height<=") && height_str.ends_with("]") =>
                    {
                        let height_part = &height_str[13..height_str.len() - 1];
                        if let Ok(height) = height_part.parse::<u32>() {
                            OutputFormat::BestWithHeight(height)
                        } else {
                            OutputFormat::Best
                        }
                    }
                    _ => OutputFormat::Best,
                };
            }

            if let Some(is_live) = config.get("is_live").and_then(|v| v.as_bool()) {
                ytdlp_config.is_live = is_live;
            }

            if let Some(timeout) = config.get("timeout").and_then(|v| v.as_u64()) {
                ytdlp_config.timeout = Some(timeout as u32);
            }

            // Parse options if provided
            if let Some(opts) = config.get("options").and_then(|v| v.as_object()) {
                for (key, value) in opts {
                    if let Some(value_str) = value.as_str() {
                        ytdlp_config
                            .options
                            .insert(key.clone(), value_str.to_string());
                    }
                }
            }

            let ytdlp_source = YtDlpSource::new(ytdlp_config);
            let handle = ytdlp_source.start().await?;
            handle.snapshot().await?
        }
        _ => {
            return Err(Error::Config(format!("Unsupported stream kind: {}", kind)));
        }
    };

    Ok(jpeg_bytes.to_vec())
}

/// Stream MJPEG video from a stream
#[utoipa::path(
    get,
    path = "/api/stream/{stream_id}/mjpeg",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "MJPEG stream", content_type = "multipart/x-mixed-replace"),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}/mjpeg")]
pub async fn mjpeg_stream(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    info!(stream_id = %stream_id, "Starting MJPEG stream");

    // Check if the stream is running in capture manager
    if !state.capture_manager.is_stream_running(&stream_id).await {
        info!(stream_id = %stream_id, "Stream not running, attempting to start");

        // Try to start the stream if it's not running
        match state.capture_manager.start_stream(&stream_id).await {
            Ok(_) => {
                info!(stream_id = %stream_id, "Stream started successfully for streaming");
                // Give it a moment to start up
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => {
                error!(stream_id = %stream_id, error = %e, "Failed to start stream for streaming");
                return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                    "stream_start_failed",
                    format!("Failed to start stream: {}", e),
                )));
            }
        }
    }

    // Create MJPEG streaming response
    let boundary = "--GLIMPSER_MJPEG_BOUNDARY";
    let content_type = format!("multipart/x-mixed-replace; boundary={}", boundary);

    let stream = create_mjpeg_stream(
        state.capture_manager.clone(),
        stream_id.clone(),
        boundary.to_string(),
    );

    info!(stream_id = %stream_id, "MJPEG stream started successfully");
    Ok(HttpResponse::Ok()
        .content_type(content_type)
        .streaming(stream))
}

/// Start a stream from a stream
#[utoipa::path(
    post,
    path = "/api/stream/{stream_id}/start",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Stream started successfully"),
        (status = 400, description = "Stream already running"),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::post("/{stream_id}/start")]
pub async fn start_stream(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    info!(stream_id = %stream_id, "Starting stream");

    match state.capture_manager.start_stream(&stream_id).await {
        Ok(_) => {
            info!(stream_id = %stream_id, "Stream started successfully");
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "message": "Stream started successfully",
                "stream_id": stream_id
            })))
        }
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_found", &msg)))
        }
        Err(Error::Config(msg)) if msg.contains("already running") => {
            Ok(HttpResponse::BadRequest().json(ErrorResponse::new("stream_already_running", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to start stream");
            // Don't expose internal error details to API consumers
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "start_error",
                "Internal server error occurred while starting stream",
            )))
        }
    }
}

/// Stop a running stream
#[utoipa::path(
    post,
    path = "/api/stream/{stream_id}/stop",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Stream stopped successfully"),
        (status = 404, description = "Stream not found or stream not running"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::post("/{stream_id}/stop")]
pub async fn stop_stream(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    info!(stream_id = %stream_id, "Stopping stream");

    match state.capture_manager.stop_stream(&stream_id).await {
        Ok(_) => {
            info!(stream_id = %stream_id, "Stream stopped successfully");
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "message": "Stream stopped successfully",
                "stream_id": stream_id
            })))
        }
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_running", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to stop stream");
            // Don't expose internal error details to API consumers
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "stop_error",
                "Internal server error occurred while stopping stream",
            )))
        }
    }
}

/// Create an MJPEG stream from a capture manager
fn create_mjpeg_stream(
    capture_manager: Arc<crate::capture_manager::CaptureManager>,
    stream_id: String,
    boundary: String,
) -> impl futures_util::Stream<Item = std::result::Result<Bytes, Box<dyn std::error::Error>>> {
    // Create interval stream for periodic snapshot capture
    let interval = tokio::time::interval(Duration::from_millis(200)); // 5 FPS
    let stream = IntervalStream::new(interval);

    stream.then(move |_| {
        let capture_manager = capture_manager.clone();
        let stream_id = stream_id.clone();
        let boundary = boundary.clone();

        async move {
            // Get capture status and take snapshot if running
            if let Some(capture_info) = capture_manager.get_capture_status(&stream_id).await {
                if capture_info.status == crate::capture_manager::CaptureStatus::Active {
                    // Try to get snapshot from one of the running captures
                    // This is a simplified approach - in production you'd want more robust snapshot handling
                    match take_stream_snapshot(&stream_id, &capture_manager).await {
                        Ok(snapshot_data) => {
                            let frame_data = format!(
                                "\r\n--{}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                                boundary,
                                snapshot_data.len()
                            );

                            let mut frame_bytes = Vec::new();
                            frame_bytes.extend_from_slice(frame_data.as_bytes());
                            frame_bytes.extend_from_slice(&snapshot_data);
                            frame_bytes.extend_from_slice(b"\r\n");

                            Ok(Bytes::from(frame_bytes))
                        }
                        Err(e) => {
                            error!(stream_id = %stream_id, error = %e, "Failed to capture snapshot for MJPEG stream");
                            // Send a simple error frame
                            let error_frame = format!(
                                "\r\n--{}\r\nContent-Type: text/plain\r\n\r\nError: Failed to capture frame\r\n",
                                boundary
                            );
                            Ok(Bytes::from(error_frame))
                        }
                    }
                } else {
                    // Stream not active, send end boundary
                    Err(Box::new(std::io::Error::other("Stream ended")) as Box<dyn std::error::Error>)
                }
            } else {
                // Stream not running, send end boundary
                Err(Box::new(std::io::Error::other("Stream not running")) as Box<dyn std::error::Error>)
            }
        }
    })
}

/// Helper function to take a snapshot from a stream using the CaptureManager
async fn take_stream_snapshot(
    stream_id: &str,
    capture_manager: &crate::capture_manager::CaptureManager,
) -> Result<Vec<u8>> {
    // Use the CaptureManager's new take_stream_snapshot method
    match capture_manager.take_stream_snapshot(stream_id).await {
        Ok(snapshot_bytes) => Ok(snapshot_bytes.to_vec()),
        Err(e) => {
            error!(stream_id = %stream_id, error = %e, "Failed to take stream snapshot");
            Err(e)
        }
    }
}
