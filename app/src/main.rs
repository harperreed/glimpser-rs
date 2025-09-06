use clap::{Parser, Subcommand};
use gl_config::Config;
use gl_core::telemetry;
use gl_db::{CreateStreamRequest, Db, StreamRepository, UserRepository};
use gl_obs::ObsState;
use gl_stream::{StreamManager, StreamMetrics};
use gl_web::AppState;
use std::process;

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

    match cli.command.unwrap_or(Commands::Start) {
        Commands::Bootstrap => {
            interactive_bootstrap(&db).await;
            return;
        }
        Commands::Start => {
            tracing::info!("glimpser starting");
            start_server(config, db).await;
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
    println!("   • Access the web interface at http://127.0.0.1:8080/static/");
    println!("   • Use the example streams for testing");
    println!("   • Take snapshots via API: /api/stream/<stream_id>/snapshot");
}

async fn start_server(config: Config, db: Db) {
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

    // Initialize capture manager with configured storage
    let capture_manager = std::sync::Arc::new(
        gl_web::capture_manager::CaptureManager::with_storage_config(
            db.pool().clone(),
            config.storage.clone(),
        ),
    );

    // Initialize stream manager for MJPEG streaming
    let stream_metrics = StreamMetrics::new();
    let stream_manager = std::sync::Arc::new(StreamManager::new(stream_metrics));

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
        capture_manager,
        stream_manager,
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
