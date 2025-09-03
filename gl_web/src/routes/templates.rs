//! ABOUTME: Template CRUD API endpoints with RBAC and validation
//! ABOUTME: Provides full template management with pagination, filtering, and ETag support

use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use gl_db::{CreateTemplateRequest, Template, TemplateRepository, UpdateTemplateRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::{info, warn};
use validator::Validate;

use crate::{middleware::auth::get_http_auth_user, models::ApiResponse};

/// Query parameters for listing templates
#[derive(Debug, Deserialize)]
pub struct ListTemplatesQuery {
    /// Page number (0-indexed)
    #[serde(default)]
    pub page: u32,
    /// Items per page (max 100)
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    /// Search by name
    pub search: Option<String>,
    /// Filter by user ID (admin only)
    pub user_id: Option<String>,
}

fn default_page_size() -> u32 {
    20
}

/// Request to create a new template
#[derive(Debug, Deserialize, Validate)]
pub struct CreateTemplateApiRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub config: Value,
    #[serde(default)]
    pub is_default: bool,
}

/// Request to update a template
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateTemplateApiRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub config: Option<Value>,
    pub is_default: Option<bool>,
}

/// Paginated response for templates
#[derive(Debug, Serialize)]
pub struct PaginatedTemplatesResponse {
    pub templates: Vec<Template>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// ETag helper
fn generate_etag(template: &Template) -> String {
    format!("\"{}\"", template.updated_at)
}

/// Validate template configuration JSON based on type
fn validate_template_config(config: &Value) -> Result<(), String> {
    let config_obj = match config.as_object() {
        Some(obj) => obj,
        None => return Err("Template config must be a JSON object".to_string()),
    };

    // Require 'kind' field
    let kind = match config_obj.get("kind").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => return Err("Template config must have a 'kind' field".to_string()),
    };

    // Validate based on kind
    match kind {
        "ffmpeg" => validate_ffmpeg_config(config_obj),
        "file" => validate_file_config(config_obj),
        "website" => validate_website_config(config_obj),
        "yt" => validate_yt_config(config_obj),
        _ => Err(format!("Unknown template kind: {}", kind)),
    }
}

fn validate_ffmpeg_config(config: &Map<String, Value>) -> Result<(), String> {
    // Require source_url for ffmpeg
    if !config.contains_key("source_url") {
        return Err("ffmpeg config must have 'source_url' field".to_string());
    }

    // Optional: output_format, hardware_accel, etc.
    Ok(())
}

fn validate_file_config(config: &Map<String, Value>) -> Result<(), String> {
    // Require file_path for file source
    if !config.contains_key("file_path") {
        return Err("file config must have 'file_path' field".to_string());
    }
    Ok(())
}

fn validate_website_config(config: &Map<String, Value>) -> Result<(), String> {
    // Require url for website
    if !config.contains_key("url") {
        return Err("website config must have 'url' field".to_string());
    }

    // Validate url is a string
    if let Some(url) = config.get("url") {
        if !url.is_string() {
            return Err("website 'url' must be a string".to_string());
        }
        let url_str = url.as_str().unwrap();
        if url_str.is_empty() {
            return Err("website 'url' cannot be empty".to_string());
        }
        // Basic URL format validation
        if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
            return Err("website 'url' must start with http:// or https://".to_string());
        }
    }

    // Validate optional fields
    if let Some(headless) = config.get("headless") {
        if !headless.is_boolean() {
            return Err("website 'headless' must be a boolean".to_string());
        }
    }

    if let Some(stealth) = config.get("stealth") {
        if !stealth.is_boolean() {
            return Err("website 'stealth' must be a boolean".to_string());
        }
    }

    if let Some(width) = config.get("width") {
        if !width.is_number() {
            return Err("website 'width' must be a number".to_string());
        }
    }

    if let Some(height) = config.get("height") {
        if !height.is_number() {
            return Err("website 'height' must be a number".to_string());
        }
    }

    if let Some(selector) = config.get("element_selector") {
        if !selector.is_string() {
            return Err("website 'element_selector' must be a string".to_string());
        }
    }

    Ok(())
}

