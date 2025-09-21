//! ABOUTME: Authentication endpoints for login and token management
//! ABOUTME: Handles user login with email/password and JWT token issuance

use crate::{
    auth::{JwtAuth, PasswordAuth},
    models::{ErrorResponse, LoginRequest, LoginResponse, SignupRequest, UserInfo},
    AppState,
};
use actix_web::{get, post, web, HttpResponse, Result};
use gl_db::{CreateUserRequest, UserRepository};
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
            if !user.is_active.unwrap_or(false) {
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
                        &state.security_config.jwt_secret,
                    ) {
                        Ok(token) => {
                            debug!("JWT token created for user: {}", user.id);

                            let response = LoginResponse {
                                access_token: token.clone(),
                                token_type: "Bearer".to_string(),
                                expires_in: JwtAuth::token_expiration_secs(),
                                user: UserInfo {
                                    id: user.id,
                                    username: user.username,
                                    email: user.email,
                                    is_active: user.is_active.unwrap_or(false),
                                    is_admin: true, // All users are admin in this system
                                    created_at: user.created_at,
                                },
                            };

                            // Set JWT token as HTTP-only cookie for image requests
                            let cookie = actix_web::cookie::Cookie::build("auth_token", token)
                                .path("/")
                                .max_age(actix_web::cookie::time::Duration::seconds(
                                    JwtAuth::token_expiration_secs() as i64,
                                ))
                                .http_only(true)
                                .secure(state.security_config.secure_cookies)
                                .same_site(actix_web::cookie::SameSite::Lax)
                                .finish();

                            Ok(HttpResponse::Ok().cookie(cookie).json(response))
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

/// Check if setup is needed (no admin users exist)
#[utoipa::path(
    get,
    path = "/api/auth/setup/needed",
    tag = "auth",
    responses(
        (status = 200, description = "Setup status", body = serde_json::Value),
    )
)]
#[get("/setup/needed")]
pub async fn setup_needed(state: web::Data<AppState>) -> Result<HttpResponse> {
    let user_repo = UserRepository::new(state.db.pool());

    match user_repo.has_any_users().await {
        Ok(has_users) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "needs_setup": !has_users
        }))),
        Err(e) => {
            warn!("Failed to check user count: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to check setup status",
            )))
        }
    }
}

/// First admin user signup endpoint (only works when no users exist)
#[utoipa::path(
    post,
    path = "/api/auth/setup/signup",
    tag = "auth",
    request_body = SignupRequest,
    responses(
        (status = 201, description = "Admin user created successfully", body = LoginResponse),
        (status = 400, description = "Invalid request or setup already complete", body = ErrorResponse),
        (status =409, description = "Users already exist", body = ErrorResponse),
    )
)]
#[post("/setup/signup")]
pub async fn setup_signup(
    state: web::Data<AppState>,
    payload: web::Json<SignupRequest>,
) -> Result<HttpResponse> {
    debug!("First admin signup attempt for email: {}", payload.email);

    // Validate request payload
    if let Err(validation_errors) = payload.0.validate() {
        warn!("Signup validation failed: {:?}", validation_errors);
        return Ok(HttpResponse::BadRequest().json(ErrorResponse::with_details(
            "validation_failed",
            "Invalid request data",
            serde_json::to_value(validation_errors).unwrap_or_default(),
        )));
    }

    let user_repo = UserRepository::new(state.db.pool());

    // Check if users already exist
    match user_repo.has_any_users().await {
        Ok(true) => {
            warn!("Admin signup attempted but users already exist");
            return Ok(HttpResponse::Conflict().json(ErrorResponse::new(
                "setup_complete",
                "Setup is already complete. Users exist in the system.",
            )));
        }
        Ok(false) => {
            debug!("No users exist, proceeding with first admin creation");
        }
        Err(e) => {
            warn!("Failed to check user count: {}", e);
            return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "database_error",
                "Failed to check setup status",
            )));
        }
    }

    // Hash the password
    let password_hash = match PasswordAuth::hash_password(&payload.password) {
        Ok(hash) => hash,
        Err(e) => {
            warn!("Failed to hash password: {}", e);
            return Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "password_hash_failed",
                "Failed to secure password",
            )));
        }
    };

    // Create the user
    let create_request = CreateUserRequest {
        username: payload.username.clone(),
        email: payload.email.clone(),
        password_hash,
    };

    match user_repo.create(create_request).await {
        Ok(user) => {
            debug!("First admin user created successfully: {}", user.id);

            // Create JWT token for immediate login
            match JwtAuth::create_token(&user.id, &user.email, &state.security_config.jwt_secret) {
                Ok(token) => {
                    debug!("JWT token created for first admin: {}", user.id);

                    let response = LoginResponse {
                        access_token: token.clone(),
                        token_type: "Bearer".to_string(),
                        expires_in: JwtAuth::token_expiration_secs(),
                        user: UserInfo {
                            id: user.id,
                            username: user.username,
                            email: user.email,
                            is_active: user.is_active.unwrap_or(false),
                            is_admin: true, // First user is admin
                            created_at: user.created_at,
                        },
                    };

                    // Set JWT token as HTTP-only cookie
                    let cookie = actix_web::cookie::Cookie::build("auth_token", token)
                        .path("/")
                        .max_age(actix_web::cookie::time::Duration::seconds(
                            JwtAuth::token_expiration_secs() as i64,
                        ))
                        .http_only(true)
                        .secure(state.security_config.secure_cookies)
                        .same_site(actix_web::cookie::SameSite::Lax)
                        .finish();

                    Ok(HttpResponse::Created().cookie(cookie).json(response))
                }
                Err(e) => {
                    warn!("Failed to create JWT token for first admin: {}", e);
                    Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                        "token_creation_failed",
                        "Failed to create authentication token",
                    )))
                }
            }
        }
        Err(e) => {
            warn!("Failed to create first admin user: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "user_creation_failed",
                "Failed to create admin user",
            )))
        }
    }
}
