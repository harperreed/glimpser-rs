//! ABOUTME: Settings endpoints for stream, user, and API key management
//! ABOUTME: Simplified settings functionality without role-based access control

use crate::{models::AdminStreamInfo, AppState};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Result};
use chrono::{DateTime, TimeZone, Utc};
use gl_db::{
    ApiKeyRepository, CreateApiKeyRequest, CreateStreamRequest, CreateUserRequest,
    StreamRepository, UpdateStreamRequest, UserRepository,
};
use gl_update::{UpdateCheckResult, UpdateInfo};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tracing::{debug, error, info, warn};

fn parse_timestamp_to_utc(s: &str) -> DateTime<Utc> {
    // Try RFC3339 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    // Fallback: our simplified format "secs.nanos"
    if let Some((secs_str, nanos_str)) = s.split_once('.') {
        if let (Ok(secs), Ok(nanos)) = (secs_str.parse::<i64>(), nanos_str.parse::<u32>()) {
            if let chrono::LocalResult::Single(dt) = Utc.timestamp_opt(secs, nanos) {
                return dt;
            }
        }
    }
    // Last resort: now
    Utc::now()
}
use crate::middleware::auth::get_http_auth_user;

/// Create stream request
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStreamRequestBody {
    pub name: String,
    pub description: Option<String>,
    pub config: serde_json::Value,
    pub is_default: Option<bool>,
}

/// Update stream request
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateStreamRequestBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<serde_json::Value>,
    pub is_default: Option<bool>,
}

// User Management Endpoints

/// User response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: String,
    pub username: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// List all users
#[get("/users")]
pub async fn list_users(state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Listing users");

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.list_active().await {
        Ok(users) => {
            debug!("Users retrieved successfully, count: {}", users.len());

            let user_responses: Vec<UserResponse> = users
                .into_iter()
                .map(|u| UserResponse {
                    id: u.id,
                    username: u.username,
                    email: u.email,
                    created_at: parse_timestamp_to_utc(&u.created_at),
                    updated_at: parse_timestamp_to_utc(&u.updated_at),
                })
                .collect();

            Ok(HttpResponse::Ok().json(user_responses))
        }
        Err(e) => {
            error!("Failed to retrieve users: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve users",
                "details": e.to_string()
            })))
        }
    }
}

/// Create user request
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserRequestBody {
    pub username: String,
    pub email: String,
    pub password: String,
}

/// Create a new user
#[post("/users")]
pub async fn create_user(
    state: web::Data<AppState>,
    req: web::Json<CreateUserRequestBody>,
) -> Result<HttpResponse> {
    debug!("Creating new user: {}", req.username);

    let user_repo = UserRepository::new(state.db.pool());

    // Hash the password
    let password_hash = match crate::auth::PasswordAuth::hash_password(
        &req.password,
        &state.security_config.argon2_params,
    ) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash password: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to hash password"
            })));
        }
    };

    let create_req = CreateUserRequest {
        username: req.username.clone(),
        email: req.email.clone(),
        password_hash,
    };

    match user_repo.create(create_req).await {
        Ok(user) => {
            info!("User created successfully: {}", user.id);
            let user_response = UserResponse {
                id: user.id,
                username: user.username,
                email: user.email,
                created_at: parse_timestamp_to_utc(&user.created_at),
                updated_at: parse_timestamp_to_utc(&user.updated_at),
            };
            Ok(HttpResponse::Created().json(user_response))
        }
        Err(e) => {
            error!("Failed to create user: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create user",
                "details": e.to_string()
            })))
        }
    }
}

/// Delete a user
#[delete("/users/{id}")]
pub async fn delete_user(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    debug!("Deleting user: {}", user_id);

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.delete(&user_id).await {
        Ok(_) => {
            info!("User deleted successfully: {}", user_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete user {}: {}", user_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete user",
                "details": e.to_string()
            })))
        }
    }
}