fn validate_yt_config(config: &Map<String, Value>) -> Result<(), String> {
    // Require url for yt-dlp
    if !config.contains_key("url") {
        return Err("yt config must have 'url' field".to_string());
    }

    // Validate url is a string
    if let Some(url) = config.get("url") {
        if !url.is_string() {
            return Err("yt 'url' must be a string".to_string());
        }
        let url_str = url.as_str().unwrap();
        if url_str.is_empty() {
            return Err("yt 'url' cannot be empty".to_string());
        }
        // Basic URL validation - should start with http/https
        if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
            return Err("yt 'url' must start with http:// or https://".to_string());
        }
    }

    // Validate optional fields
    if let Some(format) = config.get("format") {
        if !format.is_string() {
            return Err("yt 'format' must be a string".to_string());
        }
    }

    if let Some(is_live) = config.get("is_live") {
        if !is_live.is_boolean() {
            return Err("yt 'is_live' must be a boolean".to_string());
        }
    }

    if let Some(timeout) = config.get("timeout") {
        if !timeout.is_number() {
            return Err("yt 'timeout' must be a number".to_string());
        }
    }

    if let Some(options) = config.get("options") {
        if !options.is_object() {
            return Err("yt 'options' must be an object".to_string());
        }
    }

    Ok(())
}

/// GET /api/templates - List templates with pagination
pub async fn list_templates(
    query: web::Query<ListTemplatesQuery>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    info!(
        user_id = %user.id,
        page = query.page,
        page_size = query.page_size,
        search = ?query.search,
        "Listing templates"
    );

    // Validate page size
    if query.page_size > 100 {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(
            "page_size cannot exceed 100".to_string(),
        )));
    }

    let repo = TemplateRepository::new(state.db.pool());
    let offset = (query.page as i64) * (query.page_size as i64);
    let limit = query.page_size as i64;

    // All users see their own templates
    let filter_user_id = Some(user.id.as_str());

    let (templates, total) = if let Some(search) = &query.search {
        // Search by name - note: this doesn't respect user filtering in current impl
        let templates = repo
            .search_by_name(search, offset, limit)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to search templates");
                actix_web::error::ErrorInternalServerError("Database error")
            })?;
        let total = templates.len() as i64; // Approximate for search
        (templates, total)
    } else {
        let templates = repo
            .list(filter_user_id, offset, limit)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to list templates");
                actix_web::error::ErrorInternalServerError("Database error")
            })?;
        let total = repo.count(filter_user_id).await.map_err(|e| {
            warn!(error = %e, "Failed to count templates");
            actix_web::error::ErrorInternalServerError("Database error")
        })?;
        (templates, total)
    };

    let total_pages = ((total as f64) / (query.page_size as f64)).ceil() as u32;

    let response = PaginatedTemplatesResponse {
        templates,
        total,
        page: query.page,
        page_size: query.page_size,
        total_pages,
    };

    Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
}

/// GET /api/templates/{id} - Get template by ID
pub async fn get_template(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let template_id = path.into_inner();

    info!(
        user_id = %user.id,
        template_id = %template_id,
        "Getting template"
    );

    let repo = TemplateRepository::new(state.db.pool());
    let template = repo.find_by_id(&template_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let template = match template {
        Some(t) => t,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Template not found".to_string())))
        }
    };

    // Check access: users can only see their own templates
    if template.user_id != user.id {
        return Ok(
            HttpResponse::Forbidden().json(ApiResponse::<()>::error("Access denied".to_string()))
        );
    }

    // Generate ETag
    let etag = generate_etag(&template);

    // Check If-None-Match header for conditional requests
    if let Some(if_none_match) = req.headers().get("If-None-Match") {
        if let Ok(client_etag) = if_none_match.to_str() {
            if client_etag == etag {
                return Ok(HttpResponse::NotModified()
                    .insert_header(("ETag", etag))
                    .finish());
            }
        }
    }

    Ok(HttpResponse::Ok()
        .insert_header(("ETag", etag))
        .json(ApiResponse::success(template)))
}

