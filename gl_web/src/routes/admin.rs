//! ABOUTME: Admin endpoints requiring administrator role
//! ABOUTME: Provides administrative functionality for managing templates and system

use crate::{
    middleware::auth::get_http_auth_user,
    models::{ErrorResponse, TemplateInfo},
    AppState,
};
use actix_web::{get, web, HttpRequest, HttpResponse, Result};
use gl_db::TemplateRepository;
use tracing::{debug, warn};

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
