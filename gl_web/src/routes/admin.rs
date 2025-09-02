//! ABOUTME: Admin endpoints requiring administrator role
//! ABOUTME: Provides administrative functionality for managing templates and system

use crate::{auth::PasswordAuth, models::TemplateInfo, AppState};
use actix_web::{delete, get, post, web, HttpRequest, HttpResponse, Result};
use gl_db::{CreateUserRequest, UserRepository};
use gl_update::{
    UpdateCheckResult, UpdateConfig, UpdateInfo, UpdateResult, UpdateService, UpdateStatus,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

/// Test route to verify admin access
#[get("/test")]
pub async fn test_route(_req: HttpRequest) -> Result<HttpResponse> {
    debug!("Admin test route accessed");
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "message": "Admin access verified"
    })))
}

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

/// Admin user response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminUserResponse {
    pub id: String,
    pub email: String,
    pub username: String,
    pub role: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Admin API key response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminApiKeyResponse {
    pub id: String,
    pub name: String,
    pub user_id: String,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Admin create user request
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminCreateUserRequest {
    pub email: String,
    pub username: String,
    pub password: String,
    pub role: String,
}

/// List all users (admin only)
#[get("/users")]
pub async fn list_users(state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Listing users for admin");

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.list_active().await {
        Ok(users) => {
            let admin_users: Vec<AdminUserResponse> = users
                .into_iter()
                .map(|user| AdminUserResponse {
                    id: user.id,
                    email: user.email,
                    username: user.username,
                    role: user.role,
                    is_active: user.is_active,
                    created_at: user.created_at,
                    updated_at: user.updated_at,
                })
                .collect();

            debug!("Retrieved {} users", admin_users.len());
            Ok(HttpResponse::Ok().json(admin_users))
        }
        Err(e) => {
            error!("Failed to list users: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to retrieve users",
                "details": e.to_string()
            })))
        }
    }
}

/// Create new user (admin only)
#[post("/users")]
pub async fn create_user(
    state: web::Data<AppState>,
    req: web::Json<AdminCreateUserRequest>,
    _http_req: HttpRequest,
) -> Result<HttpResponse> {
    info!("Admin creating new user: {}", req.email);

    let user_repo = UserRepository::new(state.db.pool());

    // Check if user already exists
    match user_repo.find_by_email(&req.email).await {
        Ok(Some(_)) => {
            warn!("User creation failed: email already exists: {}", req.email);
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "User already exists",
                "message": format!("User with email {} already exists", req.email)
            })));
        }
        Ok(None) => {
            // Good, user doesn't exist
        }
        Err(e) => {
            error!("Database error checking existing user: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "details": e.to_string()
            })));
        }
    }

    // Hash password
    let password_hash = match PasswordAuth::hash_password(&req.password) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash password: {}", e);
            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Password hashing failed"
            })));
        }
    };

    // Create user
    let create_request = CreateUserRequest {
        username: req.username.clone(),
        email: req.email.clone(),
        password_hash,
        role: req.role.clone(),
    };

    match user_repo.create(create_request).await {
        Ok(user) => {
            info!("User created successfully: {}", user.email);
            let admin_user = AdminUserResponse {
                id: user.id,
                email: user.email,
                username: user.username,
                role: user.role,
                is_active: user.is_active,
                created_at: user.created_at,
                updated_at: user.updated_at,
            };
            Ok(HttpResponse::Created().json(admin_user))
        }
        Err(e) => {
            error!("Failed to create user: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "User creation failed",
                "details": e.to_string()
            })))
        }
    }
}

/// Delete user (admin only)
#[delete("/users/{user_id}")]
pub async fn delete_user(
    state: web::Data<AppState>,
    path: web::Path<String>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    let user_id = path.into_inner();
    info!("Admin deleting user: {}", user_id);

    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.delete(&user_id).await {
        Ok(()) => {
            info!("User deleted successfully: {}", user_id);
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "message": "User deleted successfully"
            })))
        }
        Err(e) => {
            error!("Failed to delete user: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "User deletion failed",
                "details": e.to_string()
            })))
        }
    }
}

/// List all API keys (admin only) - TODO: Implement when find_all method is added to ApiKeyRepository
#[get("/api-keys")]
pub async fn list_api_keys(_state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Listing API keys for admin - not yet implemented");

    // TODO: Implement after adding find_all method to ApiKeyRepository
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "API key listing not yet implemented"
    })))
}

