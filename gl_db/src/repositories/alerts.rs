//! ABOUTME: Alert repository for managing system notifications and user alerts
//! ABOUTME: Provides compile-time checked queries for alert CRUD operations

use gl_core::{time::now_iso8601, Error, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

/// Alert entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Alert {
    pub id: String,
    pub user_id: String,
    pub capture_id: Option<String>,
    pub alert_type: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub metadata: Option<String>, // JSON
    pub is_read: bool,
    pub is_dismissed: bool,
    pub triggered_at: String,
    pub read_at: Option<String>,
    pub dismissed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new alert
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAlertRequest {
    pub user_id: String,
    pub capture_id: Option<String>,
    pub alert_type: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub metadata: Option<String>, // JSON
    pub triggered_at: String,
}

/// Alert repository
pub struct AlertRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> AlertRepository<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new alert
    pub async fn create(&self, request: CreateAlertRequest) -> Result<Alert> {
        let id = Id::new().to_string();
        let now = now_iso8601();

        let alert = sqlx::query_as!(
            Alert,
            r#"
            INSERT INTO alerts (id, user_id, capture_id, alert_type, severity, title, message, metadata, is_read, is_dismissed, triggered_at, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, false, false, ?9, ?10, ?11)
            RETURNING *
            "#,
            id,
            request.user_id,
            request.capture_id,
            request.alert_type,
            request.severity,
            request.title,
            request.message,
            request.metadata,
            request.triggered_at,
            now,
            now
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| Error::Database(format!("Failed to create alert: {}", e)))?;

        Ok(alert)
    }

    /// Find alert by ID
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Alert>> {
        let alert = sqlx::query_as!(Alert, "SELECT * FROM alerts WHERE id = ?1", id)
            .fetch_optional(self.pool)
            .await
            .map_err(|e| Error::Database(format!("Failed to find alert: {}", e)))?;

        Ok(alert)
    }
}
