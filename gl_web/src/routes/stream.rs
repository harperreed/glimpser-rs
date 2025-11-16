//! ABOUTME: Stream-related API endpoints for snapshot capture
//! ABOUTME: Handles video stream snapshot generation from streams

use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use gl_capture::{
    CaptureSource, FfmpegConfig, FfmpegSource, FileSource, HardwareAccel, JobStatus, OutputFormat,
    RtspTransport, SnapshotConfig, YtDlpConfig, YtDlpSource,
};
#[cfg(feature = "website")]
use gl_capture::{WebsiteConfig, WebsiteSource};
use gl_core::{Error, Id, Result};
use gl_db::StreamRepository;
use gl_stream::{MjpegStream, StreamConfig, StreamSession};
use hex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::{Stream as TokioStream, StreamExt};
use tracing::{debug, error, info, warn};
use utoipa::OpenApi;

use crate::{models::ErrorResponse, AppState};

/// Generate ETag from image bytes for caching
fn generate_etag(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    format!("\"{}\"", hex::encode(&hash[..8])) // Use first 8 bytes of SHA-256 for ETag
}

/// Log sampling utility to reduce noise from repetitive errors
static MJPEG_ERROR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Sample MJPEG errors - only log every Nth occurrence to reduce noise
fn should_log_mjpeg_error() -> bool {
    let count = MJPEG_ERROR_COUNTER.fetch_add(1, Ordering::Relaxed);
    count % 50 == 0 // Log every 50th error
}

/// Response for async snapshot job submission
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SnapshotJobResponse {
    /// Unique job ID for tracking the snapshot processing
    pub job_id: String,
    /// Status of the job (Pending, Processing, Completed, Failed, Cancelled)
    pub status: String,
    /// Timestamp when the job was created
    pub created_at: String,
    /// Optional message (e.g., error details)
    pub message: Option<String>,
}

/// Response for job status queries
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JobStatusResponse {
    /// Unique job ID
    pub job_id: String,
    /// Current status of the job
    pub status: String,
    /// Progress percentage (0-100), if available
    pub progress: Option<u8>,
    /// Error message if job failed
    pub error: Option<String>,
    /// Timestamp when job was created
    pub created_at: String,
    /// Timestamp when job was completed (if finished)
    pub completed_at: Option<String>,
    /// Duration in milliseconds (if completed)
    pub duration_ms: Option<u64>,
}

