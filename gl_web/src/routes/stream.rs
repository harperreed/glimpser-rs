//! ABOUTME: Stream-related API endpoints for snapshot capture
//! ABOUTME: Handles video stream snapshot generation from templates

use actix_web::{web, HttpResponse, Result as ActixResult};
use gl_capture::{FileSource, CaptureSource};
use gl_core::{Error, Result};
use gl_db::TemplateRepository;
use serde_json::Value;
use std::path::PathBuf;
use tracing::{info, error};
use utoipa::OpenApi;

use crate::{AppState, models::ErrorResponse};

#[derive(OpenApi)]
#[openapi(
    paths(snapshot),
    components(schemas()),
    tags((name = "stream", description = "Stream snapshot operations"))
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
        Ok(jpeg_bytes) => {
            Ok(HttpResponse::Ok()
                .content_type("image/jpeg")
                .body(jpeg_bytes))
        }
        Err(Error::NotFound(msg)) => {
            Ok(HttpResponse::NotFound()
                .json(ErrorResponse::new("template_not_found", &msg)))
        }
        Err(e) => {
            error!(error = %e, template_id = template_id, "Failed to take snapshot");
            Ok(HttpResponse::InternalServerError()
                .json(ErrorResponse::new("capture_error", &e.to_string())))
        }
    }
}

async fn take_snapshot_impl(template_id: String, state: &AppState) -> Result<Vec<u8>> {
    // Get the template from the database
    let template = {
        let repo = TemplateRepository::new(state.db.pool());
        repo.find_by_id(&template_id).await?
            .ok_or_else(|| Error::NotFound(format!("Template {} not found", template_id)))?
    };
    
    // Parse the template config to extract source path
    // For now, we expect the config to have a "source_path" field
    let config: Value = serde_json::from_str(&template.config)
        .map_err(|e| Error::Config(format!("Invalid template config JSON: {}", e)))?;
    
    let source_path = config
        .get("source_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Config("Template config missing 'source_path' field".to_string()))?;
    
    let source_path = PathBuf::from(source_path);
    
    // Create a file source and take a snapshot
    let file_source = FileSource::new(&source_path);
    let handle = file_source.start().await?;
    let jpeg_bytes = handle.snapshot().await?;
    
    Ok(jpeg_bytes.to_vec())
}