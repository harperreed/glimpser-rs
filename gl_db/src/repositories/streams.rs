//! ABOUTME: Stream repository for managing video stream configurations
//! ABOUTME: Provides compile-time checked queries for stream CRUD operations

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};

/// Stream entity (mirrors templates schema)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Stream {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: String, // JSON
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub execution_status: Option<String>,
    pub last_executed_at: Option<String>,
    pub last_error_message: Option<String>,
}

/// Request to create a new stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStreamRequest {
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: String, // JSON
    pub is_default: bool,
}

/// Request to update a stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStreamRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub config: Option<String>,
    pub is_default: Option<bool>,
}

pub struct StreamRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> StreamRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, request: CreateStreamRequest) -> Result<Stream> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        let stream = sqlx::query_as::<_, Stream>(
            r#"
            INSERT INTO streams (id, user_id, name, description, config, is_default, created_at, updated_at, execution_status, last_executed_at, last_error_message)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'inactive', NULL, NULL)
            RETURNING id, user_id, name, description, config, is_default, created_at, updated_at, execution_status, last_executed_at, last_error_message
            "#,
        )
        .bind(&id)
        .bind(&request.user_id)
        .bind(&request.name)
        .bind(&request.description)
        .bind(&request.config)
        .bind(request.is_default)
        .bind(&now)
        .bind(&now)
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create stream: {}", e)))?;

        Ok(stream)
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<Stream>> {
        let stream = sqlx::query_as::<_, Stream>(
            r#"
            SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                   execution_status, last_executed_at, last_error_message
            FROM streams WHERE id = ?1
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find stream: {}", e)))?;

        Ok(stream)
    }

    /// Update a stream
    ///
    /// This method uses a transaction internally to ensure atomicity of the
    /// read-then-write operation. If any step fails, the entire operation is
    /// rolled back to prevent partial updates.
    ///
    /// Transaction timeout: 30 seconds (reasonable default for repository operations)
    pub async fn update(&self, id: &str, request: UpdateStreamRequest) -> Result<Option<Stream>> {
        use std::time::Duration;
        use tracing::warn;

        let now = now_iso8601();

        // Use transaction to ensure atomicity of read-then-write operation
        // Note: This creates a transaction directly rather than using Db::with_transaction()
        // because repositories only have access to SqlitePool, not the Db wrapper
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        // Wrap the transaction operations in a timeout to prevent indefinite hangs
        let result = tokio::time::timeout(Duration::from_secs(30), async {
            // Fetch existing stream within transaction
            let existing = sqlx::query_as::<_, Stream>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message
                FROM streams WHERE id = ?1
                "#,
            )
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Error::Database(format!("Failed to find stream: {}", e)))?;

            let existing = match existing {
                Some(s) => s,
                None => return Ok(None),
            };

            let name = request.name.unwrap_or(existing.name);
            let description = request.description.or(existing.description);
            let config = request.config.unwrap_or(existing.config);
            let is_default = request.is_default.unwrap_or(existing.is_default);

            let stream = sqlx::query_as::<_, Stream>(
                r#"
                UPDATE streams
                SET name = ?1, description = ?2, config = ?3, is_default = ?4, updated_at = ?5
                WHERE id = ?6
                RETURNING id, user_id, name, description, config, is_default, created_at, updated_at,
                          execution_status, last_executed_at, last_error_message
                "#,
            )
            .bind(&name)
            .bind(&description)
            .bind(&config)
            .bind(is_default)
            .bind(&now)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Error::Database(format!("Failed to update stream: {}", e)))?;

            // Commit transaction
            tx.commit()
                .await
                .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

            Ok::<Option<Stream>, Error>(stream)
        })
        .await;

        match result {
            Ok(result) => result,
            Err(_) => {
                warn!("Stream update transaction timed out after 30 seconds (will auto-rollback)");
                Err(Error::Database(
                    "Stream update transaction timed out after 30 seconds".to_string(),
                ))
            }
        }
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM streams WHERE id = ?1")
            .bind(id)
            .execute(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to delete stream: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn list(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Stream>> {
        let streams = if let Some(uid) = user_id {
            sqlx::query_as::<_, Stream>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message
                FROM streams WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3
                "#,
            )
            .bind(uid)
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        } else {
            sqlx::query_as::<_, Stream>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message
                FROM streams ORDER BY created_at DESC LIMIT ?1 OFFSET ?2
                "#,
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
        };

        streams.map_err(|e| Error::Database(format!("Failed to list streams: {}", e)))
    }

    pub async fn count(&self, user_id: Option<&str>) -> Result<i64> {
        let count_val = if let Some(uid) = user_id {
            let row = sqlx::query("SELECT COUNT(*) as count FROM streams WHERE user_id = ?1")
                .bind(uid)
                .fetch_one(self.pool)
                .await
                .map_err(|e| Error::Database(format!("Failed to count streams: {}", e)))?;
            row.get::<i64, _>(0)
        } else {
            let row = sqlx::query("SELECT COUNT(*) as count FROM streams")
                .fetch_one(self.pool)
                .await
                .map_err(|e| Error::Database(format!("Failed to count streams: {}", e)))?;
            row.get::<i64, _>(0)
        };
        Ok(count_val)
    }

    pub async fn search_by_name(
        &self,
        name_pattern: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Stream>> {
        let pattern = format!("%{}%", name_pattern);
        let streams = sqlx::query_as::<_, Stream>(
            r#"
            SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                   execution_status, last_executed_at, last_error_message
            FROM streams WHERE name LIKE ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3
            "#,
        )
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to search streams: {}", e)))?;

        Ok(streams)
    }

    pub async fn find_by_name_and_user(&self, name: &str, user_id: &str) -> Result<Option<Stream>> {
        let stream = sqlx::query_as::<_, Stream>(
            r#"
            SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                   execution_status, last_executed_at, last_error_message
            FROM streams WHERE name = ?1 AND user_id = ?2
            "#,
        )
        .bind(name)
        .bind(user_id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find stream by name and user: {}", e)))?;

        Ok(stream)
    }

    pub async fn update_execution_status(
        &self,
        id: &str,
        status: &str,
        last_executed_at: Option<&str>,
    ) -> Result<bool> {
        let now = now_iso8601();
        let result = sqlx::query(
            r#"
            UPDATE streams
            SET execution_status = ?1, last_executed_at = ?2, updated_at = ?3
            WHERE id = ?4
            "#,
        )
        .bind(status)
        .bind(last_executed_at)
        .bind(&now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update stream execution status: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn update_execution_status_with_error(
        &self,
        id: &str,
        status: &str,
        error_message: &str,
    ) -> Result<bool> {
        let now = now_iso8601();
        let result = sqlx::query(
            r#"
            UPDATE streams
            SET execution_status = ?1, last_error_message = ?2, updated_at = ?3
            WHERE id = ?4
            "#,
        )
        .bind(status)
        .bind(error_message)
        .bind(&now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to update stream execution status with error: {}",
                e
            ))
        })?;

        Ok(result.rows_affected() > 0)
    }

    /// Reset all streams with "active" or "starting" status to "inactive" on startup
    /// This fixes stale statuses when server restarts but capture processes are gone
    pub async fn reset_stale_active_statuses(&self) -> Result<()> {
        let result = sqlx::query(
            "UPDATE streams
             SET execution_status = 'inactive',
                 last_error_message = 'Reset on server restart'
             WHERE execution_status IN ('active', 'starting')",
        )
        .execute(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to reset stale statuses: {}", e)))?;

        if result.rows_affected() > 0 {
            tracing::info!(
                "Reset {} stale stream statuses to inactive",
                result.rows_affected()
            );
        }

        Ok(())
    }

    /// List streams with total count in a single query to eliminate N+1 pattern
    pub async fn list_with_total(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<Stream>, i64)> {
        let query = if let Some(uid) = user_id {
            sqlx::query_as::<_, StreamWithCount>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message,
                       COUNT(*) OVER() as total_count
                FROM streams
                WHERE user_id = ?1
                ORDER BY created_at DESC
                LIMIT ?2 OFFSET ?3
                "#,
            )
            .bind(uid)
            .bind(limit)
            .bind(offset)
        } else {
            sqlx::query_as::<_, StreamWithCount>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message,
                       COUNT(*) OVER() as total_count
                FROM streams
                ORDER BY created_at DESC
                LIMIT ?1 OFFSET ?2
                "#,
            )
            .bind(limit)
            .bind(offset)
        };

        let results = query
            .fetch_all(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to list streams with total: {}", e)))?;

        if results.is_empty() {
            return Ok((vec![], 0));
        }

        let total = results[0].total_count;
        let streams = results.into_iter().map(|r| r.into()).collect();

        Ok((streams, total))
    }

    /// Search streams by name with total count and proper user filtering
    pub async fn search_with_total(
        &self,
        user_id: Option<&str>,
        name_pattern: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<Stream>, i64)> {
        let pattern = format!("%{}%", name_pattern);

        let query = if let Some(uid) = user_id {
            sqlx::query_as::<_, StreamWithCount>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message,
                       COUNT(*) OVER() as total_count
                FROM streams
                WHERE user_id = ?1 AND name LIKE ?2
                ORDER BY created_at DESC
                LIMIT ?3 OFFSET ?4
                "#,
            )
            .bind(uid)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
        } else {
            sqlx::query_as::<_, StreamWithCount>(
                r#"
                SELECT id, user_id, name, description, config, is_default, created_at, updated_at,
                       execution_status, last_executed_at, last_error_message,
                       COUNT(*) OVER() as total_count
                FROM streams
                WHERE name LIKE ?1
                ORDER BY created_at DESC
                LIMIT ?2 OFFSET ?3
                "#,
            )
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
        };

        let results = query
            .fetch_all(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to search streams with total: {}", e)))?;

        if results.is_empty() {
            return Ok((vec![], 0));
        }

        let total = results[0].total_count;
        let streams = results.into_iter().map(|r| r.into()).collect();

        Ok((streams, total))
    }
}

/// Helper struct for queries that return stream data with total count
#[derive(Debug, FromRow)]
struct StreamWithCount {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    pub config: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub execution_status: Option<String>,
    pub last_executed_at: Option<String>,
    pub last_error_message: Option<String>,
    pub total_count: i64,
}

impl From<StreamWithCount> for Stream {
    fn from(stream_with_count: StreamWithCount) -> Self {
        Self {
            id: stream_with_count.id,
            user_id: stream_with_count.user_id,
            name: stream_with_count.name,
            description: stream_with_count.description,
            config: stream_with_count.config,
            is_default: stream_with_count.is_default,
            created_at: stream_with_count.created_at,
            updated_at: stream_with_count.updated_at,
            execution_status: stream_with_count.execution_status,
            last_executed_at: stream_with_count.last_executed_at,
            last_error_message: stream_with_count.last_error_message,
        }
    }
}
