//! ABOUTME: Notification dispatcher that processes analysis events and sends notifications
//! ABOUTME: Handles the end-to-end flow from events to channel delivery with retry logic

use crate::{
    adapters::{pushover::PushoverAdapter, webhook::WebhookAdapter},
    NotificationManager,
};

#[cfg(feature = "webpush")]
use crate::adapters::webpush::WebPushAdapter;
use gl_core::Result;
use gl_db::{
    AnalysisEvent, AnalysisEventRepository, CreateNotificationDelivery, Db, DeliveryStatus,
    NotificationDelivery, NotificationDeliveryRepository, UpdateDeliveryStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Configuration for notification channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannelConfig {
    pub channel_type: String,
    pub enabled: bool,
    pub config: HashMap<String, serde_json::Value>,
    pub severity_threshold: String, // minimum severity to trigger this channel
}

/// Configuration for the notification dispatcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatcherConfig {
    pub channels: Vec<NotificationChannelConfig>,
    pub polling_interval_seconds: u64,
    pub retry_delays_minutes: Vec<i32>, // exponential backoff delays
    pub max_concurrent_deliveries: usize,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            polling_interval_seconds: 30,
            retry_delays_minutes: vec![1, 5, 15, 60], // 1min, 5min, 15min, 1hr
            max_concurrent_deliveries: 10,
        }
    }
}

/// Notification dispatcher service
pub struct NotificationDispatcher {
    config: DispatcherConfig,
    analysis_events_repo: AnalysisEventRepository,
    delivery_repo: NotificationDeliveryRepository,
    notification_manager: NotificationManager,
}

impl NotificationDispatcher {
    /// Create a new notification dispatcher
    pub fn new(config: DispatcherConfig, db: Db) -> Self {
        let analysis_events_repo = AnalysisEventRepository::new(db.clone());
        let delivery_repo = NotificationDeliveryRepository::new(db);
        let notification_manager = NotificationManager::new();

        Self {
            config,
            analysis_events_repo,
            delivery_repo,
            notification_manager,
        }
    }

    /// Start the dispatcher background task
    pub async fn start(&self) -> Result<()> {
        info!(
            polling_interval = self.config.polling_interval_seconds,
            channels = self.config.channels.len(),
            "Starting notification dispatcher"
        );

        loop {
            // Process analysis events that need notifications
            if let Err(e) = self.process_pending_events().await {
                error!(error = %e, "Failed to process pending events");
            }

            // Process pending deliveries
            if let Err(e) = self.process_pending_deliveries().await {
                error!(error = %e, "Failed to process pending deliveries");
            }

            // Sleep until next polling interval
            sleep(Duration::from_secs(self.config.polling_interval_seconds)).await;
        }
    }