/// Get a specific user
#[get("/users/{id}")]
pub async fn get_user(state: web::Data<AppState>, path: web::Path<String>) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    debug!("Getting user: {}", user_id);

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.find_by_id(&user_id).await {
        Ok(Some(user)) => {
            debug!("User retrieved successfully: {}", user_id);
            let user_response = UserResponse {
                id: user.id,
                username: user.username,
                email: user.email,
                created_at: chrono::DateTime::parse_from_rfc3339(&user.created_at)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&user.updated_at)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            };
            Ok(HttpResponse::Ok().json(user_response))
        }
        Ok(None) => {
            warn!("User not found: {}", user_id);
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "User not found"
            })))
        }
        Err(e) => {
            error!("Failed to retrieve user {}: {}", user_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve user",
                "details": e.to_string()
            })))
        }
    }
}

// API Key Management Endpoints

/// API Key response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyResponse {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// List all API keys
#[get("/api-keys")]
pub async fn list_api_keys(state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Listing API keys");

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    match api_key_repo.list_all(100, 0).await {
        Ok(api_keys) => {
            debug!("API keys retrieved successfully, count: {}", api_keys.len());

            let api_key_responses: Vec<ApiKeyResponse> = api_keys
                .into_iter()
                .map(|k| ApiKeyResponse {
                    id: k.id,
                    name: k.name,
                    key_hash: k.key_hash,
                    created_at: parse_timestamp_to_utc(&k.created_at),
                    updated_at: parse_timestamp_to_utc(&k.updated_at),
                })
                .collect();

            Ok(HttpResponse::Ok().json(api_key_responses))
        }
        Err(e) => {
            error!("Failed to retrieve API keys: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve API keys",
                "details": e.to_string()
            })))
        }
    }
}

/// Create API key request
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateApiKeyRequestBody {
    pub name: String,
}

/// Create a new API key
#[post("/api-keys")]
pub async fn create_api_key(
    state: web::Data<AppState>,
    req: web::Json<CreateApiKeyRequestBody>,
    http: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Creating new API key: {}", req.name);

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    // Validate authenticated user
    let user = match get_http_auth_user(&http) {
        Some(u) => u,
        None => {
            return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required"
            })));
        }
    };

    // Generate a new API key
    let api_key = gl_core::Id::new().to_string();
    let key_hash = format!("{:x}", sha2::Sha256::digest(&api_key));

    let create_req = CreateApiKeyRequest {
        user_id: user.id,
        name: req.name.clone(),
        key_hash: key_hash.clone(),
        permissions: serde_json::to_string(&["read", "write"]).unwrap(),
        expires_at: None, // No expiration
    };

    match api_key_repo.create(create_req).await {
        Ok(created_key) => {
            info!("API key created successfully: {}", created_key.id);
            Ok(HttpResponse::Created().json(serde_json::json!({
                "id": created_key.id,
                "name": created_key.name,
                "api_key": api_key, // Return the actual key only on creation
                "created_at": created_key.created_at,
                "updated_at": created_key.updated_at
            })))
        }
        Err(e) => {
            error!("Failed to create API key: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create API key",
                "details": e.to_string()
            })))
        }
    }
}

/// Delete an API key
#[delete("/api-keys/{id}")]
pub async fn delete_api_key(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let api_key_id = path.into_inner();
    debug!("Deleting API key: {}", api_key_id);

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    match api_key_repo.delete(&api_key_id).await {
        Ok(_) => {
            info!("API key deleted successfully: {}", api_key_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete API key {}: {}", api_key_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete API key",
                "details": e.to_string()
            })))
        }
    }
}

// Plain handler wrappers (for explicit resource mapping)
// These call into the attribute-annotated endpoints above.

