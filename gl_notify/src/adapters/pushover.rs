//! ABOUTME: Pushover notification adapter for mobile push notifications
//! ABOUTME: Sends notifications via Pushover API using HTTP POST requests

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::{Notification, NotificationChannel, NotificationError, Notifier, Result};

/// Pushover notification adapter
#[derive(Debug)]
pub struct PushoverAdapter {
    client: Client,
    app_token: String,
}

impl PushoverAdapter {
    /// Create a new Pushover adapter with app token
    pub fn new(app_token: String) -> Self {
        Self {
            client: Client::new(),
            app_token,
        }
    }

    /// Create Pushover adapter with custom client
    pub fn with_client(client: Client, app_token: String) -> Self {
        Self { client, app_token }
    }

    /// Build the Pushover API payload
    fn build_payload(&self, msg: &Notification, user_key: &str, device: Option<&str>, priority: Option<i8>, sound: Option<&str>) -> Value {
        let mut payload = json!({
            "token": self.app_token,
            "user": user_key,
            "title": msg.title,
            "message": msg.body,
        });

        if let Some(device) = device {
            payload["device"] = json!(device);
        }

        if let Some(priority) = priority {
            payload["priority"] = json!(priority);
        }

        if let Some(sound) = sound {
            payload["sound"] = json!(sound);
        }

        // Add notification kind as a prefix to help with categorization
        let kind_prefix = match msg.kind {
            crate::NotificationKind::Info => "ℹ️",
            crate::NotificationKind::Warning => "⚠️", 
            crate::NotificationKind::Error => "❌",
            crate::NotificationKind::Success => "✅",
        };
        
        payload["title"] = json!(format!("{} {}", kind_prefix, msg.title));

        payload
    }
}

#[async_trait]
impl Notifier for PushoverAdapter {
    async fn send(&self, msg: &Notification) -> Result<()> {
        for channel in &msg.channels {
            if let NotificationChannel::Pushover { user_key, device, priority, sound } = channel {
                debug!(
                    notification_id = %msg.id,
                    user_key = %user_key,
                    "Sending Pushover notification"
                );

                let payload = self.build_payload(
                    msg,
                    user_key,
                    device.as_deref(),
                    *priority,
                    sound.as_deref(),
                );

                // TODO: Add retry logic and circuit breaker
                match self
                    .client
                    .post("https://api.pushover.net/1/messages.json")
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            info!(
                                notification_id = %msg.id,
                                user_key = %user_key,
                                "Pushover notification sent successfully"
                            );
                        } else {
                            let status = response.status();
                            let body = response.text().await.unwrap_or_else(|_| "Unable to read response".to_string());
                            warn!(
                                notification_id = %msg.id,
                                user_key = %user_key,
                                status = %status,
                                body = %body,
                                "Pushover API returned error"
                            );
                            return Err(NotificationError::PushoverError(format!("API error {}: {}", status, body)));
                        }
                    }
                    Err(e) => {
                        warn!(
                            notification_id = %msg.id,
                            user_key = %user_key,
                            error = %e,
                            "Failed to send Pushover notification"
                        );
                        return Err(NotificationError::HttpError(e));
                    }
                }
            }
        }
        
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        // Simple health check by validating the app token format
        if self.app_token.is_empty() || self.app_token.len() != 30 {
            return Err(NotificationError::PushoverError(
                "Invalid Pushover app token format".to_string()
            ));
        }
        
        // TODO: Could make a test API call to validate token
        Ok(())
    }

    fn name(&self) -> &str {
        "pushover"
    }
}