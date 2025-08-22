//! ABOUTME: Role-based access control middleware for authorization
//! ABOUTME: Enforces role-based permissions on protected endpoints

use crate::{middleware::auth::AuthUser, models::Role};
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error::{ErrorForbidden, ErrorUnauthorized},
    Error, HttpMessage,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::rc::Rc;
use tracing::{debug, warn};

/// RBAC middleware that requires specific roles
pub struct RequireRole {
    required_roles: Vec<Role>,
}

impl RequireRole {
    pub fn new(roles: Vec<Role>) -> Self {
        Self {
            required_roles: roles,
        }
    }

    /// Require admin role
    pub fn admin() -> Self {
        Self::new(vec![Role::Admin])
    }

    /// Require admin or operator role
    pub fn operator() -> Self {
        Self::new(vec![Role::Admin, Role::Operator])
    }

    /// Require any authenticated role (admin, operator, or viewer)
    pub fn viewer() -> Self {
        Self::new(vec![Role::Admin, Role::Operator, Role::Viewer])
    }
}

impl<S, B> Transform<S, ServiceRequest> for RequireRole
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = RequireRoleMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequireRoleMiddleware {
            service: Rc::new(service),
            required_roles: self.required_roles.clone(),
        }))
    }
}

pub struct RequireRoleMiddleware<S> {
    service: Rc<S>,
    required_roles: Vec<Role>,
}

impl<S, B> Service<ServiceRequest> for RequireRoleMiddleware<S>
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
        let required_roles = self.required_roles.clone();

        Box::pin(async move {
            // Get authenticated user from request extensions (scope the borrow)
            let auth_user = {
                let extensions = req.extensions();
                extensions.get::<AuthUser>().cloned()
            };

            match auth_user {
                Some(auth_user) => {
                    // Parse user's role
                    if let Some(user_role) = Role::from_str(&auth_user.role) {
                        // Check if user has any of the required roles
                        if required_roles.contains(&user_role) {
                            debug!(
                                "RBAC check passed for user {} with role {}",
                                auth_user.id, auth_user.role
                            );
                            service.call(req).await
                        } else {
                            warn!(
                                "RBAC check failed for user {} with role {}. Required roles: {:?}",
                                auth_user.id, auth_user.role, required_roles
                            );
                            return Err(ErrorForbidden("Insufficient permissions"));
                        }
                    } else {
                        warn!("Invalid user role: {}", auth_user.role);
                        return Err(ErrorForbidden("Invalid user role"));
                    }
                }
                None => {
                    warn!("RBAC middleware called without authenticated user");
                    Err(ErrorUnauthorized("Authentication required"))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_requirements() {
        let admin_only = RequireRole::admin();
        assert_eq!(admin_only.required_roles, vec![Role::Admin]);

        let operator_plus = RequireRole::operator();
        assert_eq!(
            operator_plus.required_roles,
            vec![Role::Admin, Role::Operator]
        );

        let viewer_plus = RequireRole::viewer();
        assert_eq!(
            viewer_plus.required_roles,
            vec![Role::Admin, Role::Operator, Role::Viewer]
        );
    }
}