#[derive(OpenApi)]
#[openapi(
    paths(snapshot, snapshot_async, job_status, job_result, recent_snapshots, mjpeg_stream, start_stream, stop_stream),
    components(schemas(SnapshotJobResponse, JobStatusResponse)),
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
    req: HttpRequest,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();

    debug!(stream_id = %stream_id, "Taking snapshot");

    match take_snapshot_impl(stream_id.clone(), &state).await {
        Ok(jpeg_bytes) => {
            // Generate ETag for caching
            let etag = generate_etag(&jpeg_bytes);

            // Check if client has matching ETag (304 Not Modified)
            if let Some(if_none_match) = req.headers().get("if-none-match") {
                if let Ok(client_etag) = if_none_match.to_str() {
                    if client_etag == etag {
                        return Ok(HttpResponse::NotModified()
                            .insert_header(("etag", etag))
                            .insert_header(("cache-control", "private, max-age=60"))
                            .finish());
                    }
                }
            }

            Ok(HttpResponse::Ok()
                .content_type("image/jpeg")
                .insert_header(("etag", etag))
                .insert_header(("cache-control", "private, max-age=60"))
                .body(jpeg_bytes))
        }
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
    req: HttpRequest,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();
    debug!(stream_id = %stream_id, "Taking thumbnail");

    match take_snapshot_impl(stream_id.clone(), &state).await {
        Ok(jpeg_bytes) => {
            // Generate ETag for caching
            let etag = generate_etag(&jpeg_bytes);

            // Check if client has matching ETag (304 Not Modified)
            if let Some(if_none_match) = req.headers().get("if-none-match") {
                if let Ok(client_etag) = if_none_match.to_str() {
                    if client_etag == etag {
                        return Ok(HttpResponse::NotModified()
                            .insert_header(("etag", etag))
                            .insert_header(("cache-control", "private, max-age=60"))
                            .finish());
                    }
                }
            }

            Ok(HttpResponse::Ok()
                .content_type("image/jpeg")
                .insert_header(("etag", etag))
                .insert_header(("cache-control", "private, max-age=60"))
                .body(jpeg_bytes))
        }
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

/// Get recent snapshots for a stream (for hover animations)
#[utoipa::path(
    get,
    path = "/api/streams/{stream_id}/recent-snapshots",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 200, description = "Recent snapshots list", body = Vec<String>),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{stream_id}/recent-snapshots")]
pub async fn recent_snapshots(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();
    info!(stream_id = %stream_id, "Getting recent snapshots");

    // Get recent snapshots from database
    let snapshot_repo = gl_db::SnapshotRepository::new(state.db.pool());

    match snapshot_repo.list_by_template(&stream_id, 0, 10).await {
        Ok(snapshots) => {
            // Convert snapshots to URLs for frontend consumption
            // Since we can't serve individual snapshots by ID, we'll return the snapshot endpoint
            // and include metadata so frontend can use timestamp-based caching
            let snapshot_data: Vec<serde_json::Value> = snapshots
                .into_iter()
                .map(|snap| {
                    serde_json::json!({
                        "url": format!("/api/stream/{}/snapshot", stream_id),
                        "captured_at": snap.captured_at,
                        "id": snap.id,
                        "file_path": snap.file_path
                    })
                })
                .collect();

            Ok(HttpResponse::Ok().json(serde_json::json!({
                "snapshots": snapshot_data,
                "total": snapshot_data.len()
            })))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to get recent snapshots");
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to retrieve snapshots",
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
    // Try fast path first: get latest snapshot from running capture or database
    if let Ok(snapshot_bytes) = state.capture_manager.get_latest_snapshot(&stream_id).await {
        return Ok(snapshot_bytes.to_vec());
    }

    // Fall back to the original on-demand capture behavior
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
        "yt" | "youtube" => {
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
        "rtsp" => {
            // RTSP stream handling
            let rtsp_url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
                Error::Config("RTSP stream config missing 'url' field".to_string())
            })?;

            // Use FFmpeg to handle RTSP with optimized settings
            let mut ffmpeg_config = FfmpegConfig {
                input_url: rtsp_url.to_string(),
                rtsp_transport: RtspTransport::Tcp, // Default to TCP for reliability
                ..Default::default()
            };

            // Parse optional RTSP-specific configuration
            if let Some(transport) = config.get("transport").and_then(|v| v.as_str()) {
                ffmpeg_config.rtsp_transport = match transport.to_lowercase().as_str() {
                    "tcp" => RtspTransport::Tcp,
                    "udp" => RtspTransport::Udp,
                    _ => RtspTransport::Auto,
                };
            }

            // Parse optional timeout (RTSP streams often need longer timeouts)
            if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
                ffmpeg_config.timeout = Some(timeout_val as u32);
            } else {
                // Default timeout for RTSP
                ffmpeg_config.timeout = Some(10);
            }

            let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
            let handle = ffmpeg_source.start().await?;
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
    let stream_id_str = path.into_inner();

    // Try to subscribe to the CaptureManager's broadcast channel for real-time streaming
    if let Some(frame_receiver) = state
        .capture_manager
        .subscribe_to_stream(&stream_id_str)
        .await
    {
        debug!(stream_id = %stream_id_str, "New client connected to MJPEG stream via CaptureManager");

        // Create a simple MJPEG stream directly from the broadcast receiver
        let mjpeg_stream = create_simple_mjpeg_stream(frame_receiver);

        // Return the streaming response
        return Ok(HttpResponse::Ok()
            .content_type("multipart/x-mixed-replace; boundary=mjpeg_frame")
            .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
            .insert_header(("Pragma", "no-cache"))
            .insert_header(("Expires", "0"))
            .insert_header(("Connection", "keep-alive"))
            .streaming(mjpeg_stream));
    }

    // Fall back to the original StreamManager-based approach
    let stream_id: Id = match stream_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(HttpResponse::BadRequest()
                .json(ErrorResponse::new("invalid_id", "Invalid Stream ID")))
        }
    };

    // Get the session from the manager
    match state.stream_manager.get_session(&stream_id) {
        Some(session) => {
            debug!(stream_id = %stream_id, "New client connected to MJPEG stream via StreamManager");

            // Subscribe to the frame broadcaster
            let frame_receiver = match session.subscribe() {
                Ok(receiver) => receiver,
                Err(e) => {
                    warn!(
                        stream_id = %stream_id,
                        error = %e,
                        "Subscription rejected"
                    );
                    return Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse::new(
                        "too_many_subscribers",
                        &format!("Too many subscribers: {}", e),
                    )));
                }
            };

            // Create the real MjpegStream from the gl_stream crate
            let mjpeg_stream = MjpegStream::new(
                session.clone(),
                frame_receiver,
                state.stream_manager.metrics().clone(),
            );

            // Return the streaming response
            Ok(HttpResponse::Ok()
                .content_type(mjpeg_stream.content_type())
                .insert_header(("Cache-Control", "no-cache"))
                .streaming(mjpeg_stream))
        }
        None => {
            warn!(stream_id = %stream_id, "No active stream session found for MJPEG request");
            Ok(HttpResponse::NotFound().json(ErrorResponse::new(
                "stream_not_running",
                "Stream is not running. Please start it first.",
            )))
        }
    }
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

    // Get the stream from the database first
    let stream = {
        let repo = StreamRepository::new(state.db.pool());
        match repo.find_by_id(&stream_id).await {
            Ok(Some(stream)) => stream,
            Ok(None) => {
                return Ok(HttpResponse::NotFound()
                    .json(ErrorResponse::new("stream_not_found", "Stream not found")))
            }
            Err(e) => {
                error!(error = %e, stream_id = stream_id, "Failed to get stream from database");
                return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                    "database_error",
                    "Failed to retrieve stream",
                )));
            }
        }
    };

    // Parse stream ID to gl_core::Id
    let stream_core_id: Id = match stream_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(HttpResponse::BadRequest()
                .json(ErrorResponse::new("invalid_id", "Invalid stream ID format")))
        }
    };

    // Get or create a stream session from the manager
    let _session = match state.stream_manager.get_session(&stream_core_id) {
        Some(session) => session,
        None => {
            // Create a new capture source and handle
            let source = match create_capture_source_from_stream(&stream) {
                Ok(source) => source,
                Err(e) => {
                    error!(error = %e, stream_id = stream_id, "Failed to create capture source");
                    return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                        "config_error",
                        format!("Failed to create capture source: {}", e),
                    )));
                }
            };

            let handle = match source.start().await {
                Ok(handle) => handle,
                Err(e) => {
                    error!(error = %e, stream_id = stream_id, "Failed to start capture source");
                    return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                        "start_error",
                        format!("Failed to start capture: {}", e),
                    )));
                }
            };

            // Create a new stream session
            let new_session = Arc::new(StreamSession::new(
                stream_core_id.clone(),
                handle,
                StreamConfig::default(), // Or load from config
                state.stream_manager.metrics().clone(),
            ));

            // Start the session's frame generation loop in the background
            tokio::spawn(Arc::clone(&new_session).start());

            // Add it to the manager
            state.stream_manager.add_session(Arc::clone(&new_session));
            new_session
        }
    };

    // Start the capture in the capture manager (if not already running)
    if !state.capture_manager.is_stream_running(&stream_id).await {
        if let Err(e) = state.capture_manager.start_stream(&stream_id).await {
            // If start fails, remove the session we might have just created
            state.stream_manager.remove_session(&stream_core_id);
            error!(error = %e, stream_id = stream_id, "Failed to start stream in capture manager");
            return Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("start_error", "Failed to start stream")));
        }
    }

    info!(stream_id = %stream_id, "Stream started successfully");
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Stream started successfully",
        "stream_id": stream_id
    })))
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

    // Parse stream ID to gl_core::Id
    let stream_core_id: Id = match stream_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(HttpResponse::BadRequest()
                .json(ErrorResponse::new("invalid_id", "Invalid stream ID format")))
        }
    };

    match state.capture_manager.stop_stream(&stream_id).await {
        Ok(_) => {
            // Also remove the session from the stream manager
            state.stream_manager.remove_session(&stream_core_id);

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

/// Helper function to create a capture source from stream configuration
fn create_capture_source_from_stream(
    stream: &gl_db::Stream,
) -> Result<Box<dyn CaptureSource + Send + Sync>> {
    let config: Value = serde_json::from_str(&stream.config)
        .map_err(|e| Error::Config(format!("Invalid stream config JSON: {}", e)))?;

    let kind = config
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("Stream config missing 'kind' field".to_string()))?;

    match kind {
        "ffmpeg" => {
            let source_url = config
                .get("source_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::Config("FFmpeg stream config missing 'source_url' field".to_string())
                })?;

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

            Ok(Box::new(FfmpegSource::new(ffmpeg_config)))
        }
        "file" => {
            let file_path = config["file_path"]
                .as_str()
                .ok_or_else(|| Error::Config("Missing file_path".to_string()))?;
            Ok(Box::new(FileSource::new(file_path)))
        }
        "website" => {
            #[cfg(feature = "website")]
            {
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

                let client = gl_capture::website_source::MockWebDriverClient::new_boxed(); // Or real client
                Ok(Box::new(WebsiteSource::new(website_config, client)))
            }
            #[cfg(not(feature = "website"))]
            {
                Err(Error::Config(
                    "Website capture not enabled - compile with 'website' feature".to_string(),
                ))
            }
        }
        "yt" | "youtube" => {
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

            Ok(Box::new(YtDlpSource::new(ytdlp_config)))
        }
        _ => Err(Error::Config(format!("Unsupported stream kind: {}", kind))),
    }
}

