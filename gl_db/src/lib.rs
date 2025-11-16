//! ABOUTME: Database layer with SQLite, migrations, and repositories
//! ABOUTME: Handles all data persistence and database operations

use gl_core::{Error, Result};
use sqlx::{
    migrate::MigrateDatabase,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Row, Sqlite, SqlitePool,
};
use tracing::{debug, info, instrument};

/// Allowed table names for statistics queries
/// This is a security measure to prevent SQL injection via dynamic table names
const ALLOWED_TABLES: &[&str] = &[
    "users",
    "api_keys",
    "streams",
    "captures",
    "jobs",
    "alerts",
    "events",
];

/// Validates that a table name contains only safe SQL identifier characters
///
/// # Security
/// This function validates SQL identifiers by ensuring they:
/// 1. Are not empty
/// 2. Start with a letter or underscore (valid SQL identifier start)
/// 3. Contain only alphanumeric characters and underscores
///
/// # Arguments
/// * `table` - The table name to validate
///
/// # Returns
/// * `true` if the table name is valid, `false` otherwise
fn is_safe_sql_identifier(table: &str) -> bool {
    // Must not be empty
    if table.is_empty() {
        return false;
    }

    let mut chars = table.chars();

    // First character must be a letter or underscore (SQL identifier rules)
    // Safe to unwrap because we already checked the string is not empty
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    // Remaining characters must be alphanumeric or underscore
    // Note: We use the same iterator, so this only checks chars after the first
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validates that a table name is in the allowed list and is a safe SQL identifier
///
/// # Security
/// This function prevents SQL injection by checking against an allow-list
/// of known table names.
///
/// # Arguments
/// * `table` - The table name to validate
///
/// # Returns
/// * `true` if the table name is valid and allowed, `false` otherwise
///
/// # Note
/// This function is currently used in tests and kept for potential future use
/// when validating table names from external sources (config, API, etc.)
#[allow(dead_code)]
fn is_valid_table_name(table: &str) -> bool {
    ALLOWED_TABLES.contains(&table) && is_safe_sql_identifier(table)
}

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
    ///
    /// # Security
    /// This method uses an allow-list of table names (ALLOWED_TABLES) to prevent SQL injection.
    /// All table names are validated to ensure they're safe SQL identifiers before being used in queries.
    #[instrument(skip(self))]
    pub async fn stats(&self) -> Result<DatabaseStats> {
        debug!("Gathering database statistics");

        let mut table_counts = std::collections::HashMap::new();

        // Iterate over the allowed tables constant
        for &table in ALLOWED_TABLES {
            // Validate that the table name is a safe SQL identifier
            // This is defense in depth - the const should already be safe,
            // but we validate to catch any programming errors if ALLOWED_TABLES is modified
            if !is_safe_sql_identifier(table) {
                return Err(Error::Database(format!(
                    "ALLOWED_TABLES contains invalid SQL identifier: '{}'. This is a programming error.",
                    table
                )));
            }

            // Safe to use table name in query after validation
            // Note: SQLx doesn't support parameterized table names, so we use format!
            // but only after strict validation against the allow-list and identifier rules
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

    #[test]
    fn test_safe_sql_identifier_validation() {
        // Test valid SQL identifiers
        assert!(is_safe_sql_identifier("users"));
        assert!(is_safe_sql_identifier("api_keys"));
        assert!(is_safe_sql_identifier("_private"));
        assert!(is_safe_sql_identifier("table123"));
        assert!(is_safe_sql_identifier("MyTable"));

        // Test valid single character identifiers
        assert!(is_safe_sql_identifier("_"));
        assert!(is_safe_sql_identifier("a"));
        assert!(is_safe_sql_identifier("Z"));

        // Test invalid - starts with number
        assert!(!is_safe_sql_identifier("1users"));
        assert!(!is_safe_sql_identifier("9table"));
        assert!(!is_safe_sql_identifier("0"));

        // Test invalid - empty
        assert!(!is_safe_sql_identifier(""));

        // Test invalid - special characters
        assert!(!is_safe_sql_identifier("user-table"));
        assert!(!is_safe_sql_identifier("user.table"));
        assert!(!is_safe_sql_identifier("user table"));
        assert!(!is_safe_sql_identifier("user;table"));
        assert!(!is_safe_sql_identifier("user'table"));
        assert!(!is_safe_sql_identifier("user\"table"));
        assert!(!is_safe_sql_identifier("$"));
        assert!(!is_safe_sql_identifier("@table"));

        // Test invalid - SQL injection attempts
        assert!(!is_safe_sql_identifier("users' OR '1'='1"));
        assert!(!is_safe_sql_identifier("users; DROP TABLE users"));
        assert!(!is_safe_sql_identifier("users--"));
    }

    #[test]
    fn test_valid_table_name_validation() {
        // Test valid table names from the allow-list
        assert!(is_valid_table_name("users"));
        assert!(is_valid_table_name("api_keys"));
        assert!(is_valid_table_name("streams"));
        assert!(is_valid_table_name("captures"));
        assert!(is_valid_table_name("jobs"));
        assert!(is_valid_table_name("alerts"));
        assert!(is_valid_table_name("events"));
    }

    #[test]
    fn test_invalid_table_name_validation() {
        // Test table names not in the allow-list (even if valid identifiers)
        assert!(!is_valid_table_name("malicious_table"));
        assert!(!is_valid_table_name("other_table"));
        assert!(!is_valid_table_name("passwords"));

        // Test SQL injection attempts
        assert!(!is_valid_table_name("DROP TABLE users"));
        assert!(!is_valid_table_name("users; DROP TABLE users--"));
        assert!(!is_valid_table_name("users' OR '1'='1"));
        assert!(!is_valid_table_name("users--"));
        assert!(!is_valid_table_name("users/*"));
        assert!(!is_valid_table_name("users; DELETE FROM users"));

        // Test special characters that should be rejected
        assert!(!is_valid_table_name("users;"));
        assert!(!is_valid_table_name("users'"));
        assert!(!is_valid_table_name("users\""));
        assert!(!is_valid_table_name("users "));
        assert!(!is_valid_table_name(" users"));
        assert!(!is_valid_table_name("users-table"));
        assert!(!is_valid_table_name("users.table"));

        // Test empty and whitespace
        assert!(!is_valid_table_name(""));
        assert!(!is_valid_table_name(" "));
        assert!(!is_valid_table_name("\t"));
        assert!(!is_valid_table_name("\n"));

        // Test identifiers starting with numbers
        assert!(!is_valid_table_name("1users"));
    }

    #[test]
    fn test_allowed_tables_constant() {
        // Verify all tables in ALLOWED_TABLES are safe SQL identifiers
        for &table in ALLOWED_TABLES {
            assert!(
                is_safe_sql_identifier(table),
                "ALLOWED_TABLES contains invalid SQL identifier: {}",
                table
            );
        }

        // Verify expected tables are present
        assert!(ALLOWED_TABLES.contains(&"users"));
        assert!(ALLOWED_TABLES.contains(&"api_keys"));
        assert!(ALLOWED_TABLES.contains(&"streams"));
        assert!(ALLOWED_TABLES.contains(&"captures"));
        assert!(ALLOWED_TABLES.contains(&"jobs"));
        assert!(ALLOWED_TABLES.contains(&"alerts"));
        assert!(ALLOWED_TABLES.contains(&"events"));

        // Verify count
        assert_eq!(ALLOWED_TABLES.len(), 7, "Expected 7 allowed tables");
    }

    #[tokio::test]
    async fn test_stats_uses_allowed_tables_only() {
        let db = create_test_db()
            .await
            .expect("Failed to create test database");

        let stats = db.stats().await.expect("Stats should be available");

        // Verify stats only contains allowed tables
        for table_name in stats.table_counts.keys() {
            assert!(
                ALLOWED_TABLES.contains(&table_name.as_str()),
                "Stats contains unauthorized table: {}",
                table_name
            );
        }

        // Verify all allowed tables are in stats
        for &table in ALLOWED_TABLES {
            assert!(
                stats.table_counts.contains_key(table),
                "Stats missing allowed table: {}",
                table
            );
        }
    }
}
