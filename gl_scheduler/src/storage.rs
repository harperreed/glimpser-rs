//! ABOUTME: Storage layer for persisting job definitions and execution history
//! ABOUTME: Provides database operations for job scheduling system

use crate::{types::*, JobResult, JobStatus};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gl_core::Result;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Trait for job storage operations
#[async_trait]
pub trait JobStorage: Send + Sync {
    /// Save a job definition
    async fn save_job(&self, job: &JobDefinition) -> Result<()>;

    /// Get a job definition by ID
    async fn get_job(&self, job_id: &str) -> Result<Option<JobDefinition>>;

    /// List all job definitions
    async fn list_jobs(&self) -> Result<Vec<JobDefinition>>;

    /// List jobs with filtering options
    async fn list_jobs_filtered(
        &self,
        enabled_only: bool,
        job_type: Option<&str>,
        tags: Option<&[String]>,
        limit: Option<u32>,
    ) -> Result<Vec<JobDefinition>>;

    /// Update a job definition
    async fn update_job(&self, job: &JobDefinition) -> Result<()>;

    /// Delete a job definition
    async fn delete_job(&self, job_id: &str) -> Result<()>;

    /// Save job execution result
    async fn save_job_result(&self, execution_id: &str, result: &JobResult) -> Result<()>;

    /// Get job execution results for a specific job
    async fn get_job_results(&self, job_id: &str, limit: Option<u32>) -> Result<Vec<JobResult>>;

    /// Get job execution result by execution ID
    async fn get_job_result(&self, execution_id: &str) -> Result<Option<JobResult>>;

    /// Get job queue statistics
    async fn get_queue_stats(&self) -> Result<JobQueueStats>;

    /// Cleanup old job results based on retention policy
    async fn cleanup_old_results(&self, retention_days: u32) -> Result<u64>;
}

/// SQLite implementation of job storage
pub struct SqliteJobStorage {
    pool: SqlitePool,
}

impl SqliteJobStorage {
    /// Create a new SQLite job storage
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Initialize database tables
    pub async fn migrate(&self) -> Result<()> {
        debug!("Running job scheduler database migrations");

        // Jobs table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS scheduled_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                job_type TEXT NOT NULL,
                schedule TEXT NOT NULL,
                parameters TEXT NOT NULL, -- JSON
                enabled INTEGER NOT NULL DEFAULT 1,
                max_retries INTEGER NOT NULL DEFAULT 3,
                timeout_seconds INTEGER,
                priority INTEGER NOT NULL DEFAULT 0,
                tags TEXT, -- JSON array
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                metadata TEXT -- JSON object
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to create jobs table: {}", e)))?;

        // Job executions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS job_executions (
                id TEXT PRIMARY KEY,
                job_id TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                completed_at TEXT,
                duration_ms INTEGER,
                result TEXT, -- JSON
                error TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                executed_on TEXT,
                metadata TEXT, -- JSON
                FOREIGN KEY (job_id) REFERENCES scheduled_jobs (id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to create executions table: {}", e))
        })?;

        // Create indexes for better performance
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_enabled ON scheduled_jobs (enabled)")
            .execute(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to create index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_jobs_type ON scheduled_jobs (job_type)")
            .execute(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to create index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_executions_job_id ON job_executions (job_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to create index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_executions_status ON job_executions (status)")
            .execute(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to create index: {}", e)))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_executions_started_at ON job_executions (started_at)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to create index: {}", e)))?;

        debug!("Job scheduler database migration completed");
        Ok(())
    }
}

