//! ABOUTME: Database layer with SQLite, migrations, and repositories
//! ABOUTME: Handles all data persistence and database operations
//!
//! ## SQLite Version Requirements
//!
//! This library requires SQLite 3.8.0 or higher for optimal performance and features:
//! - WAL mode support (3.7.0+)
//! - Memory-mapped I/O (3.7.17+)
//! - Improved busy handler (3.8.0+)
//!
//! The pragma validation system will warn about unsupported features on older versions.

use gl_core::{Error, Result};
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, Sqlite, SqlitePool,
};
use tracing::{debug, info, warn, instrument};

/// Database connection pool and operations
#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

/// Pragma configuration for validation
#[derive(Debug)]
struct PragmaConfig {
    name: &'static str,
    expected: &'static str,
    min_version: Option<&'static str>,
}

impl PragmaConfig {
    fn new(name: &'static str, expected: &'static str) -> Self {
        Self {
            name,
            expected,
            min_version: None,
        }
    }

    fn with_min_version(name: &'static str, expected: &'static str, min_version: &'static str) -> Self {
        Self {
            name,
            expected,
            min_version: Some(min_version),
        }
    }
}

impl Db {
    /// Validate and log pragma settings
    #[instrument(skip(pool))]
    async fn validate_pragmas(pool: &SqlitePool) -> Result<()> {
        info!("Validating SQLite pragma settings");

        // Get SQLite version first
        let version_row = sqlx::query("SELECT sqlite_version() as version")
            .fetch_one(pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to get SQLite version: {}", e)))?;

        let sqlite_version: String = version_row.get("version");
        info!("SQLite version: {}", sqlite_version);

        // Define expected pragma values
        let pragmas = vec![
            PragmaConfig::new("foreign_keys", "1"),
            PragmaConfig::new("synchronous", "1"), // NORMAL = 1
            PragmaConfig::new("cache_size", "-10000"), // Negative means KB, so -10000 = 10MB
            PragmaConfig::new("temp_store", "2"), // MEMORY = 2
            PragmaConfig::new("busy_timeout", "30000"),
            PragmaConfig::with_min_version("mmap_size", "268435456", "3.7.17"),
        ];

        let mut validation_errors = Vec::new();

        for pragma in pragmas {
            // Check version requirement if specified
            if let Some(min_version) = pragma.min_version {
                if !Self::version_meets_requirement(&sqlite_version, min_version) {
                    warn!(
                        "Pragma '{}' requires SQLite {} but current version is {}. Skipping validation.",
                        pragma.name, min_version, sqlite_version
                    );
                    continue;
                }
            }

            // Query the current pragma value
            let query = format!("PRAGMA {}", pragma.name);
            match sqlx::query(&query).fetch_one(pool).await {
                Ok(row) => {
                    // Try to get the value as a string first, then as i64
                    let actual_value = if let Ok(val) = row.try_get::<String, _>(0) {
                        val
                    } else if let Ok(val) = row.try_get::<i64, _>(0) {
                        val.to_string()
                    } else {
                        validation_errors.push(format!(
                            "Pragma '{}': Unable to read value",
                            pragma.name
                        ));
                        continue;
                    };

                    if actual_value == pragma.expected {
                        debug!("Pragma '{}' validated: {}", pragma.name, actual_value);
                    } else {
                        warn!(
                            "Pragma '{}' mismatch: expected '{}', got '{}'",
                            pragma.name, pragma.expected, actual_value
                        );
                        validation_errors.push(format!(
                            "Pragma '{}': expected '{}', got '{}'",
                            pragma.name, pragma.expected, actual_value
                        ));
                    }
                }
                Err(e) => {
                    warn!("Failed to query pragma '{}': {}", pragma.name, e);
                    validation_errors.push(format!(
                        "Pragma '{}': query failed - {}",
                        pragma.name, e
                    ));
                }
            }
        }

        // Log summary
        if validation_errors.is_empty() {
            info!("All pragma settings validated successfully");
            Ok(())
        } else {
            warn!(
                "Pragma validation completed with {} issue(s): {}",
                validation_errors.len(),
                validation_errors.join("; ")
            );
            // Don't fail on pragma validation errors, just warn
            // This allows the database to continue working even if some optimizations fail
            Ok(())
        }
    }

    /// Check if SQLite version meets minimum requirement
    fn version_meets_requirement(current: &str, required: &str) -> bool {
        let parse_version = |v: &str| -> Option<Vec<u32>> {
            v.split('.')
                .map(|part| part.parse::<u32>().ok())
                .collect::<Option<Vec<_>>>()
        };

        let current_parts = match parse_version(current) {
            Some(parts) => parts,
            None => return false,
        };

        let required_parts = match parse_version(required) {
            Some(parts) => parts,
            None => return false,
        };

        for (c, r) in current_parts.iter().zip(required_parts.iter()) {
            match c.cmp(r) {
                std::cmp::Ordering::Greater => return true,
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal => continue,
            }
        }

        // If all parts are equal up to the length of required_parts, version meets requirement
        true
    }

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

        // Validate pragma settings
        Self::validate_pragmas(&pool).await?;

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

    #[tokio::test]
    async fn test_pragma_validation() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");

        // Pragma validation should have already run during database initialization
        // Let's manually verify some key pragmas
        let pool = db.pool();

        // Check foreign_keys is enabled
        let fk_row = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(pool)
            .await
            .expect("Should be able to query foreign_keys pragma");
        let fk_value: i64 = fk_row.get(0);
        assert_eq!(fk_value, 1, "Foreign keys should be enabled");

        // Check synchronous mode
        let sync_row = sqlx::query("PRAGMA synchronous")
            .fetch_one(pool)
            .await
            .expect("Should be able to query synchronous pragma");
        let sync_value: i64 = sync_row.get(0);
        assert_eq!(sync_value, 1, "Synchronous should be NORMAL (1)");

        // Check temp_store is in memory
        let temp_row = sqlx::query("PRAGMA temp_store")
            .fetch_one(pool)
            .await
            .expect("Should be able to query temp_store pragma");
        let temp_value: i64 = temp_row.get(0);
        assert_eq!(temp_value, 2, "Temp store should be MEMORY (2)");
    }

    #[test]
    fn test_version_comparison() {
        // Test exact match
        assert!(Db::version_meets_requirement("3.8.0", "3.8.0"));

        // Test newer major version
        assert!(Db::version_meets_requirement("4.0.0", "3.8.0"));

        // Test newer minor version
        assert!(Db::version_meets_requirement("3.9.0", "3.8.0"));

        // Test newer patch version
        assert!(Db::version_meets_requirement("3.8.1", "3.8.0"));

        // Test older version
        assert!(!Db::version_meets_requirement("3.7.0", "3.8.0"));

        // Test with different number of components
        assert!(Db::version_meets_requirement("3.8.0.1", "3.8.0"));
        assert!(Db::version_meets_requirement("3.8", "3.8.0"));
    }

    #[tokio::test]
    async fn test_sqlite_version_detection() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");

        // Query SQLite version
        let version_row = sqlx::query("SELECT sqlite_version() as version")
            .fetch_one(db.pool())
            .await
            .expect("Should be able to get SQLite version");

        let version: String = version_row.get("version");
        assert!(!version.is_empty(), "SQLite version should not be empty");
        assert!(
            version.starts_with("3."),
            "SQLite version should be 3.x.x"
        );

        // Verify it meets our minimum requirement
        assert!(
            Db::version_meets_requirement(&version, "3.8.0"),
            "SQLite version {} should meet minimum requirement 3.8.0",
            version
        );
    }
}
