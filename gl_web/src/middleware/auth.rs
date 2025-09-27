//! ABOUTME: Authentication middleware for JWT token verification only
//! ABOUTME: Extracts and validates JWT credentials from requests

use crate::{auth::JwtAuth, models::Claims, AppState};
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    Error, HttpMessage,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::rc::Rc;
use tracing::{debug, warn};

/// Authentication middleware that extracts JWT tokens
pub struct RequireAuth;

impl RequireAuth {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RequireAuth {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, B> Transform<S, ServiceRequest> for RequireAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequireAuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequireAuthMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct RequireAuthMiddleware<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for RequireAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            let mut jwt_token: Option<&str> = None;

            // Try JWT authentication from Authorization header first
            if let Some(auth_header) = req.headers().get("authorization") {
                if let Ok(auth_str) = auth_header.to_str() {
                    if let Some(token) = auth_str.strip_prefix("Bearer ") {
                        jwt_token = Some(token);
                    }
                }
            }

            // If no Authorization header, try cookie
            if jwt_token.is_none() {
                if let Some(cookie_header) = req.headers().get("cookie") {
                    if let Ok(cookie_str) = cookie_header.to_str() {
                        // Parse cookies manually to find JWT token
                        for cookie_part in cookie_str.split(';') {
                            let cookie_part = cookie_part.trim();
                            if let Some(token_value) = cookie_part.strip_prefix("auth_token=") {
                                jwt_token = Some(token_value);
                                break;
                            }
                        }
                    }
                }
            }

            // Verify JWT token if found
            if let Some(token) = jwt_token {
                if let Some(app_state) = req.app_data::<actix_web::web::Data<AppState>>() {
                    match JwtAuth::verify_token(
                        token,
                        &app_state.security_config.jwt_secret,
                        &app_state.security_config.jwt_issuer,
                    ) {
                        Ok(claims) => {
                            debug!(
                                "JWT authentication successful for user: {} (via {})",
                                claims.sub,
                                if req.headers().get("authorization").is_some() {
                                    "header"
                                } else {
                                    "cookie"
                                }
                            );
                            req.extensions_mut().insert(AuthUser::from_jwt(claims));
                            return service.call(req).await;
                        }
                        Err(e) => {
                            warn!("JWT verification failed: {}", e);
                            return Err(ErrorUnauthorized("Invalid JWT token"));
                        }
                    }
                }
            }

            // API key authentication removed - JWT only for simplified auth

            // No valid authentication found
            Err(ErrorUnauthorized("Authentication required"))
        })
    }
}

/// Authenticated user information (JWT only)
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
}

impl AuthUser {
    fn from_jwt(claims: Claims) -> Self {
        Self {
            id: claims.sub,
            email: claims.email,
        }
    }
}

/// Helper function to extract authenticated user from HTTP request
pub fn get_http_auth_user(req: &actix_web::HttpRequest) -> Option<AuthUser> {
    req.extensions().get::<AuthUser>().cloned()
}
