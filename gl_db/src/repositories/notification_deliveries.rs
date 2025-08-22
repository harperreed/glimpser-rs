//! ABOUTME: Repository for tracking notification delivery status across different channels
//! ABOUTME: Handles retry logic, failure tracking, and delivery confirmations

use crate::Db;
use gl_core::{time::now_iso8601, Id, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Status of a notification delivery
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Sent,
    Delivered,
    Failed,
    Retry,
}

impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Retry => "retry",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "sent" => Self::Sent,
            "delivered" => Self::Delivered,
            "failed" => Self::Failed,
            "retry" => Self::Retry,
            _ => Self::Pending,
        }
    }
}

/// Notification delivery record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationDelivery {
    pub id: String,
    pub analysis_event_id: String,
    pub channel_type: String,
    pub channel_config: HashMap<String, serde_json::Value>,
    pub status: DeliveryStatus,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub scheduled_at: String,
    pub sent_at: Option<String>,
    pub delivered_at: Option<String>,
    pub failed_at: Option<String>,
    pub error_message: Option<String>,
    pub external_id: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub created_at: String,
    pub updated_at: String,
}

/// Request to create a new notification delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateNotificationDelivery {
    pub analysis_event_id: String,
    pub channel_type: String,
    pub channel_config: HashMap<String, serde_json::Value>,
    pub max_attempts: Option<i32>,
    pub scheduled_at: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Request to update delivery status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDeliveryStatus {
    pub status: DeliveryStatus,
    pub error_message: Option<String>,
    pub external_id: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

/// Repository for notification deliveries
#[derive(Clone)]
pub struct NotificationDeliveryRepository {
    db: Db,
}

impl NotificationDeliveryRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Create a new notification delivery record
    pub async fn create(
        &self,
        request: CreateNotificationDelivery,
    ) -> Result<NotificationDelivery> {
        let id = Id::new().to_string();
        let now = now_iso8601();
        let scheduled_at = request.scheduled_at.unwrap_or_else(|| now.clone());
        let max_attempts = request.max_attempts.unwrap_or(3);

        let channel_config_json = serde_json::to_string(&request.channel_config).map_err(|e| {
            gl_core::Error::Database(format!("Failed to serialize channel_config: {}", e))
        })?;

        let metadata_json = request
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()
            .map_err(|e| {
                gl_core::Error::Database(format!("Failed to serialize metadata: {}", e))
            })?;

        debug!(
            delivery_id = %id,
            event_id = %request.analysis_event_id,
            channel_type = %request.channel_type,
            scheduled_at = %scheduled_at,
            "Creating notification delivery"
        );

        sqlx::query(
            r#"
            INSERT INTO notification_deliveries (
                id, analysis_event_id, channel_type, channel_config, status, attempt_count,
                max_attempts, scheduled_at, metadata, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&request.analysis_event_id)
        .bind(&request.channel_type)
        .bind(&channel_config_json)
        .bind(DeliveryStatus::Pending.as_str())
        .bind(0)
        .bind(max_attempts)
        .bind(&scheduled_at)
        .bind(&metadata_json)
        .bind(&now)
        .bind(&now)
        .execute(&self.db.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to create notification delivery: {}", e))
        })?;

        Ok(NotificationDelivery {
            id,
            analysis_event_id: request.analysis_event_id,
            channel_type: request.channel_type,
            channel_config: request.channel_config,
            status: DeliveryStatus::Pending,
            attempt_count: 0,
            max_attempts,
            scheduled_at,
            sent_at: None,
            delivered_at: None,
            failed_at: None,
            error_message: None,
            external_id: None,
            metadata: request.metadata,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// Update delivery status
    pub async fn update_status(&self, id: &str, update: UpdateDeliveryStatus) -> Result<()> {
        let now = now_iso8601();

        let metadata_json = update
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m))
            .transpose()
            .map_err(|e| {
                gl_core::Error::Database(format!("Failed to serialize metadata: {}", e))
            })?;

        debug!(
            delivery_id = %id,
            status = %update.status.as_str(),
            "Updating notification delivery status"
        );

        let mut query =
            String::from("UPDATE notification_deliveries SET status = ?, updated_at = ?");
        let mut params = vec![update.status.as_str(), &now];

        // Set timestamp fields based on status
        match update.status {
            DeliveryStatus::Sent => {
                query.push_str(", sent_at = ?");
                params.push(&now);
            }
            DeliveryStatus::Delivered => {
                query.push_str(", delivered_at = ?");
                params.push(&now);
            }
            DeliveryStatus::Failed => {
                query.push_str(", failed_at = ?");
                params.push(&now);
            }
            _ => {}
        }

        // Increment attempt count for retry or failure
        if matches!(
            update.status,
            DeliveryStatus::Retry | DeliveryStatus::Failed
        ) {
            query.push_str(", attempt_count = attempt_count + 1");
        }

        // Add optional fields
        if update.error_message.is_some() {
            query.push_str(", error_message = ?");
        }
        if update.external_id.is_some() {
            query.push_str(", external_id = ?");
        }
        if metadata_json.is_some() {
            query.push_str(", metadata = ?");
        }

        query.push_str(" WHERE id = ?");

        let mut sql_query = sqlx::query(&query);
        for param in params {
            sql_query = sql_query.bind(param);
        }

        if let Some(ref error_msg) = update.error_message {
            sql_query = sql_query.bind(error_msg);
        }
        if let Some(ref ext_id) = update.external_id {
            sql_query = sql_query.bind(ext_id);
        }
        if let Some(ref metadata) = metadata_json {
            sql_query = sql_query.bind(metadata);
        }

        sql_query = sql_query.bind(id);

        let result = sql_query.execute(&self.db.pool).await.map_err(|e| {
            gl_core::Error::Database(format!("Failed to update delivery status: {}", e))
        })?;

        if result.rows_affected() == 0 {
            warn!(delivery_id = %id, "No notification delivery found to update");
        }

        Ok(())
    }

    /// Get pending deliveries that are due for processing
    pub async fn get_pending_deliveries(&self, limit: i64) -> Result<Vec<NotificationDelivery>> {
        let rows = sqlx::query(
            r#"
            SELECT id, analysis_event_id, channel_type, channel_config, status, attempt_count,
                   max_attempts, scheduled_at, sent_at, delivered_at, failed_at, error_message,
                   external_id, metadata, created_at, updated_at
            FROM notification_deliveries
            WHERE status IN ('pending', 'retry')
              AND scheduled_at <= datetime('now')
              AND attempt_count < max_attempts
            ORDER BY scheduled_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.db.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to get pending deliveries: {}", e))
        })?;

        let mut deliveries = Vec::new();
        for row in rows {
            deliveries.push(self.row_to_notification_delivery(row)?);
        }

        Ok(deliveries)
    }

    /// Get deliveries by analysis event ID
    pub async fn get_by_event_id(&self, event_id: &str) -> Result<Vec<NotificationDelivery>> {
        let rows = sqlx::query(
            r#"
            SELECT id, analysis_event_id, channel_type, channel_config, status, attempt_count,
                   max_attempts, scheduled_at, sent_at, delivered_at, failed_at, error_message,
                   external_id, metadata, created_at, updated_at
            FROM notification_deliveries
            WHERE analysis_event_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(event_id)
        .fetch_all(&self.db.pool)
        .await
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to get deliveries by event: {}", e))
        })?;

        let mut deliveries = Vec::new();
        for row in rows {
            deliveries.push(self.row_to_notification_delivery(row)?);
        }

        Ok(deliveries)
    }

    /// Schedule retry for failed delivery
    pub async fn schedule_retry(&self, id: &str, delay_minutes: i32) -> Result<()> {
        let scheduled_at = format!("datetime('now', '+{} minutes')", delay_minutes);

        debug!(
            delivery_id = %id,
            delay_minutes,
            "Scheduling delivery retry"
        );

        sqlx::query(&format!(
            r#"
                UPDATE notification_deliveries
                SET status = 'retry', scheduled_at = {}, updated_at = datetime('now')
                WHERE id = ? AND attempt_count < max_attempts
            "#,
            scheduled_at
        ))
        .bind(id)
        .execute(&self.db.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to schedule retry: {}", e)))?;

        Ok(())
    }

    /// Get delivery statistics
    pub async fn get_stats(&self, hours: i32) -> Result<HashMap<String, i64>> {
        let rows = sqlx::query(
            r#"
            SELECT status, COUNT(*) as count
            FROM notification_deliveries
            WHERE created_at >= datetime('now', '-' || ? || ' hours')
            GROUP BY status
            "#,
        )
        .bind(hours)
        .fetch_all(&self.db.pool)
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to get delivery stats: {}", e)))?;

        let mut stats = HashMap::new();
        for row in rows {
            let status: String = row
                .try_get("status")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get status: {}", e)))?;
            let count: i64 = row
                .try_get("count")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get count: {}", e)))?;
            stats.insert(status, count);
        }

        Ok(stats)
    }

    /// Convert database row to NotificationDelivery
    fn row_to_notification_delivery(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> Result<NotificationDelivery> {
        let channel_config_json: String = row.try_get("channel_config").map_err(|e| {
            gl_core::Error::Database(format!("Failed to get channel_config: {}", e))
        })?;
        let channel_config = serde_json::from_str(&channel_config_json).map_err(|e| {
            gl_core::Error::Database(format!("Failed to deserialize channel_config: {}", e))
        })?;

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

        let status_str: String = row
            .try_get("status")
            .map_err(|e| gl_core::Error::Database(format!("Failed to get status: {}", e)))?;

        Ok(NotificationDelivery {
            id: row
                .try_get("id")
                .map_err(|e| gl_core::Error::Database(format!("Failed to get id: {}", e)))?,
            analysis_event_id: row.try_get("analysis_event_id").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get analysis_event_id: {}", e))
            })?,
            channel_type: row.try_get("channel_type").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get channel_type: {}", e))
            })?,
            channel_config,
            status: DeliveryStatus::from_str(&status_str),
            attempt_count: row.try_get("attempt_count").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get attempt_count: {}", e))
            })?,
            max_attempts: row.try_get("max_attempts").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get max_attempts: {}", e))
            })?,
            scheduled_at: row.try_get("scheduled_at").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get scheduled_at: {}", e))
            })?,
            sent_at: row.try_get("sent_at").ok(),
            delivered_at: row.try_get("delivered_at").ok(),
            failed_at: row.try_get("failed_at").ok(),
            error_message: row.try_get("error_message").ok(),
            external_id: row.try_get("external_id").ok(),
            metadata,
            created_at: row.try_get("created_at").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get created_at: {}", e))
            })?,
            updated_at: row.try_get("updated_at").map_err(|e| {
                gl_core::Error::Database(format!("Failed to get updated_at: {}", e))
            })?,
        })
    }
}
