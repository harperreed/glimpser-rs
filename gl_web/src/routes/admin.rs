//! ABOUTME: Settings endpoints for template, user, and API key management
//! ABOUTME: Simplified settings functionality without role-based access control

use crate::{models::TemplateInfo, AppState};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Result};
use gl_db::{
    ApiKeyRepository, CreateApiKeyRequest, CreateTemplateRequest, CreateUserRequest,
    UpdateTemplateRequest, UserRepository,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tracing::{debug, error, info, warn};

/// List all templates
#[get("/templates")]
pub async fn list_templates(state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Listing templates for admin user");

    // Get templates from database
    let template_repo = gl_db::TemplateRepository::new(state.db.pool());

    match template_repo.list(None, 0, 100).await {
        Ok(templates) => {
            debug!(
                "Templates retrieved successfully, count: {}",
                templates.len()
            );

            // Convert to TemplateInfo format expected by admin panel
            let template_infos: Vec<TemplateInfo> = templates
                .into_iter()
                .map(|t| {
                    // Extract type from config JSON
                    let template_type = match serde_json::from_str::<serde_json::Value>(&t.config) {
                        Ok(config) => config
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        Err(_) => "unknown".to_string(),
                    };

                    TemplateInfo {
                        id: t.id,
                        user_id: t.user_id,
                        name: t.name,
                        description: t.description,
                        template_type,
                        is_default: t.is_default,
                        created_at: t.created_at,
                        updated_at: t.updated_at,
                    }
                })
                .collect();

            Ok(HttpResponse::Ok().json(template_infos))
        }
        Err(e) => {
            error!("Failed to retrieve templates: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve templates",
                "details": e.to_string()
            })))
        }
    }
}

/// Create template request
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTemplateRequestBody {
    pub name: String,
    pub description: Option<String>,
    pub config: String,
    pub is_default: Option<bool>,
}

/// Create a new template
#[post("/templates")]
pub async fn create_template(
    state: web::Data<AppState>,
    req: web::Json<CreateTemplateRequestBody>,
) -> Result<HttpResponse> {
    debug!("Creating new template: {}", req.name);

    let template_repo = gl_db::TemplateRepository::new(state.db.pool());

    // Validate JSON config
    if serde_json::from_str::<serde_json::Value>(&req.config).is_err() {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Invalid JSON configuration"
        })));
    }

    let create_req = CreateTemplateRequest {
        user_id: "admin".to_string(), // For now, use admin as default user_id
        name: req.name.clone(),
        description: req.description.clone(),
        config: req.config.clone(),
        is_default: req.is_default.unwrap_or(false),
    };

    match template_repo.create(create_req).await {
        Ok(template) => {
            info!("Template created successfully: {}", template.id);
            Ok(HttpResponse::Created().json(template))
        }
        Err(e) => {
            error!("Failed to create template: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to create template",
                "details": e.to_string()
            })))
        }
    }
}

/// Update template request
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTemplateRequestBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<String>,
    pub is_default: Option<bool>,
}

/// Update an existing template
#[post("/templates/{id}")]
pub async fn update_template(
    state: web::Data<AppState>,
    path: web::Path<String>,
    req: web::Json<UpdateTemplateRequestBody>,
) -> Result<HttpResponse> {
    let template_id = path.into_inner();
    debug!("Updating template: {}", template_id);

    let template_repo = gl_db::TemplateRepository::new(state.db.pool());

    // Validate JSON config if provided
    if let Some(ref config) = req.config {
        if serde_json::from_str::<serde_json::Value>(config).is_err() {
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Invalid JSON configuration"
            })));
        }
    }

    let update_req = UpdateTemplateRequest {
        name: req.name.clone(),
        description: req.description.clone(),
        config: req.config.clone(),
        is_default: req.is_default,
    };

    match template_repo.update(&template_id, update_req).await {
        Ok(template) => {
            info!("Template updated successfully: {}", template_id);
            Ok(HttpResponse::Ok().json(template))
        }
        Err(e) => {
            error!("Failed to update template {}: {}", template_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to update template",
                "details": e.to_string()
            })))
        }
    }
}

/// Delete a template
#[delete("/templates/{id}")]
pub async fn delete_template(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let template_id = path.into_inner();
    debug!("Deleting template: {}", template_id);

    let template_repo = gl_db::TemplateRepository::new(state.db.pool());

    match template_repo.delete(&template_id).await {
        Ok(_) => {
            info!("Template deleted successfully: {}", template_id);
            Ok(HttpResponse::NoContent().finish())
        }
        Err(e) => {
            error!("Failed to delete template {}: {}", template_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to delete template",
                "details": e.to_string()
            })))
        }
    }
}

/// Get a specific template
#[get("/templates/{id}")]
pub async fn get_template(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse> {
    let template_id = path.into_inner();
    debug!("Getting template: {}", template_id);

    let template_repo = gl_db::TemplateRepository::new(state.db.pool());

    match template_repo.find_by_id(&template_id).await {
        Ok(Some(template)) => {
            debug!("Template retrieved successfully: {}", template_id);
            Ok(HttpResponse::Ok().json(template))
        }
        Ok(None) => {
            warn!("Template not found: {}", template_id);
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "Template not found"
            })))
        }
        Err(e) => {
            error!("Failed to retrieve template {}: {}", template_id, e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve template",
                "details": e.to_string()
            })))
        }
    }
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
                    created_at: chrono::DateTime::parse_from_rfc3339(&u.created_at)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&u.updated_at)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
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
    let password_hash = match crate::auth::PasswordAuth::hash_password(&req.password) {
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
                created_at: chrono::DateTime::parse_from_rfc3339(&user.created_at)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&user.updated_at)
                    .unwrap()
                    .with_timezone(&chrono::Utc),
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
                    created_at: chrono::DateTime::parse_from_rfc3339(&k.created_at)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&k.updated_at)
                        .unwrap()
                        .with_timezone(&chrono::Utc),
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
) -> Result<HttpResponse> {
    debug!("Creating new API key: {}", req.name);

    let api_key_repo = ApiKeyRepository::new(state.db.pool());

    // Generate a new API key
    let api_key = gl_core::Id::new().to_string();
    let key_hash = format!("{:x}", sha2::Sha256::digest(&api_key));

    let create_req = CreateApiKeyRequest {
        user_id: "admin".to_string(), // For now, use admin as default user_id
        name: req.name.clone(),
        key_hash: key_hash.clone(),
        permissions: "read,write".to_string(), // Default permissions
        expires_at: None,                      // No expiration
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
