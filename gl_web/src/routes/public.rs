//! ABOUTME: Public endpoints for authenticated users (any role)
//! ABOUTME: Provides endpoints accessible to all authenticated users

use crate::{
    middleware::auth::get_http_auth_user,
    models::{ErrorResponse, UserInfo},
    AppState,
};
use actix_web::{get, web, HttpRequest, HttpResponse, Result};
use gl_db::UserRepository;
use serde_json::json;
use tracing::{debug, warn};

/// Get current user information
#[utoipa::path(
    get,
    path = "/api/me",
    tag = "public",
    security(("bearer" = [])),
    responses(
        (status = 200, description = "Current user information", body = UserInfo),
        (status = 401, description = "Authentication required", body = ErrorResponse),
        (status = 404, description = "User not found", body = ErrorResponse),
    )
)]
#[get("/me")]
pub async fn me(state: web::Data<AppState>, req: HttpRequest) -> Result<HttpResponse> {
    // Get authenticated user from middleware
    let auth_user = match get_http_auth_user(&req) {
        Some(user) => user,
        None => {
            warn!("Authenticated user not found in request");
            return Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                "authentication_required",
                "Authentication required",
            )));
        }
    };

    debug!("Getting user info for: {}", auth_user.id);

    let user_repo = UserRepository::new(state.db.pool());

    // Fetch fresh user data from database
    match user_repo.find_by_id(&auth_user.id).await {
        Ok(Some(user)) => {
            if !user.is_active {
                warn!("Inactive user attempted to access /me: {}", user.id);
                return Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                    "account_disabled",
                    "Account is disabled",
                )));
            }

            let user_info = UserInfo {
                id: user.id,
                username: user.username,
                email: user.email,
                role: user.role,
                is_active: user.is_active,
                created_at: user.created_at,
            };

            debug!("User info retrieved successfully for: {}", user_info.id);
            Ok(HttpResponse::Ok().json(user_info))
        }
        Ok(None) => {
            warn!("User not found in database: {}", auth_user.id);
            Ok(HttpResponse::NotFound()
                .json(ErrorResponse::new("user_not_found", "User not found")))
        }
        Err(e) => {
            warn!("Database error getting user info: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Error retrieving user information",
            )))
        }
    }
}

/// Health check endpoint
#[get("/health")]
pub async fn health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION")
    })))
}

/// Get streams endpoint (placeholder)
#[get("/streams")]
pub async fn streams() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!([])))
}

/// Get alerts endpoint (placeholder)
#[get("/alerts")]
pub async fn alerts() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!([])))
}