/// Delete API key (admin only) - TODO: Implement when delete method is added to ApiKeyRepository
#[delete("/api-keys/{key_id}")]
pub async fn delete_api_key(
    _state: web::Data<AppState>,
    path: web::Path<String>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    let key_id = path.into_inner();
    info!(
        "Admin requesting to delete API key: {} - not yet implemented",
        key_id
    );

    // TODO: Implement after adding delete method to ApiKeyRepository
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "API key deletion not yet implemented"
    })))
}

/// Request to trigger an update check
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCheckRequest {
    /// Force check even if recently checked
    pub force: Option<bool>,
}

/// Response for update check
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    pub result: UpdateCheckResult,
    pub status: String,
}

/// Request to apply an available update
#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyUpdateRequest {
    /// Update ID to apply (for safety)
    pub update_id: String,
    /// Confirm that admin wants to proceed
    pub confirm: bool,
}

/// Response for update application
#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyUpdateResponse {
    pub result: UpdateResult,
    pub status: String,
}

/// Status response for ongoing update operations
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateStatusResponse {
    pub status: UpdateStatus,
    pub current_version: String,
    pub last_check: Option<chrono::DateTime<chrono::Utc>>,
    pub pending_update: Option<UpdateInfo>,
}

/// Check for available updates
#[post("/updates/check")]
pub async fn check_updates(
    _state: web::Data<AppState>,
    req: web::Json<UpdateCheckRequest>,
    _http_req: HttpRequest,
) -> Result<HttpResponse> {
    info!("Admin requesting update check, force: {:?}", req.force);

    // TODO: Get update service from app state
    // For now, create a default config for demonstration
    let config = UpdateConfig {
        repository: "anthropics/glimpser-rs".to_string(),
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        public_key: "".to_string(), // Would be configured properly
        ..Default::default()
    };

    match UpdateService::new(config) {
        Ok(mut service) => match service.check_for_updates().await {
            Ok(result) => {
                info!(
                    "Update check completed: available={}",
                    result.update_available
                );

                let response = UpdateCheckResponse {
                    result,
                    status: "success".to_string(),
                };

                Ok(HttpResponse::Ok().json(response))
            }
            Err(e) => {
                error!("Update check failed: {}", e);
                Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Update check failed",
                    "details": e.to_string()
                })))
            }
        },
        Err(e) => {
            error!("Failed to initialize update service: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Update service initialization failed",
                "details": e.to_string()
            })))
        }
    }
}

/// Apply an available update
#[post("/updates/apply")]
pub async fn apply_update(
    _state: web::Data<AppState>,
    req: web::Json<ApplyUpdateRequest>,
    _http_req: HttpRequest,
) -> Result<HttpResponse> {
    info!("Admin requesting update application: {}", req.update_id);

    if !req.confirm {
        warn!("Update application rejected: confirmation required");
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Confirmation required",
            "message": "Set confirm=true to proceed with update"
        })));
    }

    // TODO: Get update service from app state and apply the update
    // This is a placeholder implementation
    warn!(
        "Update application not yet implemented - would apply update {}",
        req.update_id
    );

    Ok(HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "Not implemented",
        "message": "Update application is not yet implemented"
    })))
}

/// Get current update status
#[get("/updates/status")]
pub async fn update_status(_state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    debug!("Admin requesting update status");

    // TODO: Get actual status from app state
    let response = UpdateStatusResponse {
        status: UpdateStatus::Idle,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        last_check: None,
        pending_update: None,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Cancel an ongoing update (if possible)
#[post("/updates/cancel")]
pub async fn cancel_update(_state: web::Data<AppState>, _req: HttpRequest) -> Result<HttpResponse> {
    info!("Admin requesting update cancellation");

    // TODO: Implement update cancellation
    Ok(HttpResponse::NotImplemented().json(serde_json::json!({
        "error": "Not implemented",
        "message": "Update cancellation is not yet implemented"
    })))
}

/// Get update history/logs
#[get("/updates/history")]
pub async fn update_history(
    _state: web::Data<AppState>,
    _req: HttpRequest,
) -> Result<HttpResponse> {
    debug!("Admin requesting update history");

    // TODO: Implement update history from database
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "updates": [],
        "message": "Update history not yet implemented"
    })))
}
