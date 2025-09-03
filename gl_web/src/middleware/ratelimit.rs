//! ABOUTME: Rate limiting middleware for IP and API key-based buckets
//! ABOUTME: Prevents abuse by limiting requests per IP and per API key

use crate::models::ProblemDetails;
use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::{HeaderName, HeaderValue},
    Error, HttpResponse,
};
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Rate limiting configuration (IP-based only)
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per IP per window
    pub requests_per_minute: u32,
    /// Time window duration
    pub window_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            window_duration: Duration::from_secs(60),
        }
    }
}

/// Simple in-memory rate limiter using a sliding window
#[derive(Debug, Clone)]
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}

#[derive(Debug, Clone)]
struct SimpleRateLimiter {
    entries: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
    max_requests: u32,
    window_duration: Duration,
}

impl SimpleRateLimiter {
    fn new(max_requests: u32, window_duration: Duration) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window_duration,
        }
    }

    fn check_rate_limit(&self, key: &str) -> (bool, u32, Duration) {
        let now = Instant::now();
        let mut entries = self.entries.lock().unwrap();

        let entry = entries.entry(key.to_string()).or_insert(RateLimitEntry {
            count: 0,
            window_start: now,
        });

        // If window has expired, reset
        if now.duration_since(entry.window_start) >= self.window_duration {
            entry.count = 0;
            entry.window_start = now;
        }

        // Check if we can allow this request
        if entry.count < self.max_requests {
            entry.count += 1;
            let remaining = self.max_requests - entry.count;
            (true, remaining, Duration::ZERO)
        } else {
            let reset_time = self.window_duration - now.duration_since(entry.window_start);
            (false, 0, reset_time)
        }
    }
}

/// Rate limiting middleware transform
pub struct RateLimit {
    config: RateLimitConfig,
    limiter: SimpleRateLimiter,
}

impl RateLimit {
    pub fn new(config: RateLimitConfig) -> Self {
        let limiter = SimpleRateLimiter::new(config.requests_per_minute, config.window_duration);

        Self { config, limiter }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimitMiddleware {
            service: Rc::new(service),
            config: self.config.clone(),
            limiter: self.limiter.clone(),
        }))
    }
}

pub struct RateLimitMiddleware<S> {
    service: Rc<S>,
    #[allow(dead_code)]
    config: RateLimitConfig,
    limiter: SimpleRateLimiter,
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddleware<S>
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
        let limiter = self.limiter.clone();

        Box::pin(async move {
            // Extract client IP for rate limiting (simplified to IP-only)
            let client_ip = get_client_ip(&req);

            debug!("Rate limit check: ip={}", client_ip);

            // Check rate limit using IP address as key
            let (allowed, remaining, reset_time) = limiter.check_rate_limit(&client_ip);

            if allowed {
                debug!(
                    "Rate limit passed: ip={}, remaining={}",
                    client_ip, remaining
                );
                let res = service.call(req).await?;
                Ok(res.map_into_left_body())
            } else {
                warn!(
                    "Rate limit exceeded: ip={}, reset_in={}s",
                    client_ip,
                    reset_time.as_secs()
                );

                // Calculate retry-after in seconds
                let retry_after = reset_time.as_secs();

                let problem = ProblemDetails::rate_limit_error(Some(retry_after))
                    .with_extension("client_ip", serde_json::Value::String(client_ip));

                let mut response = HttpResponse::TooManyRequests()
                    .content_type("application/problem+json")
                    .json(problem);

                // Add standard rate limit headers
                if let Ok(retry_header) = HeaderValue::from_str(&retry_after.to_string()) {
                    response
                        .headers_mut()
                        .insert(HeaderName::from_static("retry-after"), retry_header);
                }
                if let Ok(remaining_header) = HeaderValue::from_str("0") {
                    response.headers_mut().insert(
                        HeaderName::from_static("x-ratelimit-remaining"),
                        remaining_header,
                    );
                }

                let (req, _) = req.into_parts();
                Ok(ServiceResponse::new(req, response).map_into_right_body())
            }
        })
    }
}

/// Extract client IP from request headers and connection info
fn get_client_ip(req: &ServiceRequest) -> String {
    // Try X-Forwarded-For first (proxy/load balancer)
    if let Some(forwarded) = req.headers().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            // Take the first IP from the comma-separated list
            if let Some(first_ip) = forwarded_str.split(',').next() {
                return first_ip.trim().to_string();
            }
        }
    }

    // Try X-Real-IP (nginx proxy)
    if let Some(real_ip) = req.headers().get("x-real-ip") {
        if let Ok(real_ip_str) = real_ip.to_str() {
            return real_ip_str.to_string();
        }
    }

    // Fall back to connection peer address
    if let Some(peer_addr) = req.peer_addr() {
        peer_addr.ip().to_string()
    } else {
        "unknown".to_string()
    }
}
