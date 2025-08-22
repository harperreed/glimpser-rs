//! ABOUTME: Authentication middleware for JWT and API key verification
//! ABOUTME: Extracts and validates authentication credentials from requests

use crate::{auth::JwtAuth, models::Claims, AppState};
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpMessage, 
    error::ErrorUnauthorized,
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use gl_db::{ApiKeyRepository, UserRepository};
use std::{
    rc::Rc,
};
use tracing::{debug, warn};

/// Authentication middleware that extracts JWT or API key
pub struct RequireAuth;

impl RequireAuth {
    pub fn new() -> Self {
        Self
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
            // Try JWT authentication first
            if let Some(auth_header) = req.headers().get("authorization") {
                if let Ok(auth_str) = auth_header.to_str() {
                    if auth_str.starts_with("Bearer ") {
                        let token = &auth_str[7..];
                        
                        if let Some(app_state) = req.app_data::<actix_web::web::Data<AppState>>() {
                            match JwtAuth::verify_token(token, &app_state.jwt_secret) {
                                Ok(claims) => {
                                    debug!("JWT authentication successful for user: {}", claims.sub);
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
                }
            }

            // Try API key authentication
            if let Some(api_key_header) = req.headers().get("x-api-key") {
                if let Ok(api_key_str) = api_key_header.to_str() {
                    if let Some(app_state) = req.app_data::<actix_web::web::Data<AppState>>() {
                        let api_key_repo = ApiKeyRepository::new(app_state.db.pool());
                        let user_repo = UserRepository::new(app_state.db.pool());
                        
                        match api_key_repo.find_by_hash(api_key_str).await {
                            Ok(Some(api_key)) => {
                                if api_key.is_active {
                                    match user_repo.find_by_id(&api_key.user_id).await {
                                        Ok(Some(user)) => {
                                            if user.is_active {
                                                debug!("API key authentication successful for user: {}", user.id);
                                                req.extensions_mut().insert(AuthUser::from_api_key(user));
                                                return service.call(req).await;
                                            }
                                        }
                                        Ok(None) => {
                                            warn!("API key references non-existent user: {}", api_key.user_id);
                                        }
                                        Err(e) => {
                                            warn!("Database error looking up user: {}", e);
                                        }
                                    }
                                } else {
                                    warn!("Inactive API key used: {}", api_key.id);
                                }
                            }
                            Ok(None) => {
                                warn!("Unknown API key used");
                            }
                            Err(e) => {
                                warn!("Database error looking up API key: {}", e);
                            }
                        }
                    }
                }
            }

            // No valid authentication found
            Err(ErrorUnauthorized("Authentication required"))
        })
    }
}

/// Authenticated user information
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    pub role: String,
    pub auth_type: AuthType,
}

/// Type of authentication used
#[derive(Debug, Clone)]
pub enum AuthType {
    Jwt,
    ApiKey,
}

impl AuthUser {
    fn from_jwt(claims: Claims) -> Self {
        Self {
            id: claims.sub,
            email: claims.email,
            role: claims.role,
            auth_type: AuthType::Jwt,
        }
    }
    
    fn from_api_key(user: gl_db::User) -> Self {
        Self {
            id: user.id,
            email: user.email,
            role: user.role,
            auth_type: AuthType::ApiKey,
        }
    }
}

/// Helper function to extract authenticated user from HTTP request
pub fn get_http_auth_user(req: &actix_web::HttpRequest) -> Option<AuthUser> {
    req.extensions().get::<AuthUser>().cloned()
}