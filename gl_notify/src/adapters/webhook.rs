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
                headers,
                method,
            } = channel
            {
                debug!(
                    notification_id = %msg.id,
                    webhook_url = %url,
                    "Sending webhook notification"
                );

                // Prepare webhook payload
                let payload = serde_json::json!({
                    "id": msg.id.to_string(),
                    "kind": msg.kind,
                    "title": msg.title,
                    "body": msg.body,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "attachments": msg.attachments,
                    "metadata": msg.metadata
                });

                // Determine HTTP method (default to POST)
                let http_method = method.as_deref().unwrap_or("POST");

                // Build request
                let mut request_builder = match http_method.to_uppercase().as_str() {
                    "GET" => self.client.get(url.as_str()),
                    "PUT" => self.client.put(url.as_str()),
                    "PATCH" => self.client.patch(url.as_str()),
                    "DELETE" => self.client.delete(url.as_str()),
                    _ => self.client.post(url.as_str()), // Default to POST
                };

                // Add custom headers if provided
                if let Some(custom_headers) = headers {
                    for (key, value) in custom_headers {
                        request_builder = request_builder.header(key, value);
                    }
                }

                // Add content-type header for POST/PUT/PATCH requests
                if matches!(
                    http_method.to_uppercase().as_str(),
                    "POST" | "PUT" | "PATCH"
                ) {
                    request_builder = request_builder
                        .header("Content-Type", "application/json")
                        .json(&payload);
                }

                // Send the request
                match request_builder.send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            debug!(
                                notification_id = %msg.id,
                                webhook_url = %url,
                                status = %response.status(),
                                "Webhook notification sent successfully"
                            );
                        } else {
                            let status = response.status();
                            let error_body = response
                                .text()
                                .await
                                .unwrap_or_else(|_| "Unknown error".to_string());
                            error!(
                                notification_id = %msg.id,
                                webhook_url = %url,
                                status = %status,
                                error_body = %error_body,
                                "Webhook request failed"
                            );
                            return Err(crate::NotificationError::WebhookError(format!(
                                "HTTP {} from {}: {}",
                                status, url, error_body
                            )));
                        }
                    }
                    Err(e) => {
                        error!(
                            notification_id = %msg.id,
                            webhook_url = %url,
                            error = %e,
                            "Failed to send webhook notification"
                        );
                        return Err(crate::NotificationError::WebhookError(format!(
                            "Request failed for {}: {}",
                            url, e
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "webhook"
    }

    async fn health_check(&self) -> Result<()> {
        // Webhook adapter is always healthy if we can create HTTP clients
        debug!("Webhook adapter health check passed");
        Ok(())
    }
}
