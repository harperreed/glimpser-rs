//! ABOUTME: Capture repository for managing capture metadata and results  
//! ABOUTME: Provides compile-time checked queries for capture CRUD operations

use gl_core::{Result, Error, time::now_iso8601, Id};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

/// Capture entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Capture {
    pub id: String,
    pub user_id: String,
    pub template_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub source_url: String,
    pub status: String,
    pub config: String, // JSON
    pub metadata: Option<String>, // JSON
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCaptureRequest {
    pub user_id: String,
    pub template_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub source_url: String,
    pub config: String, // JSON
}

/// Request to update a capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCaptureRequest {
    pub status: Option<String>,
    pub metadata: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Capture repository
pub struct CaptureRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> CaptureRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new capture
    pub async fn create(&self, request: CreateCaptureRequest) -> Result<Capture> {
        let id = Id::new().to_string();
        let now = now_iso8601();
        
        let capture = sqlx::query_as!(
            Capture,
            r#"
            INSERT INTO captures (id, user_id, template_id, name, description, source_url, status, config, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.template_id,
            request.name,
            request.description,
            request.source_url,
            request.config,
            now,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create capture: {}", e)))?;
        
        Ok(capture)
    }

    /// Find capture by ID
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Capture>> {
        let capture = sqlx::query_as!(
            Capture,
            "SELECT * FROM captures WHERE id = ?1",
            id
        )
        .fetch_optional(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to find capture: {}", e)))?;
        
        Ok(capture)
    }
}