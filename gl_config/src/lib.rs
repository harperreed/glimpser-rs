//! ABOUTME: Configuration management with validation and environment loading
//! ABOUTME: Handles all application settings from environment variables and files

use config::{Config as ConfigBuilder, Environment, File};
use gl_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use validator::Validate;

/// Main configuration struct
#[derive(Debug, Clone, Deserialize, Serialize, Validate, Default)]
#[serde(default)]
pub struct Config {
    #[validate(nested)]
    pub server: ServerConfig,
    #[validate(nested)]
    pub database: DatabaseConfig,
    #[validate(nested)]
    pub security: SecurityConfig,
    pub features: FeaturesConfig,
    #[validate(nested)]
    pub external: ExternalConfig,
    #[validate(nested)]
    pub storage: StorageConfig,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct ServerConfig {
    #[validate(length(min = 1))]
    pub host: String,
    #[validate(range(min = 1, max = 65535))]
    pub port: u16,
    #[validate(range(min = 1, max = 65535))]
    pub obs_port: u16,
    /// Directory containing PWA static files
    pub static_dir: String,
    /// Enable CSP headers for PWA
    pub enable_csp: bool,
    /// Cache max-age for static assets (seconds)
    pub static_max_age: u32,
    /// Rate limiting configuration
    #[validate(nested)]
    pub rate_limit: RateLimitConfig,
    /// Body size limits configuration
    #[validate(nested)]
    pub body_limits: BodyLimitsConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            obs_port: 9000,
            static_dir: "./static".to_string(),
            enable_csp: true,
            static_max_age: 86400, // 1 day
            rate_limit: RateLimitConfig::default(),
            body_limits: BodyLimitsConfig::default(),
        }
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct RateLimitConfig {
    /// Maximum requests per minute (IP-based only)
    #[validate(range(min = 1, max = 10000))]
    pub requests_per_minute: u32,
    /// Rate limiting window duration in seconds
    #[validate(range(min = 1, max = 3600))]
    pub window_seconds: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            window_seconds: 60,
        }
    }
}

/// Body size limits configuration
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct BodyLimitsConfig {
    /// Global JSON body size limit in bytes
    #[validate(range(min = 1024, max = 104857600))] // 1KB to 100MB
    pub global_json_limit: usize,
    /// Upload endpoints limit in bytes
    #[validate(range(min = 1024, max = 1073741824))] // 1KB to 1GB
    pub upload_limit: usize,
}

impl Default for BodyLimitsConfig {
    fn default() -> Self {
        Self {
            global_json_limit: 1048576, // 1MB default
            upload_limit: 104857600,    // 100MB for uploads
        }
    }
}

/// Database configuration
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct DatabaseConfig {
    #[validate(length(min = 1))]
    pub path: String,
    #[validate(range(min = 1, max = 100))]
    pub pool_size: u32,
    pub sqlite_wal: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "glimpser.db".to_string(),
            pool_size: 10,
            sqlite_wal: true,
        }
    }
}

/// Security configuration with secret redaction
#[derive(Clone, Deserialize, Serialize, Validate)]
pub struct SecurityConfig {
    #[validate(length(min = 32))]
    pub jwt_secret: String,
    pub argon2_params: Argon2Config,
    /// Whether to use secure cookies (requires HTTPS)
    pub secure_cookies: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        // Generate a random JWT secret by default for security
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();

        Self {
            jwt_secret: format!("INSECURE-RANDOM-{}-CHANGE-IN-PRODUCTION", timestamp),
            argon2_params: Argon2Config::default(),
            secure_cookies: false, // Default to false for development
        }
    }
}

impl fmt::Debug for SecurityConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecurityConfig")
            .field("jwt_secret", &"[REDACTED]")
            .field("argon2_params", &self.argon2_params)
            .finish()
    }
}

/// Argon2 parameters
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct Argon2Config {
    #[validate(range(min = 1024, max = 1048576))] // 1 KiB to 1 GiB
    pub memory_cost: u32,
    #[validate(range(min = 1, max = 100))]
    pub time_cost: u32,
    #[validate(range(min = 1, max = 16))]
    pub parallelism: u32,
}

impl Default for Argon2Config {
    fn default() -> Self {
        Self {
            memory_cost: 19456,
            time_cost: 2,
            parallelism: 1,
        }
    }
}

/// Feature flags
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FeaturesConfig {
    pub enable_rtsp: bool,
    pub enable_ai: bool,
}

