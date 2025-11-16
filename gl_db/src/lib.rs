//! ABOUTME: Database layer with SQLite, migrations, and repositories
//! ABOUTME: Handles all data persistence and database operations

use gl_core::{Error, Result};
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, Sqlite, SqlitePool,
};
use std::time::Duration;
use tracing::{debug, info, warn, instrument};

/// Database connection retry configuration
#[derive(Debug, Clone)]
pub struct DatabaseRetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay between retries
    pub initial_delay_ms: u64,
    /// Maximum delay between retries
    pub max_delay_ms: u64,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
}

impl Default for DatabaseRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

impl DatabaseRetryConfig {
    /// Create a new retry configuration
    pub fn new(
        max_attempts: u32,
        initial_delay_ms: u64,
        max_delay_ms: u64,
        backoff_multiplier: f64,
    ) -> Self {
        Self {
            max_attempts,
            initial_delay_ms,
            max_delay_ms,
            backoff_multiplier,
        }
    }

    /// Calculate delay for a given attempt number with exponential backoff and jitter
    fn calculate_delay(&self, attempt: u32) -> Duration {
        // Calculate exponential backoff: initial_delay * multiplier^attempt
        let delay_ms = self.initial_delay_ms as f64
            * self.backoff_multiplier.powi(attempt as i32);

        // Cap at max_delay_ms
        let capped_delay = delay_ms.min(self.max_delay_ms as f64);

        // Add simple jitter based on current time to prevent thundering herd
        // Use nanoseconds to create variation (±10%)
        let jitter = {
            use std::time::SystemTime;
            let nanos = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos();
            // Convert nanos to a value between 0.9 and 1.1 (±10%)
            // nanos % 201 gives 0-200, divide by 1000 gives 0.0-0.2, add 0.9 gives 0.9-1.1
            0.9 + ((nanos % 201) as f64 / 1000.0)
        };

        let final_delay = (capped_delay * jitter) as u64;

        Duration::from_millis(final_delay)
    }
}

/// Database connection pool and operations
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Create a new database connection with migrations and default retry configuration
    #[instrument(skip(db_path))]
    pub async fn new(db_path: &str) -> Result<Self> {
        Self::new_with_retry(db_path, DatabaseRetryConfig::default()).await
    }

    /// Create a new database connection with migrations and custom retry configuration
    #[instrument(skip(db_path, retry_config))]
    pub async fn new_with_retry(
        db_path: &str,
        retry_config: DatabaseRetryConfig,
    ) -> Result<Self> {
        info!(
            "Initializing database at: {} (max_attempts: {}, initial_delay: {}ms)",
            db_path, retry_config.max_attempts, retry_config.initial_delay_ms
        );

        let database_url = format!("sqlite://{}", db_path);
        let mut last_error = None;

        // Retry loop for database initialization
        for attempt in 0..retry_config.max_attempts {
            if attempt > 0 {
                let delay = retry_config.calculate_delay(attempt - 1);
                warn!(
                    attempt = attempt + 1,
                    max_attempts = retry_config.max_attempts,
                    delay_ms = delay.as_millis(),
                    "Database connection failed, retrying after delay..."
                );
                tokio::time::sleep(delay).await;
            }

            match Self::try_initialize(db_path, &database_url).await {
                Ok(db) => {
                    // Run migrations (will retry entire initialization if this fails)
                    match db.migrate().await {
                        Ok(_) => {
                            info!(
                                attempts = attempt + 1,
                                "Database initialized and migrated successfully"
                            );
                            return Ok(db);
                        }
                        Err(e) => {
                            warn!(
                                attempt = attempt + 1,
                                error = %e,
                                "Database migration failed, will retry initialization"
                            );
                            last_error = Some(e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        attempt = attempt + 1,
                        error = %e,
                        "Database initialization failed"
                    );
                    last_error = Some(e);
                    continue;
                }
            }
        }

        // All retries exhausted
        let error_msg = match last_error {
            Some(e) => format!(
                "Failed to initialize database after {} attempts: {}",
                retry_config.max_attempts, e
            ),
            None => format!(
                "Failed to initialize database after {} attempts",
                retry_config.max_attempts
            ),
        };

        Err(Error::Database(error_msg))
    }

    /// Try to initialize the database connection (single attempt)
    async fn try_initialize(db_path: &str, database_url: &str) -> Result<Self> {
        // Create database if it doesn't exist
        if !Sqlite::database_exists(database_url)
            .await
            .unwrap_or(false)
        {
            debug!("Creating database: {}", database_url);
            Sqlite::create_database(database_url)
                .await
                .map_err(|e| Error::Database(format!("Failed to create database: {}", e)))?;
        }

        // Configure SQLite connection options with WAL mode and performance tuning
        let connect_options = SqliteConnectOptions::new()
            .filename(db_path)
            .journal_mode(SqliteJournalMode::Wal)
            .create_if_missing(true)
            .pragma("foreign_keys", "ON")
            .pragma("synchronous", "NORMAL")
            .pragma("cache_size", "10000")
            .pragma("temp_store", "memory")
            .pragma("busy_timeout", "30000") // 30 second timeout for lock contention
            .pragma("mmap_size", "268435456"); // 256 MB memory-mapped I/O

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .min_connections(1)
            .connect_with(connect_options)
            .await
            .map_err(|e| Error::Database(format!("Failed to create connection pool: {}", e)))?;

        Ok(Self { pool })
    }

    /// Run database migrations
    #[instrument(skip(self))]
    pub async fn migrate(&self) -> Result<()> {
        info!("Running database migrations");

        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| Error::Database(format!("Migration failed: {}", e)))?;

        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Create a Db instance from an existing pool (for testing/reuse)
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Check database health
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> Result<()> {
        debug!("Performing database health check");

        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::Database(format!("Health check failed: {}", e)))?;

        debug!("Database health check passed");
        Ok(())
    }

    /// Get database statistics
    #[instrument(skip(self))]
    pub async fn stats(&self) -> Result<DatabaseStats> {
        debug!("Gathering database statistics");

        let tables = vec![
            "users", "api_keys", "streams", "captures", "jobs", "alerts", "events",
        ];

        let mut table_counts = std::collections::HashMap::new();

        for table in &tables {
            let query = format!("SELECT COUNT(*) as count FROM {}", table);
            let row = sqlx::query(&query)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| {
                    Error::Database(format!("Failed to get count for {}: {}", table, e))
                })?;

            let count: i64 = row.get("count");
            table_counts.insert(table.to_string(), count);
        }

        debug!("Database statistics gathered successfully");
        Ok(DatabaseStats { table_counts })
    }
}

