//! ABOUTME: Database layer with SQLite, migrations, and repositories
//! ABOUTME: Handles all data persistence and database operations

use gl_core::{Error, Result};
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, Sqlite, SqlitePool,
};
use tracing::{debug, info, instrument};

/// Database connection pool and operations
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Create a new database connection with migrations
    #[instrument(skip(db_path))]
    pub async fn new(db_path: &str) -> Result<Self> {
        info!("Initializing database at: {}", db_path);

        // Create database if it doesn't exist
        let database_url = format!("sqlite://{}", db_path);
        if !Sqlite::database_exists(&database_url)
            .await
            .unwrap_or(false)
        {
            info!("Creating database: {}", database_url);
            Sqlite::create_database(&database_url)
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

        let db = Self { pool };

        // Run migrations
        db.migrate().await?;

        info!("Database initialized successfully");
        Ok(db)
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
