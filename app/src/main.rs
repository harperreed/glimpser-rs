use gl_config::Config;
use gl_core::telemetry;
use gl_obs::ObsState;
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

    // Initialize observability state
    let obs_state = ObsState::new();
    
    // Start observability server
    let obs_bind_addr = format!("0.0.0.0:{}", config.server.obs_port);
    tracing::info!("Starting observability server on {}", obs_bind_addr);
    
    // For this prompt, the observability server IS the main application
    // Future prompts will add the business logic server alongside this
    if let Err(e) = gl_obs::start_server(&obs_bind_addr, obs_state).await {
        tracing::error!("Failed to start observability server: {}", e);
        process::exit(1);
    }
}
