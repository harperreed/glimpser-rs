//! ABOUTME: Hybrid server combining Axum frontend and Actix-web API
//! ABOUTME: Routes frontend requests to Axum and API requests to Actix-web

use crate::{frontend, AppState};
use axum::Router;
use gl_core::Result;
use tower_http::services::ServeDir;

/// Start the hybrid server (Axum only for now)
pub async fn start_hybrid_server(bind_addr: &str, state: AppState) -> Result<()> {
    tracing::info!("Starting Axum server on {}", bind_addr);

    // Create the Axum frontend router
    let frontend_state = frontend::FrontendState::from(state.clone());
    let frontend_router = frontend::create_frontend_router().with_state(frontend_state);

    // Configure static file serving with proper caching headers and compression
    let static_service = ServeDir::new(&state.static_config.static_dir)
        .append_index_html_on_directories(false)
        .precompressed_gzip()
        .precompressed_br();

    // Create the main router that handles both frontend and API routes
    let app = Router::new()
        // Static files with proper async serving and caching
        .nest_service("/static", static_service)
        // All other routes go to frontend (including API routes)
        .merge(frontend_router)
        .with_state(state);

    // Create TCP listener
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| gl_core::Error::Config(format!("Failed to bind to {}: {}", bind_addr, e)))?;

    tracing::info!("Axum server listening on {}", bind_addr);

    // Start the server
    axum::serve(listener, app)
        .await
        .map_err(|e| gl_core::Error::Config(format!("Server error: {}", e)))?;

    Ok(())
}

// All API routes now handled by the frontend router