#[async_trait]
impl JobStorage for SqliteJobStorage {
    async fn save_job(&self, job: &JobDefinition) -> Result<()> {
        debug!("Saving job definition: {}", job.id);

        let tags_json = serde_json::to_string(&job.tags)
            .map_err(|e| gl_core::Error::Validation(format!("Failed to serialize tags: {}", e)))?;

        let parameters_json = serde_json::to_string(&job.parameters).map_err(|e| {
            gl_core::Error::Validation(format!("Failed to serialize parameters: {}", e))
        })?;

        let metadata_json = serde_json::to_string(&job.metadata).map_err(|e| {
            gl_core::Error::Validation(format!("Failed to serialize metadata: {}", e))
        })?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO scheduled_jobs (
                id, name, description, job_type, schedule, parameters,
                enabled, max_retries, timeout_seconds, priority, tags,
                created_by, created_at, updated_at, metadata
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&job.id)
        .bind(&job.name)
        .bind(&job.description)
        .bind(&job.job_type)
        .bind(&job.schedule)
        .bind(&parameters_json)
        .bind(job.enabled as i32)
        .bind(job.max_retries as i32)
        .bind(job.timeout_seconds.map(|t| t as i64))
        .bind(job.priority)
        .bind(&tags_json)
        .bind(&job.created_by)
        .bind(job.created_at.to_rfc3339())
        .bind(job.updated_at.to_rfc3339())
        .bind(&metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to save job: {}", e)))?;

        debug!("Successfully saved job: {}", job.id);
        Ok(())
    }

    async fn get_job(&self, job_id: &str) -> Result<Option<JobDefinition>> {
        debug!("Getting job definition: {}", job_id);

        let row = sqlx::query("SELECT * FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to get job: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(self.row_to_job_definition(row)?))
        } else {
            Ok(None)
        }
    }

    async fn list_jobs(&self) -> Result<Vec<JobDefinition>> {
        debug!("Listing all job definitions");

        let rows = sqlx::query("SELECT * FROM scheduled_jobs ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to list jobs: {}", e)))?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(self.row_to_job_definition(row)?);
        }

        debug!("Found {} job definitions", jobs.len());
        Ok(jobs)
    }

    async fn list_jobs_filtered(
        &self,
        enabled_only: bool,
        job_type: Option<&str>,
        tags: Option<&[String]>,
        limit: Option<u32>,
    ) -> Result<Vec<JobDefinition>> {
        debug!(
            "Listing filtered jobs: enabled_only={}, job_type={:?}, tags={:?}, limit={:?}",
            enabled_only, job_type, tags, limit
        );

        let mut query = "SELECT * FROM scheduled_jobs WHERE 1=1".to_string();
        let mut params: Vec<String> = Vec::new();

        if enabled_only {
            query.push_str(" AND enabled = 1");
        }

        if let Some(job_type) = job_type {
            query.push_str(" AND job_type = ?");
            params.push(job_type.to_string());
        }

        // Note: Tag filtering would require more complex JSON queries in SQLite
        // For now, we'll filter in memory if tags are specified

        query.push_str(" ORDER BY priority DESC, created_at DESC");

        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }

        let mut db_query = sqlx::query(&query);
        for param in &params {
            db_query = db_query.bind(param);
        }

        let rows = db_query.fetch_all(&self.pool).await.map_err(|e| {
            gl_core::Error::Database(format!("Failed to list filtered jobs: {}", e))
        })?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(self.row_to_job_definition(row)?);
        }

        // Filter by tags in memory if specified
        if let Some(filter_tags) = tags {
            jobs.retain(|job| filter_tags.iter().any(|tag| job.tags.contains(tag)));
        }

        debug!("Found {} filtered job definitions", jobs.len());
        Ok(jobs)
    }

    async fn update_job(&self, job: &JobDefinition) -> Result<()> {
        debug!("Updating job definition: {}", job.id);

        // Update the updated_at timestamp
        let mut updated_job = job.clone();
        updated_job.updated_at = Utc::now();

        self.save_job(&updated_job).await
    }

    async fn delete_job(&self, job_id: &str) -> Result<()> {
        debug!("Deleting job definition: {}", job_id);

        let result = sqlx::query("DELETE FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to delete job: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(gl_core::Error::NotFound(format!(
                "Job not found: {}",
                job_id
            )));
        }

        debug!("Successfully deleted job: {}", job_id);
        Ok(())
    }

    async fn save_job_result(&self, execution_id: &str, result: &JobResult) -> Result<()> {
        debug!("Saving job result for execution: {}", execution_id);

        let result_json = result
            .output
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                gl_core::Error::Validation(format!("Failed to serialize result: {}", e))
            })?;

        // For now, we'll create a simplified execution record
        // In a real implementation, we'd need to track the job_id properly
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO job_executions (
                id, job_id, status, started_at, completed_at,
                duration_ms, result, error, retry_count
            ) VALUES (?, 'unknown', ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(execution_id)
        .bind(result.status.as_str())
        .bind(result.started_at.to_rfc3339())
        .bind(result.completed_at.map(|t| t.to_rfc3339()))
        .bind(result.duration_ms.map(|d| d as i64))
        .bind(result_json)
        .bind(&result.error)
        .bind(result.retry_count as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to save job result: {}", e)))?;

        debug!(
            "Successfully saved job result for execution: {}",
            execution_id
        );
        Ok(())
    }

    async fn get_job_results(&self, job_id: &str, limit: Option<u32>) -> Result<Vec<JobResult>> {
        debug!("Getting job results for job: {}", job_id);

        let limit_clause = limit.map_or_else(String::new, |l| format!(" LIMIT {}", l));
        let query = format!(
            "SELECT * FROM job_executions WHERE job_id = ? ORDER BY started_at DESC{}",
            limit_clause
        );

        let rows = sqlx::query(&query)
            .bind(job_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to get job results: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(self.row_to_job_result(row)?);
        }

        debug!("Found {} job results for job: {}", results.len(), job_id);
        Ok(results)
    }

    async fn get_job_result(&self, execution_id: &str) -> Result<Option<JobResult>> {
        debug!("Getting job result for execution: {}", execution_id);

        let row = sqlx::query("SELECT * FROM job_executions WHERE id = ?")
            .bind(execution_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to get job result: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(self.row_to_job_result(row)?))
        } else {
            Ok(None)
        }
    }

    async fn get_queue_stats(&self) -> Result<JobQueueStats> {
        debug!("Getting job queue statistics");

        // This is a simplified implementation
        // In practice, we'd calculate more detailed statistics
        let mut stats = JobQueueStats::new();

        // Count completed jobs today
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let completed_row = sqlx::query(
            "SELECT COUNT(*) as count FROM job_executions WHERE status = 'completed' AND DATE(started_at) = ?",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to get queue stats: {}", e)))?;

        stats.completed_today = completed_row.get::<i64, _>("count") as u64;

        // Count failed jobs today
        let failed_row = sqlx::query(
            "SELECT COUNT(*) as count FROM job_executions WHERE status = 'failed' AND DATE(started_at) = ?",
        )
        .bind(&today)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to get queue stats: {}", e)))?;

        stats.failed_today = failed_row.get::<i64, _>("count") as u64;

        debug!(
            "Queue stats: completed={}, failed={}",
            stats.completed_today, stats.failed_today
        );
        Ok(stats)
    }

    async fn cleanup_old_results(&self, retention_days: u32) -> Result<u64> {
        debug!("Cleaning up job results older than {} days", retention_days);

        let cutoff_date = Utc::now() - chrono::Duration::days(retention_days as i64);

        let result = sqlx::query("DELETE FROM job_executions WHERE started_at < ?")
            .bind(cutoff_date.to_rfc3339())
            .execute(&self.pool)
            .await
            .map_err(|e| {
                gl_core::Error::Database(format!("Failed to cleanup old results: {}", e))
            })?;

        let deleted_count = result.rows_affected();
        debug!("Cleaned up {} old job execution records", deleted_count);

        Ok(deleted_count)
    }
}

