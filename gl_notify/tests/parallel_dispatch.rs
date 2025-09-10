//! ABOUTME: Test that notification manager sends through adapters concurrently
//! ABOUTME: Ensures dispatch happens in parallel across multiple channels

use async_trait::async_trait;
use gl_notify::{
    Notification, NotificationChannel, NotificationKind, NotificationManager, Notifier, Result,
};
use std::{sync::Arc, time::Duration};
use tokio::time::{sleep, Instant};

struct SlowAdapter {
    delay: Duration,
}

#[async_trait]
impl Notifier for SlowAdapter {
    async fn send(&self, _msg: &Notification) -> Result<()> {
        sleep(self.delay).await;
        Ok(())
    }

    fn name(&self) -> &str {
        "slow"
    }
}

#[tokio::test]
async fn dispatches_notifications_in_parallel() {
    let mut manager = NotificationManager::new();
    let delay = Duration::from_millis(1000);
    manager.register_adapter("webhook".to_string(), Arc::new(SlowAdapter { delay }));
    manager.register_adapter("pushover".to_string(), Arc::new(SlowAdapter { delay }));

    let channels = vec![
        NotificationChannel::Webhook {
            url: "http://example.com".parse().unwrap(),
            headers: None,
            method: None,
        },
        NotificationChannel::Pushover {
            user_key: "user".to_string(),
            device: None,
            priority: None,
            sound: None,
        },
    ];

    let notification = Notification::new(
        NotificationKind::Info,
        "test".to_string(),
        "body".to_string(),
        channels,
    );

    let start = Instant::now();
    manager
        .send(&notification)
        .await
        .expect("send should complete");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(1500),
        "dispatch took {:?}",
        elapsed
    );
}