/// Database statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DatabaseStats {
    pub table_counts: std::collections::HashMap<String, i64>,
}

// Repository modules
pub mod repositories;

// Cache module
pub mod cache;

// Re-export common types and repositories
pub use cache::{CacheStats, DatabaseCache};
pub use repositories::{
    alerts::{Alert, AlertRepository, CreateAlertRequest},
    analysis_events::{AnalysisEvent, AnalysisEventRepository, CreateAnalysisEvent},
    api_keys::{ApiKey, ApiKeyRepository, CreateApiKeyRequest},
    background_snapshot_jobs::{
        BackgroundSnapshotJob, BackgroundSnapshotJobsRepository, CreateBackgroundJobRequest,
        UpdateBackgroundJobRequest,
    },
    cached_streams::CachedStreamRepository,
    cached_users::CachedUserRepository,
    captures::{Capture, CaptureRepository, CreateCaptureRequest, UpdateCaptureRequest},
    events::{CreateEventRequest, Event, EventRepository},
    jobs::{CreateJobRequest, Job, JobRepository, UpdateJobRequest},
    notification_deliveries::{
        CreateNotificationDelivery, DeliveryStatus, NotificationDelivery,
        NotificationDeliveryRepository, UpdateDeliveryStatus,
    },
    settings::{Setting, SettingsRepository, UpdateSettingRequest},
    snapshots::{CreateSnapshotRequest, Snapshot, SnapshotMetadata, SnapshotRepository},
    streams::{CreateStreamRequest, Stream, StreamRepository, UpdateStreamRequest},
    users::{CreateUserRequest, UpdateUserRequest, User, UserRepository},
};

#[cfg(test)]
mod tests {
    use super::*;
    use gl_core::Id;
    use tokio::fs;

    /// Create a test database with a unique name
    pub async fn create_test_db() -> Result<Db> {
        let test_id = Id::new().to_string();
        let db_path = format!("test_glimpser_{}.db", test_id);

        // Clean up any existing test database
        let _ = fs::remove_file(&db_path).await;

        let db = Db::new(&db_path).await?;
        Ok(db)
    }

