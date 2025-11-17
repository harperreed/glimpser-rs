//! ABOUTME: Web API layer with authentication and routing
//! ABOUTME: Provides REST endpoints and OpenAPI documentation

use actix_web::HttpServer;
use gl_ai::AiClient;
use gl_config::SecurityConfig;
use gl_core::Result;
use gl_db::{DatabaseCache, Db};
use gl_scheduler::JobScheduler;
use gl_stream::StreamManager;
use gl_update::UpdateService;

use background_snapshot_service::BackgroundSnapshotService;

pub mod auth;
pub mod background_snapshot_service;
pub mod capture_manager;
pub mod error;
pub mod frontend;
pub mod hybrid_server;
pub mod middleware;
pub mod models;

/// Route handler implementations
///
/// The routes/ directory contains all HTTP handler functions that process
/// requests and generate responses. Each module typically exports handler
/// functions that are referenced by the routing configuration.
pub mod routes;

/// Route configuration and middleware wiring
///
/// The routing/ directory contains functions that configure Actix-web routes,
/// map URL paths to handlers, and apply middleware. This separation keeps
/// handler logic (routes/) distinct from routing configuration (routing/).
///
/// Architecture:
/// - routes/ = handler implementations (the "what")
/// - routing/ = route configuration (the "how" and "where")
pub mod routing;

#[cfg(test)]
mod tests;

use routes::static_files;
use std::sync::Arc;

/// Application state shared across all handlers
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub cache: Arc<DatabaseCache>,
    pub security_config: SecurityConfig,
    pub static_config: static_files::StaticConfig,
    pub rate_limit_config: middleware::ratelimit::RateLimitConfig,
    pub body_limits_config: middleware::bodylimits::BodyLimitsConfig,
    pub capture_manager: Arc<capture_manager::CaptureManager>,
    pub stream_manager: Arc<StreamManager>,
    pub update_service: Arc<tokio::sync::Mutex<UpdateService>>,
    pub ai_client: Arc<dyn AiClient>,
    pub job_scheduler: Arc<JobScheduler>,
    pub background_snapshot_service: Arc<BackgroundSnapshotService>,
}

// Re-export the create_app function from routing module for backward compatibility
pub use routing::create_app;

/// Start the web server (Actix-web only - legacy)
pub async fn start_server(bind_addr: &str, state: AppState) -> Result<()> {
    tracing::info!("Starting web server on {}", bind_addr);

    HttpServer::new(move || create_app(state.clone()))
        .bind(bind_addr)
        .map_err(|e| gl_core::Error::Config(format!("Failed to bind web server: {}", e)))?
        .run()
        .await
        .map_err(|e| gl_core::Error::Config(format!("Web server error: {}", e)))?;

    Ok(())
}

/// Start the hybrid server (Axum frontend + Actix API)
pub async fn start_hybrid_server(bind_addr: &str, state: AppState) -> Result<()> {
    hybrid_server::start_hybrid_server(bind_addr, state).await
}
