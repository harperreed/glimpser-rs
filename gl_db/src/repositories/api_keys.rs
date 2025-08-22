//! ABOUTME: API key repository for managing API authentication tokens
//! ABOUTME: Provides compile-time checked queries for API key CRUD operations

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use tracing::{debug, instrument};

/// API key entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub key_hash: String,
    pub name: String,
    pub permissions: String, // JSON array
    pub expires_at: Option<String>,
    pub is_active: bool,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new API key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub user_id: String,
    pub key_hash: String,
    pub name: String,
    pub permissions: String, // JSON array
    pub expires_at: Option<String>,
}

/// API key repository
pub struct ApiKeyRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ApiKeyRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new API key
    #[instrument(skip(self, request))]
    pub async fn create(&self, request: CreateApiKeyRequest) -> Result<ApiKey> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        debug!("Creating API key with id: {}", id);

        let api_key = sqlx::query_as!(
            ApiKey,
            r#"
            INSERT INTO api_keys (id, user_id, key_hash, name, permissions, expires_at, is_active, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, true, ?7, ?8)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.key_hash,
            request.name,
            request.permissions,
            request.expires_at,
            now,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create API key: {}", e)))?;

        debug!("Successfully created API key: {}", api_key.id);
        Ok(api_key)
    }

    /// Find API key by hash
    #[instrument(skip(self))]
    pub async fn find_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        debug!("Finding API key by hash");

        let api_key = sqlx::query_as!(
            ApiKey,
            "SELECT * FROM api_keys WHERE key_hash = ?1 AND is_active = true",
            key_hash
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find API key by hash: {}", e)))?;

        Ok(api_key)
    }

    /// List API keys for a user
    #[instrument(skip(self))]
    pub async fn list_by_user(&self, user_id: &str) -> Result<Vec<ApiKey>> {
        debug!("Listing API keys for user: {}", user_id);

        let api_keys = sqlx::query_as!(
            ApiKey,
            "SELECT * FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC",
            user_id
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to list API keys: {}", e)))?;

        debug!("Found {} API keys for user", api_keys.len());
        Ok(api_keys)
    }
}
