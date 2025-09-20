use clap::{Parser, Subcommand};
use gl_ai::{create_client, AiConfig};
use gl_config::Config;
use gl_core::telemetry;
use gl_db::{CreateStreamRequest, Db, StreamRepository, UserRepository};
use gl_obs::ObsState;
use gl_scheduler::{create_standard_handlers, JobScheduler, SchedulerConfig, SqliteJobStorage};
use gl_stream::{StreamManager, StreamMetrics};
use gl_update::{UpdateConfig, UpdateService, UpdateStrategyType};
use gl_web::AppState;
use std::{process, sync::Arc};

#[derive(Parser)]
#[command(name = "glimpser")]
#[command(about = "Glimpser surveillance platform")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Bootstrap initial admin user (interactive)
    Bootstrap,
    /// Start the server (default)
    Start,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    telemetry::init_tracing("development", "glimpser");

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

    // Ensure required directories exist
    let artifacts_dir = &config.storage.artifacts_dir;
    if let Err(e) = std::fs::create_dir_all(artifacts_dir) {
        tracing::error!(
            "Failed to create artifacts directory '{}': {}",
            artifacts_dir,
            e
        );
        process::exit(1);
    }
    tracing::info!("Storage directory ready: {}", artifacts_dir);

    match cli.command.unwrap_or(Commands::Start) {
        Commands::Bootstrap => {
            interactive_bootstrap(&db).await;
            return;
        }
        Commands::Start => {
            tracing::info!("glimpser starting");
            if let Err(e) = start_server(config, db).await {
                tracing::error!("Failed to start server: {}", e);
                process::exit(1);
            }
        }
    }
}

async fn interactive_bootstrap(db: &Db) {
    use std::io::{self, Write};

    println!();
    println!("🔍 Glimpser Admin User Bootstrap");
    println!("================================");
    println!();

    // Get email
    print!("Enter admin email address: ");
    io::stdout().flush().unwrap();
    let mut email = String::new();
    io::stdin().read_line(&mut email).unwrap();
    let email = email.trim();

    if email.is_empty() {
        eprintln!("❌ Email cannot be empty");
        process::exit(1);
    }

    // Validate email format
    if !email.contains('@') || !email.contains('.') {
        eprintln!("❌ Invalid email format");
        process::exit(1);
    }

    // Get username (optional)
    print!("Enter username (default: admin): ");
    io::stdout().flush().unwrap();
    let mut username = String::new();
    io::stdin().read_line(&mut username).unwrap();
    let username = username.trim();
    let username = if username.is_empty() {
        "admin"
    } else {
        username
    };

    // Get password securely
    let password = match rpassword::prompt_password("Enter admin password: ") {
        Ok(pass) => pass,
        Err(_) => {
            eprintln!("❌ Failed to read password");
            process::exit(1);
        }
    };

    if password.len() < 8 {
        eprintln!("❌ Password must be at least 8 characters long");
        process::exit(1);
    }

    // Confirm password
    let confirm_password = match rpassword::prompt_password("Confirm admin password: ") {
        Ok(pass) => pass,
        Err(_) => {
            eprintln!("❌ Failed to read password confirmation");
            process::exit(1);
        }
    };

    if password != confirm_password {
        eprintln!("❌ Passwords do not match");
        process::exit(1);
    }

    println!();
    println!("Creating admin user...");

    let user_id = bootstrap_user(db, email, &password, username).await;

    println!();
    println!("Creating example streams...");
    create_example_templates(db, &user_id).await;
}

async fn bootstrap_user(db: &Db, email: &str, password: &str, username: &str) -> String {
    use chrono::Utc;
    use gl_core::id::Id;
    use gl_web::auth::PasswordAuth;

    tracing::info!("Bootstrapping admin user: {}", email);

    let user_repo = UserRepository::new(db.pool());

    // Check if user already exists
    match user_repo.find_by_email(email).await {
        Ok(Some(existing_user)) => {
            tracing::warn!(
                "User with email {} already exists, skipping bootstrap",
                email
            );
            return existing_user.id;
        }
        Ok(None) => {
            tracing::info!("Creating new admin user");
        }
        Err(e) => {
            tracing::error!("Failed to check for existing user: {}", e);
            process::exit(1);
        }
    }

    // Hash the password
    let password_hash = match PasswordAuth::hash_password(password) {
        Ok(hash) => hash,
        Err(e) => {
            tracing::error!("Failed to hash password: {}", e);
            process::exit(1);
        }
    };

    // Create the user
    let user_id = Id::new().to_string();
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let create_result = sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, true, ?5, ?6)",
    )
    .bind(&user_id)
    .bind(username)
    .bind(email)
    .bind(&password_hash)
    .bind(&now)
    .bind(&now)
    .execute(db.pool())
    .await;

    match create_result {
        Ok(_) => {
            tracing::info!("✅ Admin user created successfully!");
            tracing::info!("   Email: {}", email);
            tracing::info!("   Username: {}", username);
            tracing::info!("You can now login to the web interface at http://localhost:3000");
            user_id
        }
        Err(e) => {
            tracing::error!("Failed to create user: {}", e);
            process::exit(1);
        }
    }
}

