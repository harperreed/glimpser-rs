//! ABOUTME: Stream CRUD API endpoints with RBAC and validation
//! ABOUTME: Provides full stream management with pagination, filtering, and ETag support

use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use gl_db::{CachedStreamRepository, CreateStreamRequest, Stream, UpdateStreamRequest};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use validator::Validate;

use crate::{
    middleware::auth::get_http_auth_user,
    models::{ApiResponse, StreamConfig},
};

/// Query parameters for listing streams
#[derive(Debug, Deserialize)]
pub struct ListStreamsQuery {
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

/// Request to create a new stream
#[derive(Debug, Deserialize, Validate)]
pub struct CreateStreamApiRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub config: StreamConfig,
    #[serde(default)]
    pub is_default: bool,
}

/// Request to update a stream
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateStreamApiRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: Option<String>,
    #[validate(length(max = 500))]
    pub description: Option<String>,
    pub config: Option<StreamConfig>,
    pub is_default: Option<bool>,
}

/// Paginated response for streams
#[derive(Debug, Serialize)]
pub struct PaginatedStreamsResponse {
    pub streams: Vec<Stream>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// ETag helper
fn generate_etag(stream: &Stream) -> String {
    format!("\"{}\"", stream.updated_at)
}

/// Validate stream configuration
fn validate_stream_config(config: &StreamConfig) -> Result<(), String> {
    match config {
        StreamConfig::Rtsp(c) => c.validate(),
        StreamConfig::Ffmpeg(c) => c.validate(),
        StreamConfig::File(c) => c.validate(),
        StreamConfig::Website(c) => c.validate(),
        StreamConfig::Yt(c) => c.validate(),
    }
    .map_err(|e| e.to_string())
}

/// GET /api/streams - List streams with pagination
pub async fn list_streams(
    query: web::Query<ListStreamsQuery>,
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
        "Listing streams"
    );

    // Validate page size
    if query.page_size > 100 {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(
            "page_size cannot exceed 100".to_string(),
        )));
    }

    let repo = CachedStreamRepository::new(state.db.pool(), state.cache.clone());
    let offset = (query.page as i64) * (query.page_size as i64);
    let limit = query.page_size as i64;

    // All users see their own streams
    let filter_user_id = Some(user.id.as_str());

    // Use optimized compound queries to eliminate N+1 pattern
    let (streams, total) = if let Some(search) = &query.search {
        // Search by name with proper user filtering and count
        repo.search_with_total(filter_user_id, search, offset, limit)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to search streams with total");
                actix_web::error::ErrorInternalServerError("Database error")
            })?
    } else {
        // List streams with total count in single query
        repo.list_with_total(filter_user_id, offset, limit)
            .await
            .map_err(|e| {
                warn!(error = %e, "Failed to list streams with total");
                actix_web::error::ErrorInternalServerError("Database error")
            })?
    };

    let total_pages = ((total as f64) / (query.page_size as f64)).ceil() as u32;

    let response = PaginatedStreamsResponse {
        streams,
        total,
        page: query.page,
        page_size: query.page_size,
        total_pages,
    };

    Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
}

/// GET /api/streams/{id} - Get stream by ID
pub async fn get_stream(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let stream_id = path.into_inner();

    info!(
        user_id = %user.id,
        stream_id = %stream_id,
        "Getting stream"
    );

    let repo = CachedStreamRepository::new(state.db.pool(), state.cache.clone());
    let stream = repo.find_by_id(&stream_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find stream");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let stream = match stream {
        Some(s) => s,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Stream not found".to_string())))
        }
    };

    // Check access: users can only see their own streams
    if stream.user_id != user.id {
        return Ok(
            HttpResponse::Forbidden().json(ApiResponse::<()>::error("Access denied".to_string()))
        );
    }

    // Generate ETag
    let etag = generate_etag(&stream);

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
        .json(ApiResponse::success(stream)))
}