impl SqliteJobStorage {
    /// Convert database row to JobDefinition
    fn row_to_job_definition(&self, row: sqlx::sqlite::SqliteRow) -> Result<JobDefinition> {
        let parameters_str: String = row.get("parameters");
        let parameters = serde_json::from_str(&parameters_str).map_err(|e| {
            gl_core::Error::Validation(format!("Failed to parse parameters: {}", e))
        })?;

        let tags_str: String = row.get("tags");
        let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_else(|_| Vec::new());

        let metadata_str: String = row.get("metadata");
        let metadata: HashMap<String, String> =
            serde_json::from_str(&metadata_str).unwrap_or_else(|_| HashMap::new());

        let created_at_str: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map_err(|e| {
                gl_core::Error::Validation(format!("Invalid created_at timestamp: {}", e))
            })?
            .with_timezone(&Utc);

        let updated_at_str: String = row.get("updated_at");
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map_err(|e| {
                gl_core::Error::Validation(format!("Invalid updated_at timestamp: {}", e))
            })?
            .with_timezone(&Utc);

        Ok(JobDefinition {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            job_type: row.get("job_type"),
            schedule: row.get("schedule"),
            parameters,
            enabled: row.get::<i32, _>("enabled") != 0,
            max_retries: row.get::<i32, _>("max_retries") as u32,
            timeout_seconds: row
                .get::<Option<i64>, _>("timeout_seconds")
                .map(|t| t as u64),
            priority: row.get("priority"),
            tags,
            created_by: row.get("created_by"),
            created_at,
            updated_at,
            metadata,
        })
    }

    /// Convert database row to JobResult
    fn row_to_job_result(&self, row: sqlx::sqlite::SqliteRow) -> Result<JobResult> {
        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "pending" => JobStatus::Pending,
            "running" => JobStatus::Running,
            "completed" => JobStatus::Completed,
            "failed" => JobStatus::Failed,
            "cancelled" => JobStatus::Cancelled,
            "timed_out" => JobStatus::TimedOut,
            "retried" => JobStatus::Retried,
            _ => {
                warn!("Unknown job status: {}, defaulting to Failed", status_str);
                JobStatus::Failed
            }
        };

        let started_at_str: String = row.get("started_at");
        let started_at = DateTime::parse_from_rfc3339(&started_at_str)
            .map_err(|e| {
                gl_core::Error::Validation(format!("Invalid started_at timestamp: {}", e))
            })?
            .with_timezone(&Utc);

        let completed_at = row
            .get::<Option<String>, _>("completed_at")
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| {
                gl_core::Error::Validation(format!("Invalid completed_at timestamp: {}", e))
            })?;

        let output = row
            .get::<Option<String>, _>("result")
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| gl_core::Error::Validation(format!("Failed to parse result: {}", e)))?;

        Ok(JobResult {
            status,
            started_at,
            completed_at,
            duration_ms: row.get::<Option<i64>, _>("duration_ms").map(|d| d as u64),
            output,
            error: row.get("error"),
            retry_count: row.get::<i32, _>("retry_count") as u32,
        })
    }
}