async fn create_example_templates(db: &Db, user_id: &str) {
    let template_repo = StreamRepository::new(db.pool());

    // Create Wrigleyville EarthCam template
    let webcam_config = serde_json::json!({
        "kind": "website",
        "url": "https://www.earthcam.com/usa/illinois/chicago/wrigleyville/?cam=wrigleyville",
        "headless": true,
        "stealth": true,
        "width": 1920,
        "height": 1080,
        "element_selector": ".cam-image"
    });

    let webcam_template = CreateStreamRequest {
        user_id: user_id.to_string(),
        name: "Wrigleyville EarthCam".to_string(),
        description: Some("Live webcam feed from Wrigleyville area in Chicago".to_string()),
        config: webcam_config.to_string(),
        is_default: false,
    };

    match template_repo.create(webcam_template).await {
        Ok(template) => {
            tracing::info!("✅ Created example webcam stream: {}", template.name);
            println!(
                "   📹 Webcam Stream: {} (ID: {})",
                template.name, template.id
            );
        }
        Err(e) => {
            tracing::warn!("Failed to create webcam stream: {}", e);
            println!("   ⚠️  Failed to create webcam stream");
        }
    }

    // Create NPR news site template
    let news_config = serde_json::json!({
        "kind": "website",
        "url": "https://www.npr.org",
        "headless": true,
        "stealth": true,
        "width": 1920,
        "height": 1080,
        "element_selector": "main"
    });

    let news_template = CreateStreamRequest {
        user_id: user_id.to_string(),
        name: "NPR News Site".to_string(),
        description: Some("NPR main page for news monitoring".to_string()),
        config: news_config.to_string(),
        is_default: false,
    };

    match template_repo.create(news_template).await {
        Ok(template) => {
            tracing::info!("✅ Created example news stream: {}", template.name);
            println!("   📰 News Stream: {} (ID: {})", template.name, template.id);
        }
        Err(e) => {
            tracing::warn!("Failed to create news stream: {}", e);
            println!("   ⚠️  Failed to create news stream");
        }
    }

    println!();
    println!("🎉 Bootstrap complete! You can now:");
    println!("   • Access the web interface at http://127.0.0.1:8185/static/");
    println!("   • Use the example streams for testing");
    println!("   • Take snapshots via API: /api/stream/<stream_id>/snapshot");
}