/// POST /api/streams - Create new stream
pub async fn create_stream(
    payload: web::Json<CreateStreamApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    info!(
        user_id = %user.id,
        name = %payload.name,
        "Creating stream"
    );

    // Validate request
    payload.validate().map_err(|e| {
        warn!(error = %e, "Stream validation failed");
        actix_web::error::ErrorBadRequest(format!("Validation error: {}", e))
    })?;

    // Validate config JSON
    if let Err(msg) = validate_stream_config(&payload.config) {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(msg)));
    }

    let config_json = serde_json::to_string(&payload.config).map_err(|e| {
        warn!(error = %e, "Failed to serialize config");
        actix_web::error::ErrorBadRequest("Invalid config JSON")
    })?;

    // Verify the user exists in the database before creating stream
    let user_repo = gl_db::UserRepository::new(state.db.pool());
    match user_repo.find_by_id(&user.id).await {
        Ok(Some(_)) => {
            info!(user_id = %user.id, "User exists, proceeding with stream creation");
        }
        Ok(None) => {
            warn!(
                user_id = %user.id,
                "User from JWT token not found in database - token may be stale"
            );
            return Ok(HttpResponse::Unauthorized().json(ApiResponse::<()>::error(
                "Authentication token is invalid or stale. Please log in again.".to_string(),
            )));
        }
        Err(e) => {
            warn!(error = %e, user_id = %user.id, "Failed to verify user exists");
            return Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<()>::error(
                    "Database error while verifying user".to_string(),
                )),
            );
        }
    }

    let request = CreateStreamRequest {
        user_id: user.id.clone(),
        name: payload.name.clone(),
        description: payload.description.clone(),
        config: config_json,
        is_default: payload.is_default,
    };

    let repo = CachedStreamRepository::new(state.db.pool(), state.cache.clone());
    let stream = repo.create(request).await.map_err(|e| {
        let error_msg = e.to_string();
        warn!(error = %e, user_id = %user.id, "Failed to create stream");

        // Check if this is a foreign key constraint error
        if error_msg.contains("FOREIGN KEY constraint failed") {
            warn!(
                user_id = %user.id,
                "Foreign key constraint failed - user may not exist in database"
            );
            return actix_web::error::ErrorBadRequest(
                "Failed to create stream: Database error: Failed to create stream: error returned from database: (code: 787) FOREIGN KEY constraint failed".to_string()
            );
        }

        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    info!(
        stream_id = %stream.id,
        "Stream created successfully"
    );

    Ok(HttpResponse::Created().json(ApiResponse::success(stream)))
}

/// PUT /api/streams/{id} - Update stream
pub async fn update_stream(
    path: web::Path<String>,
    payload: web::Json<UpdateStreamApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let stream_id = path.into_inner();

    info!(
        user_id = %user.id,
        stream_id = %stream_id,
        "Updating stream"
    );

    // Validate request
    payload.validate().map_err(|e| {
        warn!(error = %e, "Stream validation failed");
        actix_web::error::ErrorBadRequest(format!("Validation error: {}", e))
    })?;

    let repo = CachedStreamRepository::new(state.db.pool(), state.cache.clone());

    // Check if stream exists and user has access
    let existing = repo.find_by_id(&stream_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find stream");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let existing = match existing {
        Some(s) => s,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Stream not found".to_string())))
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
                        "Stream has been modified by another request".to_string(),
                    )),
                );
            }
        }
    }

    // Validate config if provided
    let config_json = if let Some(config) = &payload.config {
        if let Err(msg) = validate_stream_config(config) {
            return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(msg)));
        }
        Some(serde_json::to_string(config).map_err(|e| {
            warn!(error = %e, "Failed to serialize config");
            actix_web::error::ErrorBadRequest("Invalid config JSON")
        })?)
    } else {
        None
    };

    let request = UpdateStreamRequest {
        name: payload.name.clone(),
        description: payload.description.clone(),
        config: config_json,
        is_default: payload.is_default,
    };

    let stream = repo.update(&stream_id, request).await.map_err(|e| {
        warn!(error = %e, "Failed to update stream");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let stream = stream.expect("Stream should exist after update");

    info!(
        stream_id = %stream.id,
        "Stream updated successfully"
    );

    let etag = generate_etag(&stream);

    Ok(HttpResponse::Ok()
        .insert_header(("ETag", etag))
        .json(ApiResponse::success(stream)))
}

/// DELETE /api/streams/{id} - Delete stream
pub async fn delete_stream(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;

    let stream_id = path.into_inner();

    info!(
        user_id = %user.id,
        stream_id = %stream_id,
        "Deleting stream"
    );

    let repo = CachedStreamRepository::new(state.db.pool(), state.cache.clone());

    // Check if stream exists and user has access
    let existing = repo.find_by_id(&stream_id).await.map_err(|e| {
        warn!(error = %e, "Failed to find stream");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let existing = match existing {
        Some(s) => s,
        None => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<()>::error("Stream not found".to_string())))
        }
    };

    // Check access: admin can delete all, users can delete their own
    if existing.user_id != user.id {
        return Ok(
            HttpResponse::Forbidden().json(ApiResponse::<()>::error("Access denied".to_string()))
        );
    }

    let deleted = repo.delete(&stream_id).await.map_err(|e| {
        warn!(error = %e, "Failed to delete stream");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    if !deleted {
        return Ok(
            HttpResponse::NotFound().json(ApiResponse::<()>::error("Stream not found".to_string()))
        );
    }

    info!(
        stream_id = %stream_id,
        "Stream deleted successfully"
    );

    Ok(HttpResponse::NoContent().finish())
}

/// List streams handler for actix service macro (no trailing slash)
#[actix_web::get("")]
pub async fn list_streams_service(
    query: web::Query<ListStreamsQuery>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    list_streams(query, req, state).await
}

/// Get stream handler for actix service macro
#[actix_web::get("/{id}")]
pub async fn get_stream_service(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    get_stream(path, req, state).await
}

/// Create stream handler for actix service macro (no trailing slash)
#[actix_web::post("")]
pub async fn create_stream_service(
    payload: web::Json<CreateStreamApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    create_stream(payload, req, state).await
}

/// Update stream handler for actix service macro
#[actix_web::put("/{id}")]
pub async fn update_stream_service(
    path: web::Path<String>,
    payload: web::Json<UpdateStreamApiRequest>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    update_stream(path, payload, req, state).await
}

/// Delete stream handler for actix service macro
#[actix_web::delete("/{id}")]
pub async fn delete_stream_service(
    path: web::Path<String>,
    req: HttpRequest,
    state: web::Data<crate::AppState>,
) -> ActixResult<HttpResponse> {
    delete_stream(path, req, state).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StreamConfig;
    use gl_db::Stream;
    use serde_json::json;

    #[test]
    fn test_ffmpeg_config_validation() {
        let valid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "ffmpeg",
            "source_url": "rtsp://camera/stream"
        }))
        .unwrap();
        assert!(validate_stream_config(&valid_config).is_ok());

        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "ffmpeg"
        }));
        assert!(invalid_config.is_err());
    }

    #[test]
    fn test_file_config_validation() {
        let valid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "file",
            "file_path": "/path/to/video.mp4"
        }))
        .unwrap();
        assert!(validate_stream_config(&valid_config).is_ok());

        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "file"
        }));
        assert!(invalid_config.is_err());
    }

    #[test]
    fn test_website_config_validation() {
        let valid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "website",
            "url": "https://example.com",
            "headless": true,
            "stealth": false,
            "width": 1280,
            "height": 720,
            "element_selector": "#main"
        }))
        .unwrap();
        assert!(validate_stream_config(&valid_config).is_ok());

        // Missing url
        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "website"
        }));
        assert!(invalid_config.is_err());

        // Invalid url
        let invalid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "website",
            "url": "not-a-url"
        }))
        .unwrap();
        assert!(validate_stream_config(&invalid_config).is_err());

        // Invalid field types
        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "website",
            "url": "https://example.com",
            "headless": "not-a-boolean"
        }));
        assert!(invalid_config.is_err());
    }

    #[test]
    fn test_yt_config_validation() {
        let valid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "yt",
            "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "format": "best",
            "is_live": false,
            "timeout": 60,
            "options": {
                "cookies": "/path/to/cookies.txt"
            }
        }))
        .unwrap();
        assert!(validate_stream_config(&valid_config).is_ok());

        // Missing url
        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "yt"
        }));
        assert!(invalid_config.is_err());

        // Invalid url
        let invalid_config: StreamConfig = serde_json::from_value(json!({
            "kind": "yt",
            "url": "not-a-url"
        }))
        .unwrap();
        assert!(validate_stream_config(&invalid_config).is_err());

        // Invalid field types
        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "is_live": "not-a-boolean"
        }));
        assert!(invalid_config.is_err());

        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "timeout": "not-a-number"
        }));
        assert!(invalid_config.is_err());

        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "yt",
            "url": "https://youtube.com/watch?v=test",
            "options": "not-an-object"
        }));
        assert!(invalid_config.is_err());
    }

    #[test]
    fn test_unknown_kind_validation() {
        let invalid_config = serde_json::from_value::<StreamConfig>(json!({
            "kind": "unknown"
        }));
        assert!(invalid_config.is_err());
    }

    #[test]
    fn test_etag_generation() {
        let stream = Stream {
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

        let etag = generate_etag(&stream);
        assert_eq!(etag, "\"2023-01-01T00:00:00Z\"");
    }
}
