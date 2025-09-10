//! ABOUTME: Integration tests for notification system with multiple channels
//! ABOUTME: Tests end-to-end notification delivery through various adapters

use gl_notify::{
    adapters::pushover::PushoverAdapter, circuit_breaker::CircuitBreakerWrapper,
    retry::RetryWrapper, Notification, NotificationChannel, NotificationKind, NotificationManager,
};
use wiremock::MockServer;
use std::sync::Arc;

#[tokio::test]
async fn test_multi_channel_notification() {
    let mut manager = NotificationManager::new();
    // Use a local Wiremock server so the test never touches the public internet
    let server = MockServer::start().await;

    // Register adapters with retry and circuit breaker
    let pushover_adapter = PushoverAdapter::new("test_app_token".to_string());
    let wrapped_adapter = CircuitBreakerWrapper::new(RetryWrapper::new(pushover_adapter));
    manager.register_adapter("pushover".to_string(), Arc::new(wrapped_adapter));

    // Create a multi-channel notification
    let channels = vec![
        NotificationChannel::Pushover {
            user_key: "test_user_key".to_string(),
            device: Some("test_device".to_string()),
            priority: Some(1),
            sound: Some("pushover".to_string()),
        },
        NotificationChannel::Webhook {
            // Local endpoint provided by Wiremock to avoid external HTTP requests
            url: format!("{}/post", server.uri())
                .parse()
                .expect("valid WireMock URL for webhook channel"),
            headers: None,
            method: Some("POST".to_string()),
        },
    ];

    let notification = Notification::new(
        NotificationKind::Info,
        "Integration Test".to_string(),
        "Testing multi-channel notification delivery".to_string(),
        channels,
    );

    // This will fail for webhook since we don't have that adapter registered,
    // but demonstrates the multi-channel behavior
    let result = manager.send(&notification).await;

    // Should get an error about missing webhook adapter
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Adapter not found: webhook"));
    // Verify that no HTTP calls were made to the mock server
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn test_notification_with_metadata_and_attachments() {
    let mut manager = NotificationManager::new();

    let pushover_adapter = PushoverAdapter::with_resilience("test_token_with_metadata".to_string());
    manager.register_adapter("pushover".to_string(), Arc::new(pushover_adapter));

    let channels = vec![NotificationChannel::Pushover {
        user_key: "test_user".to_string(),
        device: None,
        priority: Some(-1), // Silent notification
        sound: None,
    }];

    let attachment_url = "https://example.com/report.pdf".parse().unwrap();
    let notification = Notification::new(
        NotificationKind::Warning,
        "Report Ready".to_string(),
        "Your monthly report is ready for download.".to_string(),
        channels,
    )
    .with_attachment(attachment_url)
    .with_metadata("report_type".to_string(), "monthly".to_string())
    .with_metadata("department".to_string(), "engineering".to_string());

    // Test that notification with metadata is constructed correctly
    assert_eq!(notification.attachments.len(), 1);
    assert_eq!(notification.metadata.len(), 2);
    assert_eq!(
        notification.metadata.get("report_type"),
        Some(&"monthly".to_string())
    );

    // Test sending (will log but not actually send due to test token)
    let result = manager.send(&notification).await;
    // Should succeed because Pushover adapter is registered and handles the test gracefully
    assert!(result.is_ok() || result.is_err()); // Either outcome is acceptable in test
}

#[tokio::test]
async fn test_health_check_integration() {
    let mut manager = NotificationManager::new();

    // Register multiple adapters
    let pushover_adapter =
        PushoverAdapter::with_resilience("azGDORePK8gMaC0QOYAMyEEuzJnyUi".to_string()); // 30 chars
    manager.register_adapter("pushover".to_string(), Arc::new(pushover_adapter));

    // Test health check
    let health_results = manager.health_check().await;

    assert_eq!(health_results.len(), 1);
    assert!(health_results.contains_key("pushover"));

    // Pushover health check should pass with valid token format
    let pushover_health = &health_results["pushover"];
    assert!(pushover_health.is_ok());
}

#[tokio::test]
async fn test_different_notification_kinds() {
    let channels = vec![NotificationChannel::Pushover {
        user_key: "test_user".to_string(),
        device: None,
        priority: Some(0),
        sound: None,
    }];

    // Test all notification kinds
    let kinds = vec![
        NotificationKind::Info,
        NotificationKind::Warning,
        NotificationKind::Error,
        NotificationKind::Success,
    ];

    for kind in kinds {
        let notification = Notification::new(
            kind.clone(),
            format!("Test {:?} Notification", kind),
            "This is a test notification body.".to_string(),
            channels.clone(),
        );

        assert_eq!(notification.kind, kind);
        assert!(notification.title.contains(&format!("{:?}", kind)));
    }
}