async fn start_server(config: Config, db: Db) -> gl_core::Result<()> {
    tracing::info!(
        host = %config.server.host,
        port = %config.server.port,
        obs_port = %config.server.obs_port,
        db_path = %config.database.path,
        "Application configured and ready"
    );

    // Initialize observability state
    let obs_state = ObsState::new();

    // Initialize web application state
    let static_config = gl_web::routes::static_files::StaticConfig {
        static_dir: config.server.static_dir.clone().into(),
        max_age: config.server.static_max_age,
        enable_csp: config.server.enable_csp,
        csp_nonce: if config.server.enable_csp {
            Some(gl_web::routes::static_files::generate_csp_nonce())
        } else {
            None
        },
    };

    // Initialize capture manager with analysis and storage configuration
    let capture_manager = gl_web::capture_manager::CaptureManager::with_analysis_config(
        db.pool().clone(),
        config.storage.clone(),
        &config,
    )?;

    // Initialize stream manager for MJPEG streaming
    let stream_metrics = StreamMetrics::new();
    let stream_manager = std::sync::Arc::new(StreamManager::new(stream_metrics));

    // Initialize AI client
    let ai_config = AiConfig {
        api_key: std::env::var("OPENAI_API_KEY").ok(),
        base_url: None,
        timeout_seconds: 30,
        max_retries: 3,
        model: std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4".to_string()),
        use_online: std::env::var("OPENAI_API_KEY").is_ok(),
    };

    tracing::info!(
        use_online = ai_config.use_online,
        model = %ai_config.model,
        "Initializing AI client"
    );

    let ai_client = {
        let client = create_client(ai_config);
        Arc::from(client)
    };

    // Initialize job scheduler
    let scheduler_config = SchedulerConfig {
        max_concurrent_jobs: 10,
        job_timeout_seconds: 300, // 5 minutes
        enable_persistence: true,
        history_retention_days: 30,
        enable_metrics: true,
    };

    // Need to create Arc for capture_manager temporarily for JobScheduler::new
    let capture_manager_arc = Arc::new(capture_manager);

    let job_storage = Arc::new(SqliteJobStorage::new(db.pool().clone()));
    let job_scheduler = Arc::new(
        JobScheduler::new(
            scheduler_config,
            job_storage,
            db.clone(),
            capture_manager_arc.clone(),
        )
        .await
        .map_err(|e| gl_core::Error::Config(format!("Failed to create job scheduler: {}", e)))?,
    );

    // Register standard job handlers
    let handlers = create_standard_handlers();
    for (job_type, handler) in handlers {
        job_scheduler.register_handler(job_type, handler).await;
    }

    // Start the job scheduler
    job_scheduler
        .start()
        .await
        .map_err(|e| gl_core::Error::Config(format!("Failed to start job scheduler: {}", e)))?;

    tracing::info!("Job scheduler initialized and started");

    // Connect job scheduler to capture manager for smart snapshots
    capture_manager_arc
        .set_job_scheduler(job_scheduler.clone())
        .await;

    let web_app_state = AppState {
        db: db.clone(),
        cache: std::sync::Arc::new(gl_db::DatabaseCache::new()),
        security_config: config.security.clone(),
        static_config,
        rate_limit_config: gl_web::middleware::ratelimit::RateLimitConfig {
            requests_per_minute: config.server.rate_limit.requests_per_minute,
            window_duration: std::time::Duration::from_secs(
                config.server.rate_limit.window_seconds,
            ),
        },
        body_limits_config: gl_web::middleware::bodylimits::BodyLimitsConfig::new(
            config.server.body_limits.global_json_limit,
        )
        .with_override("/api/upload", config.server.body_limits.upload_limit),
        capture_manager: capture_manager_arc,
        stream_manager,
        job_scheduler: job_scheduler.clone(),
        update_service: {
            // Create a basic update configuration
            // In production, these values would come from the main config
            let public_key = std::env::var("GLIMPSER_UPDATE_PUBLIC_KEY")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    // Development/test key - in production this should be from secure config
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
                });

            let install_dir = std::env::var("GLIMPSER_INSTALL_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    // Use a writable directory for development
                    std::env::temp_dir().join("glimpser-updates")
                });

            // Ensure install directory exists
            if let Err(e) = std::fs::create_dir_all(&install_dir) {
                tracing::warn!(
                    "Failed to create install directory {}: {}",
                    install_dir.display(),
                    e
                );
            }

            let update_config = UpdateConfig {
                repository: "owner/glimpser".to_string(), // This should be from config
                current_version: env!("CARGO_PKG_VERSION").to_string(),
                public_key,
                strategy: UpdateStrategyType::Sidecar,
                check_interval_seconds: 3600,
                health_check_timeout_seconds: 30,
                health_check_url: format!("http://127.0.0.1:{}/healthz", config.server.port),
                binary_name: "glimpser".to_string(),
                install_dir,
                auto_apply: false,
                github_token: None,
            };

            match UpdateService::new(update_config) {
                Ok(service) => {
                    tracing::info!("Update service initialized successfully");
                    std::sync::Arc::new(tokio::sync::Mutex::new(service))
                }
                Err(e) => {
                    tracing::error!("Failed to initialize update service: {}", e);
                    tracing::warn!(
                        "Update functionality will be disabled - creating no-op service"
                    );

                    // Create a minimal config that will work for a disabled service
                    let fallback_config = UpdateConfig {
                        repository: "disabled/disabled".to_string(),
                        current_version: "0.0.0".to_string(),
                        public_key:
                            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                                .to_string(),
                        strategy: UpdateStrategyType::Sidecar,
                        check_interval_seconds: 86400, // Check once a day but will be disabled
                        health_check_timeout_seconds: 30,
                        health_check_url: "http://127.0.0.1:1/disabled".to_string(),
                        binary_name: "disabled".to_string(),
                        install_dir: std::env::temp_dir(), // Use temp dir which should always exist
                        auto_apply: false,
                        github_token: None,
                    };

                    // Try with fallback config - this should work since we use temp_dir()
                    match UpdateService::new(fallback_config) {
                        Ok(service) => {
                            tracing::info!("Fallback update service created (disabled)");
                            std::sync::Arc::new(tokio::sync::Mutex::new(service))
                        }
                        Err(fallback_error) => {
                            tracing::error!(
                                "Even fallback update service failed: {}",
                                fallback_error
                            );
                            panic!("Cannot create update service: {}", fallback_error);
                        }
                    }
                }
            }
        },
        ai_client,
    };

    // Start observability server
    let obs_bind_addr = format!("0.0.0.0:{}", config.server.obs_port);
    tracing::info!("Starting observability server on {}", obs_bind_addr);

    // Start web server
    let web_bind_addr = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!("Starting web server on {}", web_bind_addr);

    // Run both servers concurrently
    let obs_future = gl_obs::start_server(&obs_bind_addr, obs_state);
    let web_future = gl_web::start_hybrid_server(&web_bind_addr, web_app_state);

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
        return Err(gl_core::Error::External(format!("Server error: {}", e)));
    }

    Ok(())
}
