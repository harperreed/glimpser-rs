//! ABOUTME: Observability services including health checks and metrics
//! ABOUTME: Provides monitoring endpoints for operational visibility

use actix_web::{
    dev::{ServiceRequest, ServiceResponse},
    middleware::Logger,
    web, App, HttpResponse, HttpServer, Result as ActixResult,
};
use gl_core::Result;
use prometheus_client::{
    encoding::text::encode,
    metrics::{counter::Counter, histogram::Histogram},
    registry::Registry,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// Readiness gate that can be toggled to indicate service readiness
#[derive(Debug, Clone)]
pub struct ReadinessGate {
    ready: Arc<AtomicBool>,
}

impl ReadinessGate {
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::Relaxed);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }
}

impl Default for ReadinessGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics registry for Prometheus
#[derive(Debug)]
pub struct Metrics {
    registry: Arc<Mutex<Registry>>,
    http_requests_total: Counter,
    http_request_duration_seconds: Histogram,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let http_requests_total = Counter::default();
        registry.register(
            "http_requests_total",
            "Total number of HTTP requests",
            http_requests_total.clone(),
        );

        let http_request_duration_seconds =
            Histogram::new([0.1, 0.5, 1.0, 2.5, 5.0, 10.0].into_iter());
        registry.register(
            "http_request_duration_seconds",
            "HTTP request duration in seconds",
            http_request_duration_seconds.clone(),
        );

        Self {
            registry: Arc::new(Mutex::new(registry)),
            http_requests_total,
            http_request_duration_seconds,
        }
    }

    pub fn inc_requests(&self) {
        self.http_requests_total.inc();
    }

    pub fn observe_duration(&self, duration: f64) {
        self.http_request_duration_seconds.observe(duration);
    }

    pub fn encode(&self) -> Result<String> {
        let registry = self.registry.lock().map_err(|e| {
            gl_core::Error::Config(format!("Failed to lock metrics registry: {}", e))
        })?;

        let mut buffer = String::new();
        encode(&mut buffer, &registry)
            .map_err(|e| gl_core::Error::Config(format!("Failed to encode metrics: {}", e)))?;

        Ok(buffer)
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Application state for observability endpoints
#[derive(Debug, Clone)]
pub struct ObsState {
    pub readiness: ReadinessGate,
    pub metrics: Arc<Metrics>,
}

impl ObsState {
    pub fn new() -> Self {
        Self {
            readiness: ReadinessGate::new(),
            metrics: Arc::new(Metrics::new()),
        }
    }
}

impl Default for ObsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Health endpoint handler
async fn health() -> ActixResult<HttpResponse> {
    tracing::info!("Health check requested");
    Ok(HttpResponse::Ok().json(json!({
        "status": "ok"
    })))
}

/// Readiness endpoint handler
async fn readiness(state: web::Data<ObsState>) -> ActixResult<HttpResponse> {
    let is_ready = state.readiness.is_ready();
    tracing::info!("Readiness check requested, ready: {}", is_ready);

    if is_ready {
        Ok(HttpResponse::Ok().json(json!({
            "status": "ready"
        })))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(json!({
            "status": "not ready"
        })))
    }
}

/// Metrics endpoint handler
async fn metrics(state: web::Data<ObsState>) -> ActixResult<HttpResponse> {
    tracing::debug!("Metrics scrape requested");

    // Don't increment metrics counter for metrics endpoint to avoid feedback loop

    match state.metrics.encode() {
        Ok(metrics_text) => {
            tracing::debug!("Metrics encoded successfully, {} bytes", metrics_text.len());
            Ok(HttpResponse::Ok()
                .content_type("text/plain; version=0.0.4; charset=utf-8")
                .body(metrics_text))
        }
        Err(e) => {
            tracing::error!("Failed to encode metrics: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to encode metrics"
            })))
        }
    }
}

/// Create observability service factory
pub fn create_service(
    state: ObsState,
) -> App<
    impl actix_web::dev::ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(web::Data::new(state))
        .wrap(Logger::default())
        .service(
            web::scope("")
                .route("/healthz", web::get().to(health))
                .route("/readyz", web::get().to(readiness))
                .route("/metrics", web::get().to(metrics)),
        )
}

/// Start observability server
pub async fn start_server(bind_addr: &str, state: ObsState) -> Result<()> {
    tracing::info!("Starting observability server on {}", bind_addr);

    HttpServer::new(move || create_service(state.clone()))
        .bind(bind_addr)
        .map_err(|e| gl_core::Error::Config(format!("Failed to bind server: {}", e)))?
        .run()
        .await
        .map_err(|e| gl_core::Error::Config(format!("Server error: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test;

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = ObsState::new();
        let app = test::init_service(create_service(state)).await;

        let req = test::TestRequest::get().uri("/healthz").to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn test_readiness_endpoint_ready() {
        let state = ObsState::new();
        state.readiness.set_ready(true);

        let app = test::init_service(create_service(state)).await;

        let req = test::TestRequest::get().uri("/readyz").to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "ready");
    }

    #[tokio::test]
    async fn test_readiness_endpoint_not_ready() {
        let state = ObsState::new();
        state.readiness.set_ready(false);

        let app = test::init_service(create_service(state)).await;

        let req = test::TestRequest::get().uri("/readyz").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 503);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );

        let body: serde_json::Value = test::read_body_json(resp).await;
        assert_eq!(body["status"], "not ready");
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let state = ObsState::new();

        // Record some metrics (but not from the metrics endpoint itself)
        state.metrics.inc_requests();
        state.metrics.observe_duration(0.5);

        let app = test::init_service(create_service(state)).await;

        let req = test::TestRequest::get().uri("/metrics").to_request();
        let resp = test::call_service(&app, req).await;

        assert!(resp.status().is_success());
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; version=0.0.4; charset=utf-8"
        );

        let body = test::read_body(resp).await;
        let body_str = std::str::from_utf8(&body).unwrap();

        // Verify that metrics are present
        assert!(body_str.contains("http_requests_total"));
        assert!(body_str.contains("http_request_duration_seconds"));
    }

    #[tokio::test]
    async fn test_readiness_gate_toggle() {
        let gate = ReadinessGate::new();

        // Should start ready
        assert!(gate.is_ready());

        // Set not ready
        gate.set_ready(false);
        assert!(!gate.is_ready());

        // Set ready again
        gate.set_ready(true);
        assert!(gate.is_ready());
    }

    #[tokio::test]
    async fn test_metrics_functionality() {
        let metrics = Metrics::new();

        // Increment requests
        metrics.inc_requests();
        metrics.inc_requests();

        // Observe durations
        metrics.observe_duration(0.1);
        metrics.observe_duration(1.5);

        // Encode metrics
        let encoded = metrics.encode().expect("Should encode successfully");

        // Verify content
        assert!(encoded.contains("http_requests_total"));
        assert!(encoded.contains("http_request_duration_seconds"));
        assert!(encoded.contains("2")); // Should have 2 requests
    }
}
