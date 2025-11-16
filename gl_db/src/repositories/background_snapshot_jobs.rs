//! ABOUTME: Repository for background snapshot job tracking
//! ABOUTME: Provides database persistence for FFmpeg background processing jobs

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};

/// Background snapshot job status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundJobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for BackgroundJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            BackgroundJobStatus::Pending => "pending",
            BackgroundJobStatus::Processing => "processing",
            BackgroundJobStatus::Completed => "completed",
            BackgroundJobStatus::Failed => "failed",
            BackgroundJobStatus::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for BackgroundJobStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(BackgroundJobStatus::Pending),
            "processing" => Ok(BackgroundJobStatus::Processing),
            "completed" => Ok(BackgroundJobStatus::Completed),
            "failed" => Ok(BackgroundJobStatus::Failed),
            "cancelled" => Ok(BackgroundJobStatus::Cancelled),
            _ => Err(Error::Config(format!("Invalid job status: {}", s))),
        }
    }
}

/// Background snapshot job entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BackgroundSnapshotJob {
    pub id: String,
    pub input_path: String,
    pub stream_id: Option<String>,
    pub status: String,
    pub config: String, // JSON serialized SnapshotConfig
    pub result_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub created_by: Option<String>,
    pub metadata: Option<String>, // JSON
}

impl BackgroundSnapshotJob {
    /// Get the job status as enum
    pub fn status_enum(&self) -> Result<BackgroundJobStatus> {
        self.status.parse()
    }
}

/// Request to create a new background snapshot job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBackgroundJobRequest {
    pub input_path: String,
    pub stream_id: Option<String>,
    pub config: String, // JSON serialized SnapshotConfig
    pub created_by: Option<String>,
    pub metadata: Option<String>,
}

