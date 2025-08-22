//! ABOUTME: Stream-related API endpoints for snapshot capture
//! ABOUTME: Handles video stream snapshot generation from templates

use actix_web::{web, HttpResponse, Result as ActixResult};
use gl_capture::{
    CaptureSource, FfmpegConfig, FfmpegSource, FileSource, HardwareAccel, OutputFormat,
    YtDlpConfig, YtDlpSource,
};
#[cfg(feature = "website")]
use gl_capture::{WebsiteConfig, WebsiteSource};
use gl_core::{Error, Result};
use gl_db::TemplateRepository;
use serde_json::Value;
use std::path::PathBuf;
use tracing::{error, info};
use utoipa::OpenApi;

use crate::{models::ErrorResponse, AppState};

#[derive(OpenApi)]
#[openapi(
    paths(snapshot, mjpeg_stream),
    components(schemas()),
    tags((name = "stream", description = "Stream snapshot and MJPEG streaming operations"))
)]
pub struct StreamApiDoc;

/// Take a snapshot from a stream template
#[utoipa::path(
    get,
    path = "/api/stream/{template_id}/snapshot",
    params(
        ("template_id" = String, Path, description = "Template ID")
    ),
    responses(
        (status = 200, description = "Snapshot taken successfully", content_type = "image/jpeg"),
        (status = 404, description = "Template not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{template_id}/snapshot")]
pub async fn snapshot(
    path: web::Path<String>,
    state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let template_id = path.into_inner();

    info!(template_id = %template_id, "Taking snapshot");

    match take_snapshot_impl(template_id.clone(), &state).await {
        Ok(jpeg_bytes) => Ok(HttpResponse::Ok()
            .content_type("image/jpeg")
            .body(jpeg_bytes)),
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound().json(ErrorResponse::new("template_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, template_id = template_id, "Failed to take snapshot");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("capture_error", e.to_string())))
        }
    }
}

async fn take_snapshot_impl(template_id: String, state: &AppState) -> Result<Vec<u8>> {
    // Get the template from the database
    let template = {
        let repo = TemplateRepository::new(state.db.pool());
        repo.find_by_id(&template_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Template {} not found", template_id)))?
    };

    // Parse the template config to determine source type
    let config: Value = serde_json::from_str(&template.config)
        .map_err(|e| Error::Config(format!("Invalid template config JSON: {}", e)))?;

    // Determine source type from config kind field
    let kind = config
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("Template config missing 'kind' field".to_string()))?;

    let jpeg_bytes = match kind {
        "file" => {
            // File-based source
            let file_path = config
                .get("file_path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::Config("File template config missing 'file_path' field".to_string())
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
                    Error::Config("FFmpeg template config missing 'source_url' field".to_string())
                })?;

            // Parse FFmpeg configuration from template config
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
                    Error::Config("Website template config missing 'url' field".to_string())
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

                // Use mock client for now - in production this would use real WebDriver
                let client = gl_capture::website_source::MockWebDriverClient::new_boxed();
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
            let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
                Error::Config("yt template config missing 'url' field".to_string())
            })?;

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
            return Err(Error::Config(format!(
                "Unsupported template kind: {}",
                kind
            )));
        }
    };

    Ok(jpeg_bytes.to_vec())
}

/// Stream MJPEG video from a template
#[utoipa::path(
    get,
    path = "/api/stream/{template_id}/mjpeg",
    params(
        ("template_id" = String, Path, description = "Template ID")
    ),
    responses(
        (status = 200, description = "MJPEG stream", content_type = "multipart/x-mixed-replace"),
        (status = 404, description = "Template not found"),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("jwt_auth" = []), ("api_key" = []))
)]
#[actix_web::get("/{template_id}/mjpeg")]
pub async fn mjpeg_stream(
    path: web::Path<String>,
    _state: web::Data<AppState>,
) -> ActixResult<HttpResponse> {
    let template_id = path.into_inner();

    info!(template_id = %template_id, "Starting MJPEG stream");

    // For now, return a simple error message since we need to integrate with stream manager
    // This will be improved in future iterations to use the StreamManager
    Ok(HttpResponse::NotImplemented().json(ErrorResponse::new(
        "not_implemented",
        "MJPEG streaming not yet implemented - use snapshot endpoint instead",
    )))
}
