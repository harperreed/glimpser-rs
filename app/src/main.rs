use gl_config::Config;
use gl_core::telemetry;
use gl_db::Db;
use gl_obs::ObsState;
use gl_web::AppState;
use std::process;

#[tokio::main]
async fn main() {
    telemetry::init_tracing("development", "glimpser");
    tracing::info!("glimpser starting");

    // Load configuration - exit with non-zero if invalid
    let config = match Config::load() {
        Ok(config) => {
            tracing::debug!(?config, "Configuration loaded successfully");
            config
        }
        Err(e) => {
            tracing::error!("Failed to load configuration: {}", e);
            process::exit(1);
        }
    };

    tracing::info!(
        host = %config.server.host,
        port = %config.server.port,
        obs_port = %config.server.obs_port,
        db_path = %config.database.path,
        "Application configured and ready"
    );

    // Initialize database with migrations
    let db = match Db::new(&config.database.path).await {
        Ok(db) => {
            tracing::info!("Database initialized successfully");
            db
        }
        Err(e) => {
            tracing::error!("Failed to initialize database: {}", e);
            process::exit(1);
        }
    };

    // Verify database health
    if let Err(e) = db.health_check().await {
        tracing::error!("Database health check failed: {}", e);
        process::exit(1);
    }

    // Initialize observability state
    let obs_state = ObsState::new();
    
    // Initialize web application state
    let web_app_state = AppState {
        db: db.clone(),
        jwt_secret: config.security.jwt_secret.clone(),
    };
    
    // Start observability server
    let obs_bind_addr = format!("0.0.0.0:{}", config.server.obs_port);
    tracing::info!("Starting observability server on {}", obs_bind_addr);
    
    // Start web server
    let web_bind_addr = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!("Starting web server on {}", web_bind_addr);
    
    // Run both servers concurrently
    let obs_future = gl_obs::start_server(&obs_bind_addr, obs_state);
    let web_future = gl_web::start_server(&web_bind_addr, web_app_state);
    
    // Use select to run both concurrently - either succeeding means the app runs
    let result = tokio::select! {
        obs_result = obs_future => {
            tracing::error!("Observability server exited");
            obs_result
        }
        web_result = web_future => {
            tracing::error!("Web server exited");
            web_result
        }
    };
    
    if let Err(e) = result {
        tracing::error!("Server error: {}", e);
        process::exit(1);
    }
}
