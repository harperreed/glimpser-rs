//! ABOUTME: Repository for analysis events from motion detection, AI analysis, and rule engine
//! ABOUTME: Handles CRUD operations for events that may trigger notifications

use crate::Db;
use gl_core::{time::now_iso8601, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;
use tracing::debug;

/// Analysis event from the processing pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisEvent {
    pub id: String,
    pub template_id: String,
    pub event_type: String,
    pub severity: String,
    pub confidence: f64,
    pub description: String,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub processor_name: String,
    pub source_id: String,
    pub should_notify: bool,
    pub suggested_actions: Option<Vec<String>>,
    pub created_at: String,
}

/// Request to create a new analysis event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAnalysisEvent {
    pub template_id: String,
    pub event_type: String,
    pub severity: String,
    pub confidence: f64,
    pub description: String,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub processor_name: String,
    pub source_id: String,
    pub should_notify: bool,
    pub suggested_actions: Option<Vec<String>>,
}

/// Repository for analysis events
#[derive(Clone)]
pub struct AnalysisEventRepository {
    db: Db,
}

impl AnalysisEventRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Create a new analysis event
    pub async fn create(&self, request: CreateAnalysisEvent) -> Result<AnalysisEvent> {
        let id = Id::new().to_string();
        let created_at = now_iso8601();

        let metadata_json = request
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                gl_core::Error::Database(format!("Failed to serialize metadata: {}", e))
            })?;

        let suggested_actions_json = request
            .suggested_actions
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| {
                gl_core::Error::Database(format!("Failed to serialize suggested_actions: {}", e))
            })?;

        debug!(
            event_id = %id,
            template_id = %request.template_id,
            event_type = %request.event_type,
            severity = %request.severity,
            should_notify = request.should_notify,
            "Creating analysis event"
        );

        sqlx::query(
            r#"
            INSERT INTO analysis_events (
                id, template_id, event_type, severity, confidence, description,
                metadata, processor_name, source_id, should_notify, suggested_actions, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&request.template_id)
        .bind(&request.event_type)
        .bind(&request.severity)
        .bind(request.confidence)
        .bind(&request.description)
        .bind(&metadata_json)
        .bind(&request.processor_name)
        .bind(&request.source_id)
        .bind(request.should_notify)
        .bind(&suggested_actions_json)
        .bind(&created_at)
        .execute(&self.db.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to create analysis event: {}", e)))?;

        Ok(AnalysisEvent {
            id,
            template_id: request.template_id,
            event_type: request.event_type,
            severity: request.severity,
            confidence: request.confidence,
            description: request.description,
            metadata: request.metadata,
            processor_name: request.processor_name,
            source_id: request.source_id,
            should_notify: request.should_notify,
            suggested_actions: request.suggested_actions,
            created_at,
        })
    }

    /// Get analysis event by ID
    pub async fn get_by_id(&self, id: &str) -> Result<Option<AnalysisEvent>> {
        let row = sqlx::query(
            r#"
            SELECT id, template_id, event_type, severity, confidence, description,
                   metadata, processor_name, source_id, should_notify, suggested_actions, created_at
            FROM analysis_events
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.db.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to get analysis event: {}", e)))?;

        if let Some(row) = row {
            Ok(Some(self.row_to_analysis_event(row)?))
        } else {
            Ok(None)
        }
    }

    /// List analysis events with pagination
    pub async fn list(
        &self,
        template_id: Option<&str>,
        event_type: Option<&str>,
        severity: Option<&str>,
        should_notify: Option<bool>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AnalysisEvent>> {
        let mut query = String::from(
            r#"
            SELECT id, template_id, event_type, severity, confidence, description,
                   metadata, processor_name, source_id, should_notify, suggested_actions, created_at
            FROM analysis_events
            WHERE 1=1
            "#,
        );

        let mut params = Vec::new();

        if let Some(tid) = template_id {
            query.push_str(" AND template_id = ?");
            params.push(tid);
        }

        if let Some(et) = event_type {
            query.push_str(" AND event_type = ?");
            params.push(et);
        }

        if let Some(sev) = severity {
            query.push_str(" AND severity = ?");
            params.push(sev);
        }

        if let Some(notify) = should_notify {
            query.push_str(" AND should_notify = ?");
            params.push(if notify { "true" } else { "false" });
        }

        query.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");

        let mut sql_query = sqlx::query(&query);
        for param in params {
            sql_query = sql_query.bind(param);
        }
        sql_query = sql_query.bind(limit).bind(offset);

        let rows = sql_query.fetch_all(&self.db.pool).await.map_err(|e| {
            gl_core::Error::Database(format!("Failed to list analysis events: {}", e))
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(self.row_to_analysis_event(row)?);
        }

        Ok(events)
    }

    /// Get pending notification events (should_notify = true, ordered by severity and time)
    pub async fn get_pending_notifications(&self, limit: i64) -> Result<Vec<AnalysisEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, template_id, event_type, severity, confidence, description,
                   metadata, processor_name, source_id, should_notify, suggested_actions, created_at
            FROM analysis_events
            WHERE should_notify = true
            ORDER BY
                CASE severity
                    WHEN 'critical' THEN 1
                    WHEN 'high' THEN 2
                    WHEN 'medium' THEN 3
                    WHEN 'low' THEN 4
                    WHEN 'info' THEN 5
                    ELSE 6
                END,
                created_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.db.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to get pending notifications: {}", e))
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(self.row_to_analysis_event(row)?);
        }

        Ok(events)
    }

    /// Delete old analysis events (for cleanup)
    pub async fn delete_older_than(&self, days: u32) -> Result<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM analysis_events
            WHERE created_at < datetime('now', '-' || ? || ' days')
            "#,
        )
        .bind(days)
        .execute(&self.db.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to delete old analysis events: {}", e))
        })?;

        debug!(
            rows_deleted = result.rows_affected(),
            "Deleted old analysis events"
        );
        Ok(result.rows_affected())
    }

    /// Convert database row to AnalysisEvent
    fn row_to_analysis_event(&self, row: sqlx::sqlite::SqliteRow) -> Result<AnalysisEvent> {
        let metadata_json: Option<String> = row
            .try_get("metadata")
            .map_err(|e| gl_core::Error::Database(format!("Failed to get metadata: {}", e)))?;
        let metadata = if let Some(json) = metadata_json {
            Some(serde_json::from_str(&json).map_err(|e| {
                gl_core::Error::Database(format!("Failed to deserialize metadata: {}", e))
            })?)
        } else {
            None
        };

        let suggested_actions_json: Option<String> =
            row.try_get("suggested_actions").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get suggested_actions: {}", e))
            })?;
        let suggested_actions = if let Some(json) = suggested_actions_json {
            Some(serde_json::from_str(&json).map_err(|e| {
                gl_core::Error::Database(format!("Failed to deserialize suggested_actions: {}", e))
            })?)
        } else {
            None
        };

        Ok(AnalysisEvent {
            id: row
                .try_get("id")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get id: {}", e)))?,
            template_id: row.try_get("template_id").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get template_id: {}", e))
            })?,
            event_type: row.try_get("event_type").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get event_type: {}", e))
            })?,
            severity: row
                .try_get("severity")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get severity: {}", e)))?,
            confidence: row.try_get("confidence").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get confidence: {}", e))
            })?,
            description: row.try_get("description").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get description: {}", e))
            })?,
            metadata,
            processor_name: row.try_get("processor_name").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get processor_name: {}", e))
            })?,
            source_id: row
                .try_get("source_id")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get source_id: {}", e)))?,
            should_notify: row.try_get("should_notify").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get should_notify: {}", e))
            })?,
            suggested_actions,
            created_at: row.try_get("created_at").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get created_at: {}", e))
            })?,
        })
    }
}
