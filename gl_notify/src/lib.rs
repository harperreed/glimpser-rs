//! ABOUTME: Notification system with multiple channel adapters
//! ABOUTME: Sends alerts via SMTP, SMS, webhooks, and push notifications

use async_trait::async_trait;
use gl_core::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use url::Url;

pub mod adapters;
pub mod cap;
pub mod circuit_breaker;
pub mod dispatcher;
pub mod retry;

pub use cap::{CapNotification, CapNotificationBuilder};
pub use circuit_breaker::CircuitBreakerWrapper;
pub use dispatcher::{DispatcherConfig, NotificationChannelConfig, NotificationDispatcher};
pub use retry::RetryWrapper;

/// Result type for notification operations
pub type Result<T> = std::result::Result<T, NotificationError>;

/// Errors that can occur during notification operations
#[derive(Error, Debug)]
pub enum NotificationError {
    #[error("Channel adapter not found: {0}")]
    ChannelNotFound(String),
    #[error("SMTP error: {0}")]
    SmtpError(String),
    #[error("SMS error: {0}")]
    SmsError(String),
    #[error("Webhook error: {0}")]
    WebhookError(String),
    #[error("WebPush error: {0}")]
    WebPushError(String),
    #[error("Pushover error: {0}")]
    PushoverError(String),
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("Circuit breaker open for channel: {0}")]
    CircuitBreakerOpen(String),
    #[error("Retry exhausted for notification: {0}")]
    RetryExhausted(String),
}

// Add error conversions for web-push when feature is enabled
#[cfg(feature = "webpush")]
impl From<web_push::WebPushError> for NotificationError {
    fn from(err: web_push::WebPushError) -> Self {
        NotificationError::WebPushError(err.to_string())
    }
}

/// Type of notification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Warning,
    Error,
    Success,
}

/// Channel configuration for different notification types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationChannel {
    Webhook {
        url: Url,
        headers: Option<HashMap<String, String>>,
        method: Option<String>,
    },
    WebPush {
        endpoint: Url,
        p256dh: String,
        auth: String,
    },
    Pushover {
        user_key: String,
        device: Option<String>,
        priority: Option<i8>,
        sound: Option<String>,
    },
}

/// Core notification message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique identifier for the notification
    pub id: Id,
    /// Type/severity of notification
    pub kind: NotificationKind,
    /// Short title or subject
    pub title: String,
    /// Main message body
    pub body: String,
    /// Target channels for delivery
    pub channels: Vec<NotificationChannel>,
    /// Optional file attachments as URIs
    pub attachments: Vec<Url>,
    /// Optional metadata for adapters
    pub metadata: HashMap<String, String>,
}

impl Notification {
    /// Create a new notification
    pub fn new(
        kind: NotificationKind,
        title: String,
        body: String,
        channels: Vec<NotificationChannel>,
    ) -> Self {
        Self {
            id: Id::new(),
            kind,
            title,
            body,
            channels,
            attachments: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add an attachment URL
    pub fn with_attachment(mut self, url: Url) -> Self {
        self.attachments.push(url);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Core trait for notification adapters
#[async_trait]
pub trait Notifier: Send + Sync {
    /// Send a notification through this adapter
    async fn send(&self, msg: &Notification) -> Result<()>;

    /// Check if the adapter is healthy/available
    async fn health_check(&self) -> Result<()> {
        Ok(())
    }

    /// Get the adapter's name for logging/debugging
    fn name(&self) -> &str;
}

/// Multi-channel notification manager
pub struct NotificationManager {
    adapters: HashMap<String, Box<dyn Notifier>>,
}

impl NotificationManager {
    /// Create a new notification manager
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register a notification adapter
    pub fn register_adapter(&mut self, name: String, adapter: Box<dyn Notifier>) {
        self.adapters.insert(name, adapter);
    }

    /// Send a notification through all applicable adapters
    pub async fn send(&self, notification: &Notification) -> Result<()> {
        let mut errors = Vec::new();

        for channel in &notification.channels {
            let adapter_name = match channel {
                NotificationChannel::Webhook { .. } => "webhook",
                NotificationChannel::WebPush { .. } => "webpush",
                NotificationChannel::Pushover { .. } => "pushover",
            };

            if let Some(adapter) = self.adapters.get(adapter_name) {
                if let Err(e) = adapter.send(notification).await {
                    errors.push(format!("{}: {}", adapter_name, e));
                }
            } else {
                errors.push(format!("Adapter not found: {}", adapter_name));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(NotificationError::RetryExhausted(errors.join(", ")))
        }
    }

    /// Get all registered adapter names
    pub fn adapters(&self) -> Vec<&str> {
        self.adapters.keys().map(|s| s.as_str()).collect()
    }

    /// Health check all adapters
    pub async fn health_check(&self) -> HashMap<String, Result<()>> {
        let mut results = HashMap::new();

        for (name, adapter) in &self.adapters {
            results.insert(name.clone(), adapter.health_check().await);
        }

        results
    }
}

impl Clone for NotificationManager {
    fn clone(&self) -> Self {
        // For now, clone creates a new empty manager
        // In a real implementation, we'd need to handle adapter cloning
        Self::new()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_creation() {
        let channels = vec![NotificationChannel::Pushover {
            user_key: "test_user".to_string(),
            device: None,
            priority: Some(0),
            sound: None,
        }];

        let notification = Notification::new(
            NotificationKind::Info,
            "Test Title".to_string(),
            "Test Body".to_string(),
            channels,
        );

        assert_eq!(notification.kind, NotificationKind::Info);
        assert_eq!(notification.title, "Test Title");
        assert_eq!(notification.body, "Test Body");
        assert_eq!(notification.channels.len(), 1);
        assert!(notification.attachments.is_empty());
        assert!(notification.metadata.is_empty());
    }

    #[test]
    fn test_notification_with_attachments() {
        let channels = vec![NotificationChannel::Webhook {
            url: "https://example.com/webhook".parse().unwrap(),
            headers: None,
            method: None,
        }];

        let attachment_url = "https://example.com/file.pdf".parse().unwrap();
        let notification = Notification::new(
            NotificationKind::Warning,
            "Test".to_string(),
            "Body".to_string(),
            channels,
        )
        .with_attachment(attachment_url)
        .with_metadata("source".to_string(), "test".to_string());

        assert_eq!(notification.attachments.len(), 1);
        assert_eq!(
            notification.metadata.get("source"),
            Some(&"test".to_string())
        );
    }

    #[tokio::test]
    async fn test_notification_manager() {
        use crate::adapters::pushover::PushoverAdapter;

        let mut manager = NotificationManager::new();

        // Register a Pushover adapter
        let pushover_adapter = PushoverAdapter::new("test_token".to_string());
        manager.register_adapter("pushover".to_string(), Box::new(pushover_adapter));

        assert_eq!(manager.adapters().len(), 1);
        assert!(manager.adapters().contains(&"pushover"));

        // Test health check
        let health_results = manager.health_check().await;
        assert!(health_results.contains_key("pushover"));
    }
}
