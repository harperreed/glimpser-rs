//! ABOUTME: Rate limiting middleware for IP and API key-based buckets
//! ABOUTME: Prevents abuse by limiting requests per IP and per API key

use crate::models::ProblemDetails;
use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::{HeaderName, HeaderValue},
    Error, HttpResponse,
};
use dashmap::DashMap;
use futures_util::future::{ready, LocalBoxFuture, Ready};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Rate limiting configuration (IP-based only)
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per IP per window
    pub requests_per_minute: u32,
    /// Time window duration
    pub window_duration: Duration,
    /// Trusted proxy IP addresses that can set X-Forwarded-For headers
    pub trusted_proxies: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            window_duration: Duration::from_secs(60),
            trusted_proxies: vec!["127.0.0.1".to_string(), "::1".to_string()], // Localhost by default
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
    entries: Arc<DashMap<String, RateLimitEntry>>,
    max_requests: u32,
    window_duration: Duration,
}

impl SimpleRateLimiter {
    fn new(max_requests: u32, window_duration: Duration) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            max_requests,
            window_duration,
        }
    }

    fn check_rate_limit(&self, key: &str) -> (bool, u32, Duration) {
        let now = Instant::now();

        // Use DashMap's entry API for lock-free operations
        let mut entry = self
            .entries
            .entry(key.to_string())
            .or_insert(RateLimitEntry {
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
        let config = self.config.clone();

        Box::pin(async move {
            // Extract client IP for rate limiting with trusted proxy validation
            let client_ip = get_client_ip(&req, &config.trusted_proxies);

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

/// Extract client IP from request headers and connection info with trusted proxy validation
fn get_client_ip(req: &ServiceRequest, trusted_proxies: &[String]) -> String {
    // Get the actual connection peer address first
    let peer_ip = req
        .peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Only trust proxy headers if the request comes from a trusted proxy
    let is_trusted_proxy = trusted_proxies.iter().any(|trusted| trusted == &peer_ip);

    if is_trusted_proxy {
        // Try X-Forwarded-For first (proxy/load balancer)
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(forwarded_str) = forwarded.to_str() {
                // Take the first IP from the comma-separated list
                if let Some(first_ip) = forwarded_str.split(',').next() {
                    let trimmed_ip = first_ip.trim();
                    if !trimmed_ip.is_empty() {
                        debug!(
                            "Using X-Forwarded-For IP: {} from trusted proxy: {}",
                            trimmed_ip, peer_ip
                        );
                        return trimmed_ip.to_string();
                    }
                }
            }
        }

        // Try X-Real-IP (nginx proxy)
        if let Some(real_ip) = req.headers().get("x-real-ip") {
            if let Ok(real_ip_str) = real_ip.to_str() {
                if !real_ip_str.is_empty() {
                    debug!(
                        "Using X-Real-IP: {} from trusted proxy: {}",
                        real_ip_str, peer_ip
                    );
                    return real_ip_str.to_string();
                }
            }
        }
    } else if !trusted_proxies.is_empty() {
        // Log when proxy headers are ignored from untrusted sources
        if req.headers().get("x-forwarded-for").is_some()
            || req.headers().get("x-real-ip").is_some()
        {
            warn!("Ignoring proxy headers from untrusted IP: {}", peer_ip);
        }
    }

    // Fall back to connection peer address
    debug!("Using peer IP: {}", peer_ip);
    peer_ip
}
