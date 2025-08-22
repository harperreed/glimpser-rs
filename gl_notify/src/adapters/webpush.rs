//! ABOUTME: WebPush notification adapter for browser push notifications
//! ABOUTME: Sends push notifications to web browsers using the WebPush protocol

use async_trait::async_trait;
use tracing::{debug, error};

use crate::{Notification, NotificationChannel, Notifier, Result};

/// WebPush notification adapter
#[derive(Debug)]
pub struct WebPushAdapter {
    // TODO: Add web-push client when feature is enabled
}

impl WebPushAdapter {
    /// Create a new WebPush adapter
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for WebPushAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Notifier for WebPushAdapter {
    async fn send(&self, msg: &Notification) -> Result<()> {
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

                // TODO: Implement actual WebPush sending when web-push feature is enabled
                // For now, just log that we would send
                error!(
                    notification_id = %msg.id,
                    endpoint = %endpoint,
                    title = %msg.title,
                    "WebPush adapter not fully implemented - would send: {}",
                    msg.body
                );
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "webpush"
    }
}