/// External service configuration with secret redaction
#[derive(Clone, Deserialize, Serialize, Validate, Default)]
#[serde(default)]
pub struct ExternalConfig {
    pub twilio: Option<TwilioConfig>,
    pub smtp: Option<SmtpConfig>,
    #[validate(url)]
    pub webhook_base_url: Option<String>,
}

impl fmt::Debug for ExternalConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalConfig")
            .field("twilio", &self.twilio.as_ref().map(|_| "[REDACTED]"))
            .field("smtp", &self.smtp.as_ref().map(|_| "[REDACTED]"))
            .field("webhook_base_url", &self.webhook_base_url)
            .finish()
    }
}

/// Twilio configuration
#[derive(Clone, Deserialize, Serialize, Validate)]
pub struct TwilioConfig {
    #[validate(length(min = 1))]
    pub account_sid: String,
    #[validate(length(min = 1))]
    pub auth_token: String,
    #[validate(length(min = 1))]
    pub from_number: String,
}

impl fmt::Debug for TwilioConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwilioConfig")
            .field("account_sid", &"[REDACTED]")
            .field("auth_token", &"[REDACTED]")
            .field("from_number", &self.from_number)
            .finish()
    }
}

/// SMTP configuration
#[derive(Clone, Deserialize, Serialize, Validate)]
pub struct SmtpConfig {
    #[validate(length(min = 1))]
    pub host: String,
    #[validate(range(min = 1, max = 65535))]
    pub port: u16,
    #[validate(email)]
    pub username: String,
    #[validate(length(min = 1))]
    pub password: String,
}

impl fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Storage configuration
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(default)]
pub struct StorageConfig {
    /// Local filesystem storage directory for artifacts
    pub artifacts_dir: String,
    /// Object store URL for S3-compatible storage
    pub object_store_url: Option<String>,
    /// S3 bucket name
    pub bucket: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            artifacts_dir: "./data/artifacts".to_string(),
            object_store_url: None,
            bucket: None,
        }
    }
}