    /// Clean up test database
    #[allow(dead_code)]
    async fn cleanup_test_db(db_path: &str) {
        let _ = fs::remove_file(db_path).await;
        let _ = fs::remove_file(format!("{}-wal", db_path)).await;
        let _ = fs::remove_file(format!("{}-shm", db_path)).await;
    }

    #[tokio::test]
    async fn test_database_initialization() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");

        // Test health check
        db.health_check().await.expect("Health check should pass");

        // Test stats
        let stats = db.stats().await.expect("Stats should be available");
        assert!(stats.table_counts.contains_key("users"));
        assert_eq!(stats.table_counts["users"], 0);
    }

    #[tokio::test]
    async fn test_user_repository_create_and_find() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");
        let repo = UserRepository::new(db.pool());

        // Create a user
        let create_request = CreateUserRequest {
            username: "testuser".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hashed_password".to_string(),
        };

        let user = repo
            .create(create_request)
            .await
            .expect("Failed to create user");

        assert!(!user.id.is_empty());
        assert_eq!(user.username, "testuser");
        assert_eq!(user.email, "test@example.com");
        // No admin roles needed
        assert!(user.is_active.unwrap_or(false));

        // Find by ID
        let found_user = repo
            .find_by_id(&user.id)
            .await
            .expect("Failed to find user")
            .expect("User should exist");

        assert_eq!(found_user.id, user.id);
        assert_eq!(found_user.username, user.username);

        // Find by username
        let found_by_username = repo
            .find_by_username("testuser")
            .await
            .expect("Failed to find user by username")
            .expect("User should exist");

        assert_eq!(found_by_username.id, user.id);

        // Find by email
        let found_by_email = repo
            .find_by_email("test@example.com")
            .await
            .expect("Failed to find user by email")
            .expect("User should exist");

        assert_eq!(found_by_email.id, user.id);
    }

    #[tokio::test]
    async fn test_user_repository_list_active() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");
        let repo = UserRepository::new(db.pool());

        // Initially no users
        let users = repo.list_active().await.expect("Failed to list users");
        assert_eq!(users.len(), 0);

        // Create a user
        let create_request = CreateUserRequest {
            username: "activeuser".to_string(),
            email: "active@example.com".to_string(),
            password_hash: "hashed_password".to_string(),
        };

        let _user = repo
            .create(create_request)
            .await
            .expect("Failed to create user");

        // Now should have one active user
        let users = repo.list_active().await.expect("Failed to list users");
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].username, "activeuser");
    }

    #[tokio::test]
    async fn test_api_key_repository_create_and_find() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");
        let user_repo = UserRepository::new(db.pool());
        let api_key_repo = ApiKeyRepository::new(db.pool());

        // First create a user
        let user_request = CreateUserRequest {
            username: "keyuser".to_string(),
            email: "keyuser@example.com".to_string(),
            password_hash: "hashed_password".to_string(),
        };
        let user = user_repo
            .create(user_request)
            .await
            .expect("Failed to create user");

        // Create API key
        let api_key_request = CreateApiKeyRequest {
            user_id: user.id.clone(),
            key_hash: "hashed_key".to_string(),
            name: "Test API Key".to_string(),
            permissions: "[\"read\", \"write\"]".to_string(),
            expires_at: None,
        };

        let api_key = api_key_repo
            .create(api_key_request)
            .await
            .expect("Failed to create API key");

        assert!(!api_key.id.is_empty());
        assert_eq!(api_key.user_id, user.id);
        assert_eq!(api_key.name, "Test API Key");
        assert!(api_key.is_active);

        // Find by hash
        let found_key = api_key_repo
            .find_by_hash("hashed_key")
            .await
            .expect("Failed to find API key")
            .expect("API key should exist");

        assert_eq!(found_key.id, api_key.id);

        // List by user
        let user_keys = api_key_repo
            .list_by_user(&user.id)
            .await
            .expect("Failed to list user API keys");

        assert_eq!(user_keys.len(), 1);
        assert_eq!(user_keys[0].id, api_key.id);
    }

    #[tokio::test]
    async fn test_database_migrations_run_successfully() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");

        // Run migrations again - should be idempotent
        db.migrate()
            .await
            .expect("Migrations should run successfully");

        // Verify all tables exist by checking stats
        let stats = db.stats().await.expect("Stats should be available");

        let expected_tables = vec![
            "users", "api_keys", "streams", "captures", "jobs", "alerts", "events",
        ];
        for table in expected_tables {
            assert!(
                stats.table_counts.contains_key(table),
                "Table {} should exist",
                table
            );
        }
    }
}
