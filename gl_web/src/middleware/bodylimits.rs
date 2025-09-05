//! ABOUTME: Body size limit middleware with per-endpoint overrides
//! ABOUTME: Prevents oversized payloads and returns RFC 7807 error responses

use crate::models::ProblemDetails;
use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, HttpResponse,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::collections::HashMap;
use std::rc::Rc;
use tracing::{debug, warn};

/// Body size limits configuration
#[derive(Debug, Clone)]
pub struct BodyLimitsConfig {
    /// Default JSON body size limit in bytes
    pub default_json_limit: usize,
    /// Per-endpoint override limits (path -> limit)
    pub endpoint_overrides: HashMap<String, usize>,
}

impl Default for BodyLimitsConfig {
    fn default() -> Self {
        Self {
            default_json_limit: 1048576, // 1MB
            endpoint_overrides: HashMap::new(),
        }
    }
}

impl BodyLimitsConfig {
    /// Create new config with default limit
    pub fn new(default_json_limit: usize) -> Self {
        Self {
            default_json_limit,
            endpoint_overrides: HashMap::new(),
        }
    }

    /// Add an endpoint override
    pub fn with_override(mut self, path: impl Into<String>, limit: usize) -> Self {
        self.endpoint_overrides.insert(path.into(), limit);
        self
    }

    /// Get the limit for a specific path
    pub fn get_limit_for_path(&self, path: &str) -> usize {
        // Check for exact match first
        if let Some(&limit) = self.endpoint_overrides.get(path) {
            return limit;
        }

        // Check for prefix matches (for dynamic routes)
        for (pattern, &limit) in &self.endpoint_overrides {
            if path.starts_with(pattern) {
                return limit;
            }
        }

        self.default_json_limit
    }
}

/// Body size limit middleware transform
pub struct BodyLimits {
    config: BodyLimitsConfig,
}

impl BodyLimits {
    pub fn new(config: BodyLimitsConfig) -> Self {
        Self { config }
    }
}

impl<S, B> Transform<S, ServiceRequest> for BodyLimits
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = BodyLimitsMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(BodyLimitsMiddleware {
            service: Rc::new(service),
            config: self.config.clone(),
        }))
    }
}

pub struct BodyLimitsMiddleware<S> {
    service: Rc<S>,
    config: BodyLimitsConfig,
}

impl<S, B> Service<ServiceRequest> for BodyLimitsMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let config = self.config.clone();

        Box::pin(async move {
            let path = req.path();
            let content_length = req
                .headers()
                .get("content-length")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<usize>().ok());

            // Only check content-length for JSON requests
            let is_json_request = req
                .headers()
                .get("content-type")
                .and_then(|h| h.to_str().ok())
                .map(|ct| ct.starts_with("application/json"))
                .unwrap_or(false);

            if is_json_request {
                let limit = config.get_limit_for_path(path);

                if let Some(content_len) = content_length {
                    debug!(
                        "Body size check: path={}, content_length={}, limit={}",
                        path, content_len, limit
                    );

                    if content_len > limit {
                        warn!(
                            "Body size limit exceeded: path={}, size={}, limit={}",
                            path, content_len, limit
                        );

                        let problem = ProblemDetails::payload_too_large_error(limit as u64)
                            .with_extension("path", serde_json::Value::String(path.to_string()))
                            .with_extension(
                                "received_size",
                                serde_json::Value::Number(content_len.into()),
                            );

                        let response = HttpResponse::PayloadTooLarge()
                            .content_type("application/problem+json")
                            .json(problem);

                        let (req, _) = req.into_parts();
                        return Ok(ServiceResponse::new(req, response).map_into_right_body());
                    }
                } else {
                    debug!("No content-length header for JSON request: {}", path);
                }
            }

            // Call the next service in the chain
            let res = service.call(req).await?;
            Ok(res.map_into_left_body())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_body_limits_config() {
        let config = BodyLimitsConfig::new(1000)
            .with_override("/api/admin", 10000)
            .with_override("/api/upload", 100000);

        assert_eq!(config.get_limit_for_path("/api/auth"), 1000);
        assert_eq!(config.get_limit_for_path("/api/admin"), 10000);
        assert_eq!(config.get_limit_for_path("/api/settings/streams"), 1000); // Uses default, not /api/admin override
        assert_eq!(config.get_limit_for_path("/api/upload"), 100000);
        assert_eq!(config.get_limit_for_path("/api/upload/files"), 100000);
        assert_eq!(config.get_limit_for_path("/unknown"), 1000);
    }
}