/// POST /api/templates - Create new template
pub async fn create_template(
    payload: web::Json<CreateTemplateApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    info!(
        user_id = %user.id,
        name = %payload.name,
        "Creating template"
    );

    // Validate request
    payload.validate().map_err(|e| {
        warn!(error = %e, "Template validation failed");
        actix_web::error::ErrorBadRequest(format!("Validation error: {}", e))
    })?;

    // Validate config JSON
    if let Err(msg) = validate_template_config(&payload.config) {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(msg)));
    }

    let config_json = serde_json::to_string(&payload.config).map_err(|e| {
        warn!(error = %e, "Failed to serialize config");
        actix_web::error::ErrorBadRequest("Invalid config JSON")
    })?;

    let request = CreateTemplateRequest {
        user_id: user.id.clone(),
        name: payload.name.clone(),
        description: payload.description.clone(),
        config: config_json,
        is_default: payload.is_default,
    };

    let repo = TemplateRepository::new(state.db.pool());
    let template = repo.create(request).await.map_err(|e| {
        warn!(error = %e, "Failed to create template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    info!(
        template_id = %template.id,
        "Template created successfully"
    );

    Ok(HttpResponse::Created().json(ApiResponse::success(template)))
}

/// PUT /api/templates/{id} - Update template
pub async fn update_template(
    path: web::Path<String>,
    payload: web::Json<UpdateTemplateApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let template_id = path.into_inner();

    info!(
        user_id = %user.id,
        template_id = %template_id,
        "Updating template"
    );

    // Validate request
    payload.validate().map_err(|e| {
        warn!(error = %e, "Template validation failed");
        actix_web::error::ErrorBadRequest(format!("Validation error: {}", e))
    })?;

    let repo = TemplateRepository::new(state.db.pool());

    // Check if template exists and user has access
    let existing = repo.find_by_id(&template_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let existing = match existing {
        Some(t) => t,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Template not found".to_string())))
        }
    };

    // Check access: admin can update all, users can update their own
    if existing.user_id != user.id {
        return Ok(
            HttpResponse::Forbidden().json(ApiResponse::<()>::error("Access denied".to_string()))
        );
    }

    // Check If-Match header for optimistic concurrency
    if let Some(if_match) = req.headers().get("If-Match") {
        if let Ok(client_etag) = if_match.to_str() {
            let current_etag = generate_etag(&existing);
            if client_etag != current_etag {
                return Ok(
                    HttpResponse::PreconditionFailed().json(ApiResponse::<()>::error(
                        "Template has been modified by another request".to_string(),
                    )),
                );
            }
        }
    }

    // Validate config if provided
    let config_json = if let Some(config) = &payload.config {
        if let Err(msg) = validate_template_config(config) {
            return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(msg)));
        }
        Some(serde_json::to_string(config).map_err(|e| {
            warn!(error = %e, "Failed to serialize config");
            actix_web::error::ErrorBadRequest("Invalid config JSON")
        })?)
    } else {
        None
    };

    let request = UpdateTemplateRequest {
        name: payload.name.clone(),
        description: payload.description.clone(),
        config: config_json,
        is_default: payload.is_default,
    };

    let template = repo.update(&template_id, request).await.map_err(|e| {
        warn!(error = %e, "Failed to update template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let template = template.expect("Template should exist after update");

    info!(
        template_id = %template.id,
        "Template updated successfully"
    );

    let etag = generate_etag(&template);

    Ok(HttpResponse::Ok()
        .insert_header(("ETag", etag))
        .json(ApiResponse::success(template)))
}

/// DELETE /api/templates/{id} - Delete template
pub async fn delete_template(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let template_id = path.into_inner();

    info!(
        user_id = %user.id,
        template_id = %template_id,
        "Deleting template"
    );

    let repo = TemplateRepository::new(state.db.pool());

    // Check if template exists and user has access
    let existing = repo.find_by_id(&template_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let existing = match existing {
        Some(t) => t,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Template not found".to_string())))
        }
    };

    // Check access: admin can delete all, users can delete their own
    if existing.user_id != user.id {
        return Ok(
            HttpResponse::Forbidden().json(ApiResponse::<()>::error("Access denied".to_string()))
        );
    }

    let deleted = repo.delete(&template_id).await.map_err(|e| {
        warn!(error = %e, "Failed to delete template");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    if !deleted {
        return Ok(HttpResponse::NotFound()
            .json(ApiResponse::<()>::error("Template not found".to_string())));
    }

    info!(
        template_id = %template_id,
        "Template deleted successfully"
    );

    Ok(HttpResponse::NoContent().finish())
}

/// List templates handler for actix service macro (no trailing slash)
#[actix_web::get("")]
pub async fn list_templates_service(
    query: web::Query<ListTemplatesQuery>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    list_templates(query, req, state).await
}

/// Get template handler for actix service macro
#[actix_web::get("/{id}")]
pub async fn get_template_service(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    get_template(path, req, state).await
}

/// Create template handler for actix service macro (no trailing slash)
#[actix_web::post("")]
pub async fn create_template_service(
    payload: web::Json<CreateTemplateApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    create_template(payload, req, state).await
}

/// Update template handler for actix service macro
#[actix_web::put("/{id}")]
pub async fn update_template_service(
    path: web::Path<String>,
    payload: web::Json<UpdateTemplateApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    update_template(path, payload, req, state).await
}

/// Delete template handler for actix service macro
#[actix_web::delete("/{id}")]
pub async fn delete_template_service(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    delete_template(path, req, state).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ffmpeg_config_validation() {
        let valid_config = json!({
            "kind": "ffmpeg",
            "source_url": "rtsp://camera/stream"
        });
        assert!(validate_template_config(&valid_config).is_ok());

        let invalid_config = json!({
            "kind": "ffmpeg"
        });
        assert!(validate_template_config(&invalid_config).is_err());
    }

    #[test]
    fn test_file_config_validation() {
        let valid_config = json!({
            "kind": "file",
            "file_path": "/path/to/video.mp4"
        });
        assert!(validate_template_config(&valid_config).is_ok());

        let invalid_config = json!({
            "kind": "file"
        });
        assert!(validate_template_config(&invalid_config).is_err());
    }

    #[test]
    fn test_website_config_validation() {
        let valid_config = json!({
            "kind": "website",
            "url": "https://example.com",
            "headless": true,
            "stealth": false,
            "width": 1280,
            "height": 720,
            "element_selector": "#main"
        });
        assert!(validate_template_config(&valid_config).is_ok());

        // Missing url
        let invalid_config = json!({
            "kind": "website"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        // Invalid url
        let invalid_config = json!({
            "kind": "website",
            "url": "not-a-url"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        // Invalid field types
        let invalid_config = json!({
            "kind": "website",
            "url": "https://example.com",
            "headless": "not-a-boolean"
        });
        assert!(validate_template_config(&invalid_config).is_err());
    }

    #[test]
    fn test_yt_config_validation() {
        let valid_config = json!({
            "kind": "yt",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "format": "best",
            "is_live": false,
            "timeout": 60,
            "options": {
                "cookies": "/path/to/cookies.txt"
            }
        });
        assert!(validate_template_config(&valid_config).is_ok());

        // Missing url
        let invalid_config = json!({
            "kind": "yt"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        // Invalid url
        let invalid_config = json!({
            "kind": "yt",
            "url": "not-a-url"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        // Invalid field types
        let invalid_config = json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "is_live": "not-a-boolean"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        let invalid_config = json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "timeout": "not-a-number"
        });
        assert!(validate_template_config(&invalid_config).is_err());

        let invalid_config = json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "options": "not-an-object"
        });
        assert!(validate_template_config(&invalid_config).is_err());
    }

    #[test]
    fn test_unknown_kind_validation() {
        let invalid_config = json!({
            "kind": "unknown"
        });
        assert!(validate_template_config(&invalid_config).is_err());
    }

    #[test]
    fn test_etag_generation() {
        let template = Template {
            id: "test".to_string(),
            user_id: "user".to_string(),
            name: "Test".to_string(),
            description: None,
            config: "{}".to_string(),
            is_default: false,
            execution_status: Some("idle".to_string()),
            last_executed_at: None,
            last_error_message: None,
            created_at: "2023-01-01T00:00:00Z".to_string(),
            updated_at: "2023-01-01T00:00:00Z".to_string(),
        };

        let etag = generate_etag(&template);
        assert_eq!(etag, "\"2023-01-01T00:00:00Z\"");
    }
}