impl Config {
    /// Load configuration from environment variables and optional .env file
    pub fn load() -> Result<Self> {
        let mut builder = ConfigBuilder::builder();

        // Set defaults first
        builder = builder
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("server.obs_port", 9000)?
            .set_default("server.static_dir", "./static")?
            .set_default("server.enable_csp", true)?
            .set_default("server.static_max_age", 86400)?
            .set_default("server.rate_limit.requests_per_minute", 100)?
            .set_default("server.rate_limit.window_seconds", 60)?
            .set_default("server.body_limits.global_json_limit", 1048576)?
            .set_default("server.body_limits.upload_limit", 104857600)?
            .set_default("database.path", "glimpser.db")?
            .set_default("database.pool_size", 10)?
            .set_default("database.sqlite_wal", true)?
            .set_default("security.argon2_params.memory_cost", 19456)?
            .set_default("security.argon2_params.time_cost", 2)?
            .set_default("security.argon2_params.parallelism", 1)?
            .set_default("security.secure_cookies", false)?
            .set_default("features.enable_rtsp", false)?
            .set_default("features.enable_ai", false)?
            .set_default("storage.artifacts_dir", "./data/artifacts")?;

        // Handle nested environment variables that don't work with the standard separator
        // JWT secret
        if let Ok(jwt_secret) = std::env::var("GLIMPSER_SECURITY_JWT_SECRET") {
            builder = builder.set_override("security.jwt_secret", jwt_secret)?;
        } else {
            let default_jwt_secret = format!(
                "INSECURE-RANDOM-{}-CHANGE-IN-PRODUCTION",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            builder = builder.set_default("security.jwt_secret", default_jwt_secret)?;
        }

        // Database pool size
        if let Ok(pool_size) = std::env::var("GLIMPSER_DATABASE_POOL_SIZE") {
            builder = builder.set_override("database.pool_size", pool_size)?;
        }

        // Server observability port
        if let Ok(obs_port) = std::env::var("GLIMPSER_SERVER_OBS_PORT") {
            builder = builder.set_override("server.obs_port", obs_port)?;
        }

        // Try to load from .env file if it exists (optional)
        if std::path::Path::new(".env").exists() {
            builder = builder.add_source(File::with_name(".env").required(false));
        }

        // Load from environment variables with GLIMPSER_ prefix (highest priority)
        builder = builder.add_source(
            Environment::with_prefix("GLIMPSER")
                .try_parsing(true)
                .separator("_"),
        );

        let config = builder
            .build()
            .map_err(|e| Error::Config(format!("Failed to build config: {}", e)))?;

        let parsed: Config = config
            .try_deserialize()
            .map_err(|e| Error::Config(format!("Failed to deserialize config: {}", e)))?;

        // Validate the configuration
        let validation_result = parsed.validate();
        validation_result.map_err(|e| Error::Config(format!("Config validation failed: {}", e)))?;

        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Use a mutex to serialize tests that modify environment variables
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_config_defaults() {
        let _lock = ENV_MUTEX.lock().unwrap();

        // Clear any existing env vars that might interfere
        let vars_to_clear = [
            "GLIMPSER_SERVER_HOST",
            "GLIMPSER_SERVER_PORT",
            "GLIMPSER_DATABASE_PATH",
            "GLIMPSER_DATABASE_POOL_SIZE",
            "GLIMPSER_SECURITY_JWT_SECRET",
            "GLIMPSER_SECURITY_SECURE_COOKIES",
        ];

        let original_values: Vec<_> = vars_to_clear.iter().map(|key| env::var(key).ok()).collect();

        for key in &vars_to_clear {
            env::remove_var(key);
        }

        let config = Config::load().expect("Should load with defaults");

        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.database.path, "glimpser.db");
        assert_eq!(config.database.pool_size, 10);
        assert!(config.database.sqlite_wal);

        // Restore original env vars
        for (key, value) in vars_to_clear.iter().zip(original_values.iter()) {
            if let Some(val) = value {
                env::set_var(key, val);
            }
        }
    }

    #[test]
    fn test_config_from_env() {
        let _lock = ENV_MUTEX.lock().unwrap();

        // Clear any existing env vars first
        env::remove_var("GLIMPSER_SERVER_HOST");
        env::remove_var("GLIMPSER_SERVER_PORT");
        env::remove_var("GLIMPSER_DATABASE_POOL_SIZE");
        env::remove_var("GLIMPSER_SECURITY_JWT_SECRET");

        env::set_var("GLIMPSER_SERVER_HOST", "0.0.0.0");
        env::set_var("GLIMPSER_SERVER_PORT", "9000");
        env::set_var(
            "GLIMPSER_SECURITY_JWT_SECRET",
            "valid32characterjwtsecretfortest",
        ); // Valid length

        let config = Config::load().expect("Should load from env");

        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 9000);

        // Cleanup
        env::remove_var("GLIMPSER_SERVER_HOST");
        env::remove_var("GLIMPSER_SERVER_PORT");
        env::remove_var("GLIMPSER_SECURITY_JWT_SECRET");
    }

    #[test]
    fn test_config_validation_failure() {
        let _lock = ENV_MUTEX.lock().unwrap();

        // Clear any existing values first
        env::remove_var("GLIMPSER_SERVER_PORT");
        env::set_var(
            "GLIMPSER_SECURITY_JWT_SECRET",
            "toolongbutstillvalid32charactershere",
        ); // Valid length
        env::set_var("GLIMPSER_DATABASE_POOL_SIZE", "200"); // Invalid - too big

        let result = Config::load();
        assert!(result.is_err());

        // Cleanup
        env::remove_var("GLIMPSER_SECURITY_JWT_SECRET");
        env::remove_var("GLIMPSER_DATABASE_POOL_SIZE");
    }

    #[test]
    fn test_secret_redaction() {
        let _lock = ENV_MUTEX.lock().unwrap();

        // Clear any environment variables that might interfere
        env::remove_var("GLIMPSER_SERVER_PORT");
        env::remove_var("GLIMPSER_DATABASE_POOL_SIZE");
        env::remove_var("GLIMPSER_SECURITY_JWT_SECRET");

        let config = Config::load().expect("Should load with defaults");
        let debug_output = format!("{:?}", config);

        // Secrets should be redacted
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("INSECURE-RANDOM"));
    }

    #[test]
    fn test_jwt_secret_too_short() {
        let _lock = ENV_MUTEX.lock().unwrap();

        env::set_var("GLIMPSER_SECURITY_JWT_SECRET", "short"); // Too short

        let result = Config::load();
        assert!(result.is_err());

        // Cleanup
        env::remove_var("GLIMPSER_SECURITY_JWT_SECRET");
    }
}