/// Request to update a background snapshot job
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateBackgroundJobRequest {
    pub status: Option<BackgroundJobStatus>,
    pub result_size: Option<i64>,
    pub error_message: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Repository for background snapshot jobs
pub struct BackgroundSnapshotJobsRepository;

impl BackgroundSnapshotJobsRepository {
    /// Create a new background snapshot job
    ///
    /// This method uses a transaction internally to ensure atomicity of the
    /// insert-then-select operation. If any step fails, the entire operation is
    /// rolled back to prevent orphaned records.
    pub async fn create(
        pool: &SqlitePool,
        request: CreateBackgroundJobRequest,
    ) -> Result<BackgroundSnapshotJob> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        // Use transaction to ensure atomicity of insert-then-select operation
        // Note: This creates a transaction directly rather than using Db::with_transaction()
        // because this static method only has access to SqlitePool, not the Db wrapper
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| Error::Database(format!("Failed to begin transaction: {}", e)))?;

        let _row = sqlx::query!(
            r#"
            INSERT INTO background_snapshot_jobs (
                id, input_path, stream_id, status, config,
                created_at, created_by, metadata
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
            id,
            request.input_path,
            request.stream_id,
            "pending",
            request.config,
            now,
            request.created_by,
            request.metadata
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::Database(format!("Failed to create background snapshot job: {}", e)))?;

        // Get the created job within same transaction
        let job = sqlx::query!(
            r#"
            SELECT
                id,
                input_path,
                stream_id,
                status,
                config,
                result_size,
                error_message,
                created_at,
                started_at,
                completed_at,
                duration_ms,
                created_by,
                metadata
            FROM background_snapshot_jobs
            WHERE id = ?
            "#,
            id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to get created background snapshot job: {}",
                e
            ))
        })?;

        let job = BackgroundSnapshotJob {
            id: job.id.unwrap(),
            input_path: job.input_path,
            stream_id: job.stream_id,
            status: job.status,
            config: job.config,
            result_size: job.result_size,
            error_message: job.error_message,
            created_at: job.created_at,
            started_at: job.started_at,
            completed_at: job.completed_at,
            duration_ms: job.duration_ms,
            created_by: job.created_by,
            metadata: job.metadata,
        };

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| Error::Database(format!("Failed to commit transaction: {}", e)))?;

        Ok(job)
    }

    /// Get a background snapshot job by ID
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<BackgroundSnapshotJob>> {
        let job = sqlx::query!(
            r#"
            SELECT
                id,
                input_path,
                stream_id,
                status,
                config,
                result_size,
                error_message,
                created_at,
                started_at,
                completed_at,
                duration_ms,
                created_by,
                metadata
            FROM background_snapshot_jobs
            WHERE id = ?
            "#,
            id
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to get background snapshot job: {}", e)))?;

        Ok(job.map(|row| BackgroundSnapshotJob {
            id: row.id.unwrap(),
            input_path: row.input_path,
            stream_id: row.stream_id,
            status: row.status,
            config: row.config,
            result_size: row.result_size,
            error_message: row.error_message,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            duration_ms: row.duration_ms,
            created_by: row.created_by,
            metadata: row.metadata,
        }))
    }

    /// Update a background snapshot job
    pub async fn update(
        pool: &SqlitePool,
        id: &str,
        request: UpdateBackgroundJobRequest,
    ) -> Result<()> {
        let mut query = String::from("UPDATE background_snapshot_jobs SET ");
        let mut params = Vec::new();
        let mut set_clauses = Vec::new();

        if let Some(status) = request.status {
            set_clauses.push("status = ?");
            params.push(status.to_string());
        }

        if let Some(result_size) = request.result_size {
            set_clauses.push("result_size = ?");
            params.push(result_size.to_string());
        }

        if let Some(ref error_message) = request.error_message {
            set_clauses.push("error_message = ?");
            params.push(error_message.clone());
        }

        if let Some(ref started_at) = request.started_at {
            set_clauses.push("started_at = ?");
            params.push(started_at.clone());
        }

        if let Some(ref completed_at) = request.completed_at {
            set_clauses.push("completed_at = ?");
            params.push(completed_at.clone());
        }

        if let Some(duration_ms) = request.duration_ms {
            set_clauses.push("duration_ms = ?");
            params.push(duration_ms.to_string());
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        query.push_str(&set_clauses.join(", "));
        query.push_str(" WHERE id = ?");
        params.push(id.to_string());

        let mut query_builder = sqlx::query(&query);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        query_builder.execute(pool).await.map_err(|e| {
            Error::Database(format!("Failed to update background snapshot job: {}", e))
        })?;

        Ok(())
    }

    /// List background snapshot jobs with optional filters
    pub async fn list(
        pool: &SqlitePool,
        status: Option<BackgroundJobStatus>,
        stream_id: Option<&str>,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Vec<BackgroundSnapshotJob>> {
        let mut query = String::from("SELECT id, input_path, stream_id, status, config, result_size, error_message, created_at, started_at, completed_at, duration_ms, created_by, metadata FROM background_snapshot_jobs WHERE 1=1");
        let mut params = Vec::new();

        if let Some(status) = status {
            query.push_str(" AND status = ?");
            params.push(status.to_string());
        }

        if let Some(stream_id) = stream_id {
            query.push_str(" AND stream_id = ?");
            params.push(stream_id.to_string());
        }

        query.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = limit {
            query.push_str(" LIMIT ?");
            params.push(limit.to_string());

            if let Some(offset) = offset {
                query.push_str(" OFFSET ?");
                params.push(offset.to_string());
            }
        }

        let mut query_builder = sqlx::query(&query);
        for param in params {
            query_builder = query_builder.bind(param);
        }

        let rows = query_builder.fetch_all(pool).await.map_err(|e| {
            Error::Database(format!("Failed to list background snapshot jobs: {}", e))
        })?;

        let jobs = rows
            .into_iter()
            .map(|row| BackgroundSnapshotJob {
                id: row.get("id"),
                input_path: row.get("input_path"),
                stream_id: row.get("stream_id"),
                status: row.get("status"),
                config: row.get("config"),
                result_size: row.get("result_size"),
                error_message: row.get("error_message"),
                created_at: row.get("created_at"),
                started_at: row.get("started_at"),
                completed_at: row.get("completed_at"),
                duration_ms: row.get("duration_ms"),
                created_by: row.get("created_by"),
                metadata: row.get("metadata"),
            })
            .collect();

        Ok(jobs)
    }

    /// Delete old completed jobs (cleanup)
    pub async fn cleanup_old_jobs(
        pool: &SqlitePool,
        older_than: &str,
        max_jobs_to_delete: i32,
    ) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM background_snapshot_jobs
            WHERE id IN (
                SELECT id FROM background_snapshot_jobs
                WHERE status IN ('completed', 'failed', 'cancelled')
                AND completed_at < ?
                ORDER BY completed_at ASC
                LIMIT ?
            )
            "#,
            older_than,
            max_jobs_to_delete
        )
        .execute(pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to cleanup old background snapshot jobs: {}",
                e
            ))
        })?;

        Ok(result.rows_affected())
    }

    /// Get job count by status
    pub async fn count_by_status(
        pool: &SqlitePool,
    ) -> Result<std::collections::HashMap<String, i64>> {
        let rows = sqlx::query!(
            r#"
            SELECT status, COUNT(*) as count
            FROM background_snapshot_jobs
            GROUP BY status
            "#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| {
            Error::Database(format!(
                "Failed to count background snapshot jobs by status: {}",
                e
            ))
        })?;

        let mut counts = std::collections::HashMap::new();
        for row in rows {
            counts.insert(row.status, row.count);
        }

        Ok(counts)
    }
}
