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

    pub async fn update(&self, id: &str, request: UpdateStreamRequest) -> Result<Option<Stream>> {
        let now = now_iso8601();

        // Fetch existing first
        let existing = match self.find_by_id(id).await? {
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
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to update stream: {}", e)))?;

        Ok(stream)
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
}