/// Create a simple MJPEG stream from a broadcast receiver
fn create_simple_mjpeg_stream(
    receiver: broadcast::Receiver<bytes::Bytes>,
) -> impl TokioStream<Item = std::result::Result<bytes::Bytes, actix_web::Error>> {
    tokio_stream::wrappers::BroadcastStream::new(receiver).map(|result| match result {
        Ok(frame_data) => {
            // Create multipart frame with headers
            let frame_header = format!(
                "--mjpeg_frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
                frame_data.len()
            );
            // Combine header and data
            let mut combined = bytes::BytesMut::from(frame_header.as_bytes());
            combined.extend_from_slice(&frame_data);
            combined.extend_from_slice(b"\r\n");
            Ok(combined.freeze())
        }
        Err(e) => {
            if should_log_mjpeg_error() {
                warn!("MJPEG stream error (sampling 1/50): {}", e);
            }
            Err(actix_web::error::ErrorInternalServerError("Stream error"))
        }
    })
}

/// Submit a background snapshot job (async)
#[utoipa::path(
    post,
    path = "/api/stream/{stream_id}/snapshot-async",
    params(
        ("stream_id" = String, Path, description = "Stream ID")
    ),
    responses(
        (status = 202, description = "Snapshot job submitted successfully", body = SnapshotJobResponse),
        (status = 404, description = "Stream not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::post("/{stream_id}/snapshot-async")]
pub async fn snapshot_async(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let stream_id = path.into_inner();
    info!(stream_id = %stream_id, "Submitting async snapshot job");

    match submit_snapshot_job_impl(stream_id.clone(), &state).await {
        Ok(response) => Ok(HttpResponse::Accepted().json(response)),
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("stream_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, stream_id = stream_id, "Failed to submit snapshot job");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("job_submission_error", e.to_string())))
        }
    }
}

/// Get job status for a background snapshot job
#[utoipa::path(
    get,
    path = "/api/jobs/{job_id}/status",
    params(
        ("job_id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job status retrieved successfully", body = JobStatusResponse),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/jobs/{job_id}/status")]
pub async fn job_status(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let job_id = path.into_inner();
    info!(job_id = %job_id, "Getting job status");

    match state
        .background_snapshot_service
        .get_job_status(&job_id)
        .await
    {
        Ok(status) => {
            let response = JobStatusResponse {
                job_id: job_id.clone(),
                status: format!("{:?}", status),
                progress: None,             // TODO: Add progress tracking
                error: None,                // TODO: Extract error from status
                created_at: "".to_string(), // TODO: Add timestamp tracking
                completed_at: None,
                duration_ms: None,
            };
            Ok(HttpResponse::Ok().json(response))
        }
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("job_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, job_id = job_id, "Failed to get job status");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("status_retrieval_error", e.to_string())))
        }
    }
}

/// Get result of a completed background snapshot job
#[utoipa::path(
    get,
    path = "/api/jobs/{job_id}/result",
    params(
        ("job_id" = String, Path, description = "Job ID")
    ),
    responses(
        (status = 200, description = "Job result retrieved successfully", content_type = "image/jpeg"),
        (status = 202, description = "Job still processing"),
        (status = 404, description = "Job not found or failed"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/jobs/{job_id}/result")]
pub async fn job_result(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let job_id = path.into_inner();
    info!(job_id = %job_id, "Getting job result");

    // First check status
    match state
        .background_snapshot_service
        .get_job_status(&job_id)
        .await
    {
        Ok(job_option) => match job_option {
            Some(job) => match job.status {
                JobStatus::Completed => {
                    // Job is complete, get result
                    match state
                        .background_snapshot_service
                        .get_job_result(&job_id)
                        .await
                    {
                        Ok(result_bytes) => match result_bytes {
                            Some(bytes) => Ok(HttpResponse::Ok()
                                .content_type("image/jpeg")
                                .body(bytes.to_vec())),
                            None => {
                                error!(job_id = job_id, "Job completed but no result available");
                                Ok(HttpResponse::NotFound().json(ErrorResponse::new(
                                    "no_result",
                                    "Job completed but result not found",
                                )))
                            }
                        },
                        Err(e) => {
                            error!(error = %e, job_id = job_id, "Failed to get job result");
                            Ok(HttpResponse::InternalServerError()
                                .json(ErrorResponse::new("result_retrieval_error", e.to_string())))
                        }
                    }
                }
                JobStatus::Processing | JobStatus::Pending => {
                    Ok(HttpResponse::Accepted().json(serde_json::json!({
                        "message": "Job is still processing",
                        "status": format!("{:?}", job.status)
                    })))
                }
                JobStatus::Failed => Ok(HttpResponse::NotFound().json(ErrorResponse::new(
                    "job_failed",
                    "Job failed during processing",
                ))),
                JobStatus::Cancelled => Ok(HttpResponse::NotFound()
                    .json(ErrorResponse::new("job_cancelled", "Job was cancelled"))),
            },
            None => {
                warn!(job_id = job_id, "Job not found");
                Ok(HttpResponse::NotFound()
                    .json(ErrorResponse::new("job_not_found", "Job not found")))
            }
        },
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("job_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, job_id = job_id, "Failed to get job status for result");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("status_check_error", e.to_string())))
        }
    }
}

/// Implementation function for submitting async snapshot jobs
async fn submit_snapshot_job_impl(
    stream_id: String,
    state: &AppState,
) -> Result<SnapshotJobResponse> {
    // Get the stream from the database
    let stream = {
        let repo = StreamRepository::new(state.db.pool());
        repo.find_by_id(&stream_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Stream {} not found", stream_id)))?
    };

    // Create capture source from stream configuration
    let source = create_capture_source_from_stream(&stream)?;

    // Create snapshot config with reasonable defaults
    let snapshot_config = SnapshotConfig::default();

    // Submit the stream snapshot job using the new method
    let job_id = state
        .background_snapshot_service
        .submit_stream_snapshot_job(source, snapshot_config, stream_id.clone(), None)
        .await?;

    Ok(SnapshotJobResponse {
        job_id: job_id.clone(),
        status: "Pending".to_string(),
        created_at: gl_core::time::now_iso8601(),
        message: Some("Snapshot job submitted successfully".to_string()),
    })
}
