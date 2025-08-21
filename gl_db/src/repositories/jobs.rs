//! ABOUTME: Job repository for managing background processing and scheduling
//! ABOUTME: Provides compile-time checked queries for job CRUD operations

use gl_core::{Result, Error, time::now_iso8601, Id};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

/// Job entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Job {
    pub id: String,
    pub user_id: String,
    pub capture_id: Option<String>,
    pub job_type: String,
    pub priority: i64,
    pub status: String,
    pub payload: String, // JSON
    pub result: Option<String>, // JSON
    pub error_message: Option<String>,
    pub attempts: i64,
    pub max_attempts: i64,
    pub scheduled_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobRequest {
    pub user_id: String,
    pub capture_id: Option<String>,
    pub job_type: String,
    pub priority: i64,
    pub payload: String, // JSON
    pub max_attempts: i64,
    pub scheduled_at: String,
}

/// Request to update a job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateJobRequest {
    pub status: Option<String>,
    pub result: Option<String>,
    pub error_message: Option<String>,
    pub attempts: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Job repository
pub struct JobRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> JobRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new job
    pub async fn create(&self, request: CreateJobRequest) -> Result<Job> {
        let id = Id::new().to_string();
        let now = now_iso8601();
        
        let job = sqlx::query_as!(
            Job,
            r#"
            INSERT INTO jobs (id, user_id, capture_id, job_type, priority, status, payload, attempts, max_attempts, scheduled_at, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, 0, ?7, ?8, ?9, ?10)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.capture_id,
            request.job_type,
            request.priority,
            request.payload,
            request.max_attempts,
            request.scheduled_at,
            now,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create job: {}", e)))?;
        
        Ok(job)
    }

    /// Find job by ID
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Job>> {
        let job = sqlx::query_as!(
            Job,
            "SELECT * FROM jobs WHERE id = ?1",
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find job: {}", e)))?;
        
        Ok(job)
    }
}