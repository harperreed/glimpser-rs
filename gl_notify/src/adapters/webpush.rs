//! ABOUTME: WebPush notification adapter for browser push notifications
//! ABOUTME: Sends push notifications to web browsers using the WebPush protocol

use async_trait::async_trait;
use tracing::{debug, error, warn};

use crate::{Notification, NotificationChannel, Notifier, Result};

#[cfg(feature = "webpush")]
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, VapidSignatureBuilder, WebPushClient,
    WebPushMessage, WebPushPayload,
};

/// WebPush notification adapter
#[derive(Debug)]
pub struct WebPushAdapter {
    #[cfg(feature = "webpush")]
    client: IsahcWebPushClient,
    #[cfg(feature = "webpush")]
    vapid_private_key: Vec<u8>,
    #[cfg(feature = "webpush")]
    vapid_public_key: Vec<u8>,
}

impl WebPushAdapter {
    /// Create a new WebPush adapter
    #[cfg(feature = "webpush")]
    pub fn new(vapid_private_key: Vec<u8>, vapid_public_key: Vec<u8>) -> Self {
        Self {
            client: IsahcWebPushClient::new().expect("Failed to create WebPush client"),
            vapid_private_key,
            vapid_public_key,
        }
    }

    /// Create a new WebPush adapter (stub when feature is disabled)
    #[cfg(not(feature = "webpush"))]
    pub fn new(_vapid_private_key: Vec<u8>, _vapid_public_key: Vec<u8>) -> Self {
        Self {}
    }
}

impl Default for WebPushAdapter {
    fn default() -> Self {
        // Use empty keys for default - this won't work for real usage
        Self::new(vec![], vec![])
    }
}

#[async_trait]
impl Notifier for WebPushAdapter {
    async fn send(&self, msg: &Notification) -> Result<()> {
        #[cfg(feature = "webpush")]
        {
            for channel in &msg.channels {
                if let NotificationChannel::WebPush {
                    endpoint,
                    p256dh,
                    auth,
                } = channel
                {
                    debug!(
                        notification_id = %msg.id,
                        endpoint = %endpoint,
                        "Sending WebPush notification"
                    );

                    // Create subscription info
                    let subscription_info = SubscriptionInfo::new(endpoint.as_str(), p256dh, auth);

                    // Prepare push payload
                    let payload = serde_json::json!({
                        "title": msg.title,
                        "body": msg.body,
                        "icon": "/icon-192x192.png", // Default icon
                        "badge": "/badge-72x72.png", // Default badge
                        "data": {
                            "id": msg.id.to_string(),
                            "kind": msg.kind,
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                            "metadata": msg.metadata
                        }
                    })
                    .to_string();

                    let message = WebPushMessage::builder()
                        .set_payload(WebPushPayload::Plaintext(payload))
                        .set_content_encoding(ContentEncoding::AesGcm)
                        .build()?;

                    // Build VAPID signature
                    let vapid_key_str =
                        std::str::from_utf8(&self.vapid_private_key).map_err(|e| {
                            crate::NotificationError::WebPushError(format!(
                                "Invalid VAPID key encoding: {}",
                                e
                            ))
                        })?;
                    let mut sig_builder =
                        VapidSignatureBuilder::from_pem_no_sub(vapid_key_str.as_bytes())?;
                    let signature = sig_builder.build()?;

                    // Send the push notification
                    match self
                        .client
                        .send_with_vapid(message, &subscription_info, &signature)
                        .await
                    {
                        Ok(response) => {
                            debug!(
                                notification_id = %msg.id,
                                endpoint = %endpoint,
                                status = ?response.status_code,
                                "WebPush notification sent successfully"
                            );
                        }
                        Err(e) => {
                            error!(
                                notification_id = %msg.id,
                                endpoint = %endpoint,
                                error = %e,
                                "Failed to send WebPush notification"
                            );
                            return Err(crate::NotificationError::WebPushError(format!(
                                "Failed to send WebPush to {}: {}",
                                endpoint, e
                            )));
                        }
                    }
                }
            }
        }

        #[cfg(not(feature = "webpush"))]
        {
            for channel in &msg.channels {
                if let NotificationChannel::WebPush { endpoint, .. } = channel {
                    warn!(
                        notification_id = %msg.id,
                        endpoint = %endpoint,
                        title = %msg.title,
                        "WebPush feature not enabled - would send: {}",
                        msg.body
                    );
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "webpush"
    }

    async fn health_check(&self) -> Result<()> {
        #[cfg(feature = "webpush")]
        {
            // Check if VAPID keys are properly configured
            if self.vapid_private_key.is_empty() || self.vapid_public_key.is_empty() {
                return Err(crate::NotificationError::WebPushError(
                    "VAPID keys not configured".to_string(),
                ));
            }
            debug!("WebPush adapter health check passed");
        }

        #[cfg(not(feature = "webpush"))]
        {
            debug!("WebPush adapter health check passed (feature disabled)");
        }

        Ok(())
    }
}