pub async fn list_streams_handler(
    state: web::Data<AppState>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Listing streams for admin user");

    let stream_repo = StreamRepository::new(state.db.pool());

    match stream_repo.list(None, 0, 100).await {
        Ok(streams) => {
            debug!("Streams retrieved successfully, count: {}", streams.len());

            let mut stream_infos: Vec<AdminStreamInfo> = Vec::new();

            for t in streams {
                let stream_type = match serde_json::from_str::<serde_json::Value>(&t.config) {
                    Ok(config) => config
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    Err(_) => "unknown".to_string(),
                };

                // Check if stream is running with capture manager
                // Wrap in error handling to prevent endpoint crash
                let status = match state.capture_manager.is_stream_running(&t.id).await {
                    true => "active".to_string(),
                    false => "inactive".to_string(),
                };

                stream_infos.push(AdminStreamInfo {
                    id: t.id,
                    user_id: t.user_id,
                    name: t.name,
                    description: t.description,
                    stream_type,
                    is_default: t.is_default,
                    created_at: t.created_at,
                    updated_at: t.updated_at,
                    status,
                });
            }

            Ok(HttpResponse::Ok().json(stream_infos))
        }
        Err(e) => {
            error!("Failed to retrieve streams: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve streams",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn get_stream_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let stream_id = path.into_inner();
    debug!("Getting stream: {}", stream_id);

    let stream_repo = StreamRepository::new(state.db.pool());

    match stream_repo.find_by_id(&stream_id).await {
        Ok(Some(stream)) => {
            debug!("Stream retrieved successfully: {}", stream_id);
            Ok(HttpResponse::Ok().json(stream))
        }
        Ok(None) => {
            warn!("Stream not found: {}", stream_id);
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "Stream not found"
            })))
        }
        Err(e) => {
            error!("Failed to retrieve stream {}: {}", stream_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve stream",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn create_stream_handler(
    state: web::Data<AppState>,
    req: web::Json<CreateStreamRequestBody>,
    http: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Creating new stream: {}", req.name);

    let stream_repo = StreamRepository::new(state.db.pool());

    let user = match get_http_auth_user(&http) {
        Some(u) => u,
        None => {
            return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required"
            })));
        }
    };

    if !req.config.is_object() {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Invalid JSON configuration: expected an object"
        })));
    }

    let create_req = CreateStreamRequest {
        user_id: user.id,
        name: req.name.clone(),
        description: req.description.clone(),
        config: match serde_json::to_string(&req.config) {
            Ok(s) => s,
            Err(_) => {
                return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                    "error": "Invalid JSON configuration"
                })));
            }
        },
        is_default: req.is_default.unwrap_or(false),
    };

    match stream_repo.create(create_req).await {
        Ok(stream) => {
            info!("Stream created successfully: {}", stream.id);
            Ok(HttpResponse::Created().json(stream))
        }
        Err(e) => {
            error!("Failed to create stream: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create stream",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn update_stream_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
    req: web::Json<UpdateStreamRequestBody>,
) -> Result<HttpResponse> {
    let stream_id = path.into_inner();
    debug!("Updating stream: {}", stream_id);

    let stream_repo = StreamRepository::new(state.db.pool());

    if let Some(ref config) = req.config {
        if !config.is_object() {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Invalid JSON configuration: expected an object"
            })));
        }
    }

    let update_req = UpdateStreamRequest {
        name: req.name.clone(),
        description: req.description.clone(),
        config: match &req.config {
            Some(v) => Some(match serde_json::to_string(v) {
                Ok(s) => s,
                Err(_) => {
                    return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                        "error": "Invalid JSON configuration"
                    })));
                }
            }),
            None => None,
        },
        is_default: req.is_default,
    };

    match stream_repo.update(&stream_id, update_req).await {
        Ok(stream) => {
            info!("Stream updated successfully: {}", stream_id);
            Ok(HttpResponse::Ok().json(stream))
        }
        Err(e) => {
            error!("Failed to update stream {}: {}", stream_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to update stream",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn delete_stream_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let stream_id = path.into_inner();
    debug!("Deleting stream: {}", stream_id);

    let stream_repo = StreamRepository::new(state.db.pool());

    match stream_repo.delete(&stream_id).await {
        Ok(_) => {
            info!("Stream deleted successfully: {}", stream_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete stream {}: {}", stream_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete stream",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn list_users_handler(
    state: web::Data<AppState>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Listing users");

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.list_active().await {
        Ok(users) => {
            debug!("Users retrieved successfully, count: {}", users.len());

            let user_responses: Vec<UserResponse> = users
                .into_iter()
                .map(|u| UserResponse {
                    id: u.id,
                    username: u.username,
                    email: u.email,
                    created_at: parse_timestamp_to_utc(&u.created_at),
                    updated_at: parse_timestamp_to_utc(&u.updated_at),
                })
                .collect();

            Ok(HttpResponse::Ok().json(user_responses))
        }
        Err(e) => {
            error!("Failed to retrieve users: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve users",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn get_user_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    debug!("Getting user: {}", user_id);

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.find_by_id(&user_id).await {
        Ok(Some(user)) => {
            debug!("User retrieved successfully: {}", user_id);
            let user_response = UserResponse {
                id: user.id,
                username: user.username,
                email: user.email,
                created_at: parse_timestamp_to_utc(&user.created_at),
                updated_at: parse_timestamp_to_utc(&user.updated_at),
            };
            Ok(HttpResponse::Ok().json(user_response))
        }
        Ok(None) => {
            warn!("User not found: {}", user_id);
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "User not found"
            })))
        }
        Err(e) => {
            error!("Failed to retrieve user {}: {}", user_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve user",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn create_user_handler(
    state: web::Data<AppState>,
    req: web::Json<CreateUserRequestBody>,
) -> Result<HttpResponse> {
    debug!("Creating new user: {}", req.username);

    let user_repo = UserRepository::new(state.db.pool());

    let password_hash = match crate::auth::PasswordAuth::hash_password(
        &req.password,
        &state.security_config.argon2_params,
    ) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash password: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to hash password"
            })));
        }
    };

    let create_req = CreateUserRequest {
        username: req.username.clone(),
        email: req.email.clone(),
        password_hash,
    };

    match user_repo.create(create_req).await {
        Ok(user) => {
            info!("User created successfully: {}", user.id);
            let user_response = UserResponse {
                id: user.id,
                username: user.username,
                email: user.email,
                created_at: parse_timestamp_to_utc(&user.created_at),
                updated_at: parse_timestamp_to_utc(&user.updated_at),
            };
            Ok(HttpResponse::Created().json(user_response))
        }
        Err(e) => {
            error!("Failed to create user: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create user",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn delete_user_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    debug!("Deleting user: {}", user_id);

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.delete(&user_id).await {
        Ok(_) => {
            info!("User deleted successfully: {}", user_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete user {}: {}", user_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete user",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn list_api_keys_handler(
    state: web::Data<AppState>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Listing API keys");

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    match api_key_repo.list_all(100, 0).await {
        Ok(api_keys) => {
            debug!("API keys retrieved successfully, count: {}", api_keys.len());

            let api_key_responses: Vec<ApiKeyResponse> = api_keys
                .into_iter()
                .map(|k| ApiKeyResponse {
                    id: k.id,
                    name: k.name,
                    key_hash: k.key_hash,
                    created_at: parse_timestamp_to_utc(&k.created_at),
                    updated_at: parse_timestamp_to_utc(&k.updated_at),
                })
                .collect();

            Ok(HttpResponse::Ok().json(api_key_responses))
        }
        Err(e) => {
            error!("Failed to retrieve API keys: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve API keys",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn create_api_key_handler(
    state: web::Data<AppState>,
    req: web::Json<CreateApiKeyRequestBody>,
    http: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Creating new API key: {}", req.name);

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    let user = match get_http_auth_user(&http) {
        Some(u) => u,
        None => {
            return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
                "error": "Authentication required"
            })));
        }
    };

    let api_key = gl_core::Id::new().to_string();
    let key_hash = format!("{:x}", sha2::Sha256::digest(&api_key));

    let create_req = CreateApiKeyRequest {
        user_id: user.id,
        name: req.name.clone(),
        key_hash: key_hash.clone(),
        permissions: serde_json::to_string(&["read", "write"]).unwrap(),
        expires_at: None,
    };

    match api_key_repo.create(create_req).await {
        Ok(created_key) => {
            info!("API key created successfully: {}", created_key.id);
            Ok(HttpResponse::Created().json(serde_json::json!({
                "id": created_key.id,
                "name": created_key.name,
                "api_key": api_key,
                "created_at": created_key.created_at,
                "updated_at": created_key.updated_at
            })))
        }
        Err(e) => {
            error!("Failed to create API key: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create API key",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn delete_api_key_handler(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let api_key_id = path.into_inner();
    debug!("Deleting API key: {}", api_key_id);

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    match api_key_repo.delete(&api_key_id).await {
        Ok(_) => {
            info!("API key deleted successfully: {}", api_key_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete API key {}: {}", api_key_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete API key",
                "details": e.to_string()
            })))
        }
    }
}

/// Export stream configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct StreamExport {
    pub name: String,
    pub description: Option<String>,
    pub config: serde_json::Value,
    pub is_default: bool,
}

/// Export all streams as JSON
pub async fn export_streams(state: web::Data<AppState>, req: HttpRequest) -> Result<HttpResponse> {
    // Check authentication
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;
    debug!("Exporting streams for user: {}", user.id);

    let stream_repo = StreamRepository::new(state.db.pool());

    // Get all streams for the user
    match stream_repo.list(Some(&user.id), 0, 1000).await {
        Ok(streams) => {
            let exports: Vec<StreamExport> = streams
                .into_iter()
                .map(|stream| {
                    let config = serde_json::from_str(&stream.config)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    StreamExport {
                        name: stream.name,
                        description: stream.description,
                        config,
                        is_default: stream.is_default,
                    }
                })
                .collect();

            info!("Exported {} streams for user {}", exports.len(), user.id);
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "streams": exports,
                "export_date": chrono::Utc::now().to_rfc3339(),
                "user_id": user.id
            })))
        }
        Err(e) => {
            error!("Failed to export streams: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to export streams",
                "details": e.to_string()
            })))
        }
    }
}

/// Import stream configuration request
#[derive(Debug, Deserialize)]
pub struct StreamImportRequest {
    pub streams: Vec<StreamExport>,
    pub overwrite_mode: Option<String>, // "skip", "overwrite", or "create_new"
}

/// Import streams from JSON
pub async fn import_streams(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<StreamImportRequest>,
) -> Result<HttpResponse> {
    // Check authentication
    let user = get_http_auth_user(&req)
        .ok_or_else(|| actix_web::error::ErrorUnauthorized("Authentication required"))?;
    debug!(
        "Importing {} streams for user: {}",
        body.streams.len(),
        user.id
    );

    let stream_repo = StreamRepository::new(state.db.pool());
    let overwrite_mode = body.overwrite_mode.as_deref().unwrap_or("skip");

    let mut imported = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    for stream_export in &body.streams {
        // Check if stream with same name exists
        let existing = stream_repo
            .find_by_name_and_user(&stream_export.name, &user.id)
            .await
            .ok()
            .flatten();

        let mut stream_name = stream_export.name.clone();

        match overwrite_mode {
            "skip" if existing.is_some() => {
                skipped += 1;
                continue;
            }
            "overwrite" if existing.is_some() => {
                // Delete existing stream first
                if let Some(existing_stream) = existing {
                    if let Err(e) = stream_repo.delete(&existing_stream.id).await {
                        errors.push(format!(
                            "Failed to delete existing stream '{}': {}",
                            stream_export.name, e
                        ));
                        continue;
                    }
                }
            }
            "create_new" if existing.is_some() => {
                // Append number to make unique name
                let mut counter = 1;
                loop {
                    stream_name = format!("{} ({})", stream_export.name, counter);
                    let check = stream_repo
                        .find_by_name_and_user(&stream_name, &user.id)
                        .await
                        .ok()
                        .flatten();
                    if check.is_none() {
                        break;
                    }
                    counter += 1;
                    if counter > 100 {
                        errors.push(format!(
                            "Could not find unique name for stream '{}'",
                            stream_export.name
                        ));
                        continue;
                    }
                }
            }
            _ => {} // For "skip" with no existing, or any other case, proceed normally
        }

        // Create new stream
        let create_request = CreateStreamRequest {
            user_id: user.id.clone(),
            name: stream_name,
            description: stream_export.description.clone(),
            config: stream_export.config.to_string(),
            is_default: stream_export.is_default,
        };

        match stream_repo.create(create_request).await {
            Ok(_) => imported += 1,
            Err(e) => {
                errors.push(format!(
                    "Failed to import stream '{}': {}",
                    stream_export.name, e
                ));
            }
        }
    }

    let response = serde_json::json!({
        "imported": imported,
        "skipped": skipped,
        "errors": errors,
        "total": body.streams.len()
    });

    if errors.is_empty() {
        info!(
            "Successfully imported {} streams, skipped {} for user {}",
            imported, skipped, user.id
        );
        Ok(HttpResponse::Ok().json(response))
    } else {
        warn!(
            "Import completed with errors for user {}: imported={}, skipped={}, errors={}",
            user.id,
            imported,
            skipped,
            errors.len()
        );
        Ok(HttpResponse::PartialContent().json(response))
    }
}

// Software Update Management Endpoints

/// Request body for applying updates
#[derive(Debug, Deserialize)]
pub struct ApplyUpdateRequest {
    pub update_info: UpdateInfo,
}

/// Response for update status
#[derive(Debug, Serialize)]
pub struct UpdateStatusResponse {
    pub status: String,
    pub current_version: String,
    pub available_update: Option<UpdateInfo>,
    pub last_check: Option<DateTime<Utc>>,
}

/// Check for available software updates
pub async fn check_updates_handler(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse> {
    debug!("Checking for software updates");

    let mut update_service = state.update_service.lock().await;

    match update_service.check_for_updates().await {
        Ok(check_result) => {
            debug!(
                "Update check completed: available={}, version={}",
                check_result.update_available, check_result.current_version
            );

            let response = serde_json::json!({
                "success": true,
                "data": check_result,
                "message": if check_result.update_available {
                    "Update available"
                } else {
                    "No updates available"
                }
            });

            Ok(HttpResponse::Ok().json(response))
        }
        Err(e) => {
            error!("Failed to check for updates: {}", e);
            let error_result = UpdateCheckResult::error(
                "unknown".to_string(),
                format!("Failed to check for updates: {}", e),
            );

            let response = serde_json::json!({
                "success": false,
                "error": "Failed to check for updates",
                "data": error_result
            });

            Ok(HttpResponse::InternalServerError().json(response))
        }
    }
}

/// Apply a software update
pub async fn apply_update_handler(
    _req: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<ApplyUpdateRequest>,
) -> Result<HttpResponse> {
    info!(
        "Applying software update to version: {}",
        body.update_info.version
    );

    let mut update_service = state.update_service.lock().await;

    match update_service.apply_update(body.update_info.clone()).await {
        Ok(update_result) => {
            if update_result.success {
                info!(
                    "Update applied successfully: {} -> {}",
                    update_result.previous_version,
                    update_result.new_version.as_deref().unwrap_or("unknown")
                );

                let response = serde_json::json!({
                    "success": true,
                    "data": update_result,
                    "message": "Update applied successfully"
                });

                Ok(HttpResponse::Ok().json(response))
            } else {
                warn!(
                    "Update failed: {} -> error: {}",
                    update_result.previous_version,
                    update_result.error.as_deref().unwrap_or("unknown")
                );

                let response = serde_json::json!({
                    "success": false,
                    "error": update_result.error.as_deref().unwrap_or("Update failed"),
                    "data": update_result
                });

                Ok(HttpResponse::InternalServerError().json(response))
            }
        }
        Err(e) => {
            error!("Failed to apply update: {}", e);

            let response = serde_json::json!({
                "success": false,
                "error": format!("Failed to apply update: {}", e)
            });

            Ok(HttpResponse::InternalServerError().json(response))
        }
    }
}

/// Get current update status
pub async fn get_update_status_handler(
    _req: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse> {
    debug!("Getting update status");

    let update_service = state.update_service.lock().await;
    let status = update_service.status();

    // For now, we'll return basic status information
    // In a full implementation, this could include more detailed status tracking
    let status_response = UpdateStatusResponse {
        status: status.as_str().to_string(),
        current_version: "unknown".to_string(), // Could be extracted from config
        available_update: None,                 // Could be stored from last check
        last_check: None,                       // Could be tracked separately
    };

    let response = serde_json::json!({
        "success": true,
        "data": status_response,
        "message": "Update status retrieved"
    });

    Ok(HttpResponse::Ok().json(response))
}