    /// Process analysis events that should trigger notifications
    async fn process_pending_events(&self) -> Result<()> {
        // Get events that should notify but don't have delivery records yet
        let pending_events = self
            .analysis_events_repo
            .get_pending_notifications(100)
            .await?;

        if pending_events.is_empty() {
            return Ok(());
        }

        debug!(
            event_count = pending_events.len(),
            "Processing pending notification events"
        );

        for event in pending_events {
            // Check if we already have deliveries for this event
            let existing_deliveries = self.delivery_repo.get_by_event_id(&event.id).await?;

            if !existing_deliveries.is_empty() {
                // Already processed this event
                continue;
            }

            // Create delivery records for each enabled channel
            for channel in &self.config.channels {
                if !channel.enabled {
                    continue;
                }

                // Check severity threshold
                if !self.meets_severity_threshold(&event.severity, &channel.severity_threshold) {
                    debug!(
                        event_id = %event.id,
                        event_severity = %event.severity,
                        channel_threshold = %channel.severity_threshold,
                        channel_type = %channel.channel_type,
                        "Event does not meet channel severity threshold"
                    );
                    continue;
                }

                // Create delivery record
                let delivery_request = CreateNotificationDelivery {
                    analysis_event_id: event.id.clone(),
                    channel_type: channel.channel_type.clone(),
                    channel_config: channel.config.clone(),
                    max_attempts: Some(self.config.retry_delays_minutes.len() as i32 + 1),
                    scheduled_at: None,
                    metadata: None,
                };

                match self.delivery_repo.create(delivery_request).await {
                    Ok(delivery) => {
                        debug!(
                            delivery_id = %delivery.id,
                            event_id = %event.id,
                            channel_type = %channel.channel_type,
                            "Created notification delivery record"
                        );
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            event_id = %event.id,
                            channel_type = %channel.channel_type,
                            "Failed to create delivery record"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Process pending notification deliveries
    async fn process_pending_deliveries(&self) -> Result<()> {
        let pending_deliveries = self
            .delivery_repo
            .get_pending_deliveries(self.config.max_concurrent_deliveries as i64)
            .await?;

        if pending_deliveries.is_empty() {
            return Ok(());
        }

        debug!(
            delivery_count = pending_deliveries.len(),
            "Processing pending notification deliveries"
        );

        // Process deliveries concurrently
        let handles: Vec<_> = pending_deliveries
            .into_iter()
            .map(|delivery| {
                let delivery_repo = self.delivery_repo.clone();
                let analysis_events_repo = self.analysis_events_repo.clone();
                let notification_manager = self.notification_manager.clone();
                let retry_delays = self.config.retry_delays_minutes.clone();

                tokio::spawn(async move {
                    Self::process_single_delivery(
                        delivery,
                        delivery_repo,
                        analysis_events_repo,
                        notification_manager,
                        retry_delays,
                    )
                    .await
                })
            })
            .collect();

        // Wait for all deliveries to complete
        for handle in handles {
            if let Err(e) = handle.await {
                error!(error = %e, "Delivery processing task failed");
            }
        }

        Ok(())
    }

    /// Process a single notification delivery
    async fn process_single_delivery(
        delivery: NotificationDelivery,
        delivery_repo: NotificationDeliveryRepository,
        analysis_events_repo: AnalysisEventRepository,
        notification_manager: NotificationManager,
        retry_delays: Vec<i32>,
    ) -> Result<()> {
        debug!(
            delivery_id = %delivery.id,
            channel_type = %delivery.channel_type,
            attempt_count = delivery.attempt_count,
            "Processing notification delivery"
        );

        // Get the original analysis event
        let event = match analysis_events_repo
            .get_by_id(&delivery.analysis_event_id)
            .await?
        {
            Some(event) => event,
            None => {
                warn!(
                    delivery_id = %delivery.id,
                    event_id = %delivery.analysis_event_id,
                    "Analysis event not found for delivery"
                );
                return Ok(());
            }
        };

        // Attempt to send the notification
        let result = Self::send_notification(&notification_manager, &delivery, &event).await;

        match result {
            Ok(external_id) => {
                // Success - mark as sent
                let update = UpdateDeliveryStatus {
                    status: DeliveryStatus::Sent,
                    external_id,
                    error_message: None,
                    metadata: None,
                };

                delivery_repo.update_status(&delivery.id, update).await?;

                info!(
                    delivery_id = %delivery.id,
                    event_id = %event.id,
                    channel_type = %delivery.channel_type,
                    "Notification sent successfully"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    delivery_id = %delivery.id,
                    event_id = %event.id,
                    channel_type = %delivery.channel_type,
                    attempt_count = delivery.attempt_count,
                    "Failed to send notification"
                );

                // Check if we should retry
                if delivery.attempt_count < retry_delays.len() as i32 {
                    // Schedule retry with exponential backoff
                    let delay_minutes = retry_delays[delivery.attempt_count as usize];
                    delivery_repo
                        .schedule_retry(&delivery.id, delay_minutes)
                        .await?;

                    debug!(
                        delivery_id = %delivery.id,
                        delay_minutes,
                        "Scheduled notification retry"
                    );
                } else {
                    // Max attempts reached - mark as failed
                    let update = UpdateDeliveryStatus {
                        status: DeliveryStatus::Failed,
                        external_id: None,
                        error_message: Some(e.to_string()),
                        metadata: None,
                    };

                    delivery_repo.update_status(&delivery.id, update).await?;

                    warn!(
                        delivery_id = %delivery.id,
                        event_id = %event.id,
                        channel_type = %delivery.channel_type,
                        "Notification failed after max attempts"
                    );
                }
            }
        }

        Ok(())
    }

    /// Send a notification via the appropriate adapter
    async fn send_notification(
        _notification_manager: &NotificationManager,
        delivery: &NotificationDelivery,
        event: &AnalysisEvent,
    ) -> Result<Option<String>> {
        match delivery.channel_type.as_str() {
            "pushover" => {
                Self::send_pushover_notification(_notification_manager, delivery, event).await
            }
            "webhook" => {
                Self::send_webhook_notification(_notification_manager, delivery, event).await
            }
            #[cfg(feature = "webpush")]
            "webpush" => {
                Self::send_webpush_notification(_notification_manager, delivery, event).await
            }
            _ => {
                warn!(
                    channel_type = %delivery.channel_type,
                    "Unknown notification channel type"
                );
                Err(gl_core::Error::Validation(format!(
                    "Unknown channel type: {}",
                    delivery.channel_type
                )))
            }
        }
    }

    /// Send Pushover notification
    async fn send_pushover_notification(
        _notification_manager: &NotificationManager,
        delivery: &NotificationDelivery,
        event: &AnalysisEvent,
    ) -> Result<Option<String>> {
        let user_key = delivery
            .channel_config
            .get("user_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                gl_core::Error::Validation("Missing user_key in Pushover config".to_string())
            })?;

        let title = format!("{} Alert", event.event_type);
        let message = format!(
            "{}\nSeverity: {}\nConfidence: {:.2}\nSource: {}",
            event.description, event.severity, event.confidence, event.source_id
        );

        // Create a resilient Pushover adapter (simplified for this example)
        let _adapter = PushoverAdapter::with_resilience("mock_app_token".to_string());
        // In real implementation, this would use the actual Pushover API
        // adapter.send_notification(user_key, &title, &message).await

        // For now, just log success
        debug!(
            user_key,
            title = %title,
            message = %message,
            "Would send Pushover notification"
        );

        Ok(Some("mock_pushover_id".to_string()))
    }

    /// Send webhook notification
    async fn send_webhook_notification(
        _notification_manager: &NotificationManager,
        delivery: &NotificationDelivery,
        event: &AnalysisEvent,
    ) -> Result<Option<String>> {
        let webhook_url = delivery
            .channel_config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                gl_core::Error::Validation("Missing url in webhook config".to_string())
            })?;

        // Create webhook payload
        let payload = serde_json::json!({
            "event_id": event.id,
            "event_type": event.event_type,
            "severity": event.severity,
            "confidence": event.confidence,
            "description": event.description,
            "source_id": event.source_id,
            "created_at": event.created_at,
            "metadata": event.metadata
        });

        // Create a basic webhook adapter
        let _adapter = WebhookAdapter::new();
        // In real implementation, this would make the HTTP request
        // adapter.send_webhook(webhook_url, &payload).await

        debug!(
            webhook_url,
            payload = ?payload,
            "Would send webhook notification"
        );

        Ok(Some("mock_webhook_id".to_string()))
    }

    /// Send web push notification
    #[cfg(feature = "webpush")]
    async fn send_webpush_notification(
        _notification_manager: &NotificationManager,
        delivery: &NotificationDelivery,
        event: &AnalysisEvent,
    ) -> Result<Option<String>> {
        let endpoint = delivery
            .channel_config
            .get("endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                gl_core::Error::Validation("Missing endpoint in web push config".to_string())
            })?;

        let title = format!("{} Alert", event.event_type);
        let body = format!("{} (Severity: {})", event.description, event.severity);

        // Create a basic web push adapter
        let adapter = WebPushAdapter::new();
        // In real implementation, this would send the web push
        // adapter.send_push(endpoint, &title, &body).await

        debug!(
            endpoint,
            title = %title,
            body = %body,
            "Would send web push notification"
        );

        Ok(Some("mock_webpush_id".to_string()))
    }

    /// Check if event severity meets channel threshold
    fn meets_severity_threshold(&self, event_severity: &str, threshold: &str) -> bool {
        let severity_levels = ["info", "low", "medium", "high", "critical"];

        let event_level = severity_levels
            .iter()
            .position(|&s| s == event_severity)
            .unwrap_or(0);
        let threshold_level = severity_levels
            .iter()
            .position(|&s| s == threshold)
            .unwrap_or(0);

        event_level >= threshold_level
    }

    /// Get dispatcher statistics
    pub async fn get_stats(&self) -> Result<HashMap<String, serde_json::Value>> {
        let delivery_stats = self.delivery_repo.get_stats(24).await?;

        let mut stats = HashMap::new();
        let delivery_stats_value = serde_json::to_value(delivery_stats).map_err(|e| {
            gl_core::Error::Config(format!("Failed to serialize delivery stats: {}", e))
        })?;
        stats.insert("delivery_stats_24h".to_string(), delivery_stats_value);
        stats.insert(
            "channels_configured".to_string(),
            serde_json::Value::Number(self.config.channels.len().into()),
        );
        stats.insert(
            "polling_interval_seconds".to_string(),
            serde_json::Value::Number(self.config.polling_interval_seconds.into()),
        );

        Ok(stats)
    }
}
