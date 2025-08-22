//! ABOUTME: Authentication endpoints for login and token management
//! ABOUTME: Handles user login with email/password and JWT token issuance

use crate::{
    auth::{JwtAuth, PasswordAuth},
    models::{ErrorResponse, LoginRequest, LoginResponse, UserInfo},
    AppState,
};
use actix_web::{post, web, HttpResponse, Result};
use gl_db::UserRepository;
use tracing::{debug, warn};
use validator::Validate;

/// User login endpoint
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = LoginResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 401, description = "Invalid credentials", body = ErrorResponse),
    )
)]
#[post("/login")]
pub async fn login(
    state: web::Data<AppState>,
    payload: web::Json<LoginRequest>,
) -> Result<HttpResponse> {
    debug!("Login attempt for email: {}", payload.email);

    // Validate request payload
    if let Err(validation_errors) = payload.0.validate() {
        warn!("Login validation failed: {:?}", validation_errors);
        return Ok(HttpResponse::BadRequest().json(ErrorResponse::with_details(
            "validation_failed",
            "Invalid request data",
            serde_json::to_value(validation_errors).unwrap_or_default(),
        )));
    }

    let user_repo = UserRepository::new(state.db.pool());

    // Find user by email
    match user_repo.find_by_email(&payload.email).await {
        Ok(Some(user)) => {
            if !user.is_active {
                warn!("Login attempt for inactive user: {}", user.id);
                return Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                    "account_disabled",
                    "Account is disabled",
                )));
            }

            // Verify password
            match PasswordAuth::verify_password(&payload.password, &user.password_hash) {
                Ok(true) => {
                    debug!("Password verification successful for user: {}", user.id);

                    // Create JWT token
                    match JwtAuth::create_token(
                        &user.id,
                        &user.email,
                        &user.role,
                        &state.jwt_secret,
                    ) {
                        Ok(token) => {
                            debug!("JWT token created for user: {}", user.id);

                            let response = LoginResponse {
                                access_token: token,
                                token_type: "Bearer".to_string(),
                                expires_in: JwtAuth::token_expiration_secs(),
                                user: UserInfo {
                                    id: user.id,
                                    username: user.username,
                                    email: user.email,
                                    role: user.role,
                                    is_active: user.is_active,
                                    created_at: user.created_at,
                                },
                            };

                            Ok(HttpResponse::Ok().json(response))
                        }
                        Err(e) => {
                            warn!("Failed to create JWT token: {}", e);
                            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                                "token_creation_failed",
                                "Failed to create authentication token",
                            )))
                        }
                    }
                }
                Ok(false) => {
                    warn!("Invalid password for user: {}", user.email);
                    Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                        "invalid_credentials",
                        "Invalid email or password",
                    )))
                }
                Err(e) => {
                    warn!("Password verification error: {}", e);
                    Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                        "authentication_error",
                        "Authentication system error",
                    )))
                }
            }
        }
        Ok(None) => {
            warn!("Login attempt for non-existent email: {}", payload.email);
            Ok(HttpResponse::Unauthorized().json(ErrorResponse::new(
                "invalid_credentials",
                "Invalid email or password",
            )))
        }
        Err(e) => {
            warn!("Database error during login: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "System error during authentication",
            )))
        }
    }
}
