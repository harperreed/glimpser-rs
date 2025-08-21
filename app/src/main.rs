use gl_config::Config;
use gl_core::telemetry;
use std::process;

fn main() {
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
        db_path = %config.database.path,
        "Application configured and ready"
    );
    
    println!("Hello, world!");
}
