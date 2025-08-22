//! ABOUTME: Admin endpoints requiring administrator role
//! ABOUTME: Provides administrative functionality for managing templates and system

use crate::{models::TemplateInfo, AppState};
use actix_web::{get, web, HttpRequest, HttpResponse, Result};
use tracing::debug;

/// List all templates (admin only)
#[get("/templates")]
pub async fn list_templates(
    _state: web::Data<AppState>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Listing templates for admin user");

    // For now, return empty list - this is just to test the structure
    let templates: Vec<TemplateInfo> = vec![];

    debug!(
        "Templates retrieved successfully, count: {}",
        templates.len()
    );
    Ok(HttpResponse::Ok().json(templates))
}
