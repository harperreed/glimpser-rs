//! ABOUTME: Webhook notification adapter for HTTP POST notifications
//! ABOUTME: Sends JSON payloads to configured webhook endpoints

use async_trait::async_trait;
use reqwest::Client;
use tracing::{debug, error};

use crate::{Notification, NotificationChannel, Notifier, Result};

/// Webhook notification adapter
#[derive(Debug)]
pub struct WebhookAdapter {
    #[allow(dead_code)]
    client: Client,
}

impl WebhookAdapter {
    /// Create a new webhook adapter
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Create webhook adapter with custom client
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

impl Default for WebhookAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for WebhookAdapter {
    async fn send(&self, msg: &Notification) -> Result<()> {
        for channel in &msg.channels {
            if let NotificationChannel::Webhook {
                url,
                headers: _,
                method: _,
            } = channel
            {
                debug!(
                    notification_id = %msg.id,
                    webhook_url = %url,
                    "Sending webhook notification"
                );

                // TODO: Implement actual webhook sending with retry logic and circuit breaker
                // For now, just log that we would send
                error!(
                    notification_id = %msg.id,
                    webhook_url = %url,
                    title = %msg.title,
                    "Webhook adapter not fully implemented - would send: {}",
                    msg.body
                );
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "webhook"
    }
}
