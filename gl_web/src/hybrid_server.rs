//! ABOUTME: Hybrid server combining Axum frontend and Actix-web API
//! ABOUTME: Routes frontend requests to Axum and API requests to Actix-web

use crate::{frontend, AppState};
use axum::{
    body::Body,
    http::{StatusCode, Uri},
    response::IntoResponse,
    routing::any,
    Router,
};
use gl_core::Result;

/// Start the hybrid server (Axum only for now)
pub async fn start_hybrid_server(bind_addr: &str, state: AppState) -> Result<()> {
    tracing::info!("Starting Axum server on {}", bind_addr);

    // Create the Axum frontend router
    let frontend_state = frontend::FrontendState::from(state.clone());
    let frontend_router = frontend::create_frontend_router().with_state(frontend_state);

    // Create the main router that handles both frontend and API routes
    let app = Router::new()
        // Static files
        .route("/static/*path", any(static_handler))
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

/// Handler for static files
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path();
    tracing::info!("Static file request: {}", path);

    // Serve actual static files
    match path {
        "/static/stream-form.js" => {
            // Read the actual JavaScript file
            match std::fs::read_to_string("gl_web/static/stream-form.js") {
                Ok(content) => axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/javascript")
                    .body(Body::from(content))
                    .unwrap(),
                Err(_) => axum::response::Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("JavaScript file not found"))
                    .unwrap(),
            }
        }
        "/static/app.css" => axum::response::Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/css")
            .body(Body::from("/* Custom CSS would be served here */"))
            .unwrap(),
        _ => axum::response::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(format!("Static file not found: {}", path)))
            .unwrap(),
    }
}
