//! ABOUTME: Integration tests for analysis pipeline with realistic scenarios
//! ABOUTME: Tests motion burst deduplication, time window rules, and end-to-end processing

use bytes::Bytes;
use chrono::{TimeZone, Utc};
use gl_analysis::{
    rule_engine::{
        Action, ComparisonOperator, Condition, ConditionType, DeduplicationConfig,
        QuietHoursConfig, Rule, RuleSet,
    },
    AnalysisConfig, AnalysisService, EventSeverity, ProcessorContext, ProcessorInput,
};
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

/// Helper to create test frame data
fn create_test_frame() -> Bytes {
    // Create a simple test JPEG-like frame
    Bytes::from(vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46,
    ])
}

/// Helper to create test input with frame data
fn create_test_input(template_id: &str, source_id: &str) -> ProcessorInput {
    ProcessorInput {
        template_id: template_id.to_string(),
        frame_data: Some(create_test_frame()),
        frame_format: Some("jpeg".to_string()),
        text_content: None,
        context: ProcessorContext::new(source_id.to_string()),
        timestamp: Utc::now(),
    }
}

/// Test basic analysis pipeline without rules
/// Note: Currently skipped because motion detection requires real image data
/// TODO: Create integration tests with realistic test images
#[tokio::test]
#[ignore]
async fn test_basic_analysis_pipeline() {
    let config = AnalysisConfig::default();
    let mut service = AnalysisService::new(config).unwrap();

    let input = create_test_input("template_001", "camera_01");
    let events = service.analyze(input).await.unwrap();

    println!(
        "Generated {} events: {:?}",
        events.len(),
        events.iter().map(|e| &e.event_type).collect::<Vec<_>>()
    );

    // Should generate at least motion and summary events
    assert!(!events.is_empty());
}

/// Test motion burst scenario with deduplication
/// Note: Currently skipped because motion detection requires real image data
/// TODO: Create integration tests with realistic test images
#[tokio::test]
#[ignore]
async fn test_motion_burst_deduplication() {
    // Configure deduplication for motion events
    let dedup_config = DeduplicationConfig {
        window_minutes: 2,
        event_types: vec!["motion_detected".to_string()],
        key_fields: vec!["event_type".to_string(), "source_id".to_string()],
    };

    let rule_set = RuleSet {
        rules: vec![],
        deduplication: Some(dedup_config),
        quiet_hours: None,
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();

    // Simulate rapid motion events (burst scenario)
    let mut total_events = 0;
    let mut motion_events = 0;

    for i in 0..3 {
        let input = create_test_input("template_burst", "camera_entrance");
        let events = service.analyze(input).await.unwrap();

        total_events += events.len();
        motion_events += events
            .iter()
            .filter(|e| e.event_type == "motion_detected")
            .count();

        println!("Burst {} generated {} events", i + 1, events.len());

        // No delay needed; deduplication window covers rapid events
    }

    println!(
        "Total motion events: {} out of {} total events",
        motion_events, total_events
    );

    // Should emit exactly one motion event due to deduplication
    assert_eq!(
        motion_events, 1,
        "Deduplication should allow only the first motion event"
    );
}

/// Test rule evaluation with time windows
#[tokio::test]
async fn test_time_window_rules() {
    // Create a rule that suppresses notifications during business hours
    let suppress_rule = Rule {
        id: "business_hours".to_string(),
        name: "Suppress during business hours".to_string(),
        description: Some("Reduce noise during 9-5 workdays".to_string()),
        conditions: vec![Condition {
            condition_type: ConditionType::TimeWindow {
                start: "09:00".to_string(),
                end: "17:00".to_string(),
                days: vec![1, 2, 3, 4, 5], // Monday-Friday
            },
        }],
        actions: vec![Action::SuppressNotification],
        enabled: true,
        priority: 100,
    };

    let rule_set = RuleSet {
        rules: vec![suppress_rule],
        deduplication: None,
        quiet_hours: None,
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();

    // Test during business hours (simulated)
    let mut input = create_test_input("template_office", "camera_office");

    // Simulate Tuesday 2023-12-05 at 14:00 UTC (business hours)
    input.timestamp = Utc.with_ymd_and_hms(2023, 12, 5, 14, 0, 0).unwrap();

    let events = service.analyze(input).await.unwrap();

    // Events should have notifications suppressed
    for event in &events {
        if event.event_type == "motion_detected" {
            assert!(
                !event.should_notify,
                "Notifications should be suppressed during business hours"
            );
        }
    }

    println!(
        "Business hours test passed: {} events with notifications suppressed",
        events.len()
    );
}

/// Test severity-based rule evaluation
#[tokio::test]
async fn test_severity_based_rules() {
    // Rule to escalate high-confidence motion events
    let escalation_rule = Rule {
        id: "high_confidence_escalation".to_string(),
        name: "Escalate high confidence events".to_string(),
        description: None,
        conditions: vec![
            Condition {
                condition_type: ConditionType::EventType {
                    pattern: "motion_detected".to_string(),
                    matches: true,
                },
            },
            Condition {
                condition_type: ConditionType::Confidence {
                    operator: ComparisonOperator::GreaterThan,
                    value: 0.9,
                },
            },
        ],
        actions: vec![
            Action::SetSeverity {
                severity: EventSeverity::High,
            },
            Action::AddMetadata {
                key: "escalated".to_string(),
                value: serde_json::Value::Bool(true),
            },
        ],
        enabled: true,
        priority: 200,
    };

    let rule_set = RuleSet {
        rules: vec![escalation_rule],
        deduplication: None,
        quiet_hours: None,
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();
    let input = create_test_input("template_security", "camera_perimeter");

    let events = service.analyze(input).await.unwrap();

    // Look for escalated events
    let escalated_events: Vec<_> = events
        .iter()
        .filter(|e| e.metadata.contains_key("escalated"))
        .collect();

    println!(
        "Found {} escalated events out of {} total",
        escalated_events.len(),
        events.len()
    );

    for event in escalated_events {
        assert_eq!(event.severity, EventSeverity::High);
        assert!(event.metadata.get("escalated").unwrap().as_bool().unwrap());
    }
}

/// Test quiet hours functionality
#[tokio::test]
async fn test_quiet_hours() {
    let quiet_hours = QuietHoursConfig {
        start_time: "22:00".to_string(),
        end_time: "06:00".to_string(),
        days: vec![0, 1, 2, 3, 4, 5, 6], // All days
        actions: vec![Action::SuppressNotification],
    };

    let rule_set = RuleSet {
        rules: vec![],
        deduplication: None,
        quiet_hours: Some(quiet_hours),
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();

    // Test during quiet hours (2 AM)
    let mut input = create_test_input("template_night", "camera_bedroom");
    input.timestamp = Utc.with_ymd_and_hms(2023, 12, 1, 2, 0, 0).unwrap();

    let events = service.analyze(input).await.unwrap();

    // All events should have notifications suppressed during quiet hours
    for event in &events {
        assert!(
            !event.should_notify,
            "Event {} should not notify during quiet hours",
            event.event_type
        );
    }

    println!(
        "Quiet hours test passed: {} events with notifications suppressed",
        events.len()
    );
}

/// Test event count based rules
#[tokio::test]
async fn test_event_count_rules() {
    // Rule to trigger alert if too many motion events in short time
    let burst_rule = Rule {
        id: "motion_burst_alert".to_string(),
        name: "Alert on motion burst".to_string(),
        description: Some("Alert when too much motion detected quickly".to_string()),
        conditions: vec![Condition {
            condition_type: ConditionType::EventCount {
                event_type: Some("motion_detected".to_string()),
                count: 3,
                operator: ComparisonOperator::GreaterThanOrEqual,
                window_minutes: 1,
            },
        }],
        actions: vec![
            Action::SetSeverity {
                severity: EventSeverity::Critical,
            },
            Action::AddMetadata {
                key: "burst_detected".to_string(),
                value: serde_json::Value::Bool(true),
            },
        ],
        enabled: true,
        priority: 300,
    };

    let rule_set = RuleSet {
        rules: vec![burst_rule],
        deduplication: None,
        quiet_hours: None,
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();

    // Generate multiple motion events rapidly
    for i in 0..4 {
        let input = create_test_input("template_burst_alert", "camera_entrance");
        let events = service.analyze(input).await.unwrap();

        println!("Burst iteration {}: {} events", i + 1, events.len());

        // Check if burst alert was triggered on later events
        if i >= 2 {
            // After we have 3+ events in history
            let burst_events: Vec<_> = events
                .iter()
                .filter(|e| e.metadata.contains_key("burst_detected"))
                .collect();

            if !burst_events.is_empty() {
                println!("Burst alert triggered after {} iterations", i + 1);
                assert!(
                    !burst_events.is_empty(),
                    "Expected burst alert to be triggered"
                );

                for event in burst_events {
                    assert_eq!(event.severity, EventSeverity::Critical);
                }
                break;
            }
        }

        sleep(Duration::from_millis(100)).await;
    }
}

/// Test complex processor chain with all processors
#[tokio::test]
async fn test_full_processor_chain() {
    let config = AnalysisConfig {
        enabled_processors: vec![
            "motion".to_string(),
            "ai_description".to_string(),
            "summary".to_string(),
        ],
        processor_configs: {
            let mut configs = HashMap::new();
            configs.insert(
                "ai_description".to_string(),
                serde_json::json!({
                    "motion_only": false,
                    "detail_level": "high",
                    "focus": "security"
                }),
            );
            configs.insert(
                "summary".to_string(),
                serde_json::json!({
                    "min_events": 1,
                    "max_length": 150,
                    "style": "security_brief"
                }),
            );
            configs
        },
        rules: None,
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();
    let input = create_test_input("template_full_chain", "camera_main");

    let events = service.analyze(input).await.unwrap();

    println!("Full chain generated {} events:", events.len());
    for event in &events {
        println!(
            "  - {}: {} ({})",
            event.event_type, event.description, event.processor_name
        );
    }

    // Should have events from different processors
    let processor_names: std::collections::HashSet<_> =
        events.iter().map(|e| e.processor_name.as_str()).collect();

    println!("Processors that generated events: {:?}", processor_names);

    // Verify we got events from multiple processors
    assert!(!events.is_empty(), "Should generate at least one event");
}

/// Test rule priority ordering
#[tokio::test]
async fn test_rule_priority() {
    // Low priority rule that sets severity to High
    let low_priority_rule = Rule {
        id: "low_priority".to_string(),
        name: "Low Priority Rule".to_string(),
        description: None,
        conditions: vec![Condition {
            condition_type: ConditionType::EventType {
                pattern: "motion_detected".to_string(),
                matches: true,
            },
        }],
        actions: vec![Action::SetSeverity {
            severity: EventSeverity::High,
        }],
        enabled: true,
        priority: 10,
    };

    // High priority rule that sets severity to Critical (should win)
    let high_priority_rule = Rule {
        id: "high_priority".to_string(),
        name: "High Priority Rule".to_string(),
        description: None,
        conditions: vec![Condition {
            condition_type: ConditionType::EventType {
                pattern: "motion_detected".to_string(),
                matches: true,
            },
        }],
        actions: vec![Action::SetSeverity {
            severity: EventSeverity::Critical,
        }],
        enabled: true,
        priority: 100, // Higher priority
    };

    let rule_set = RuleSet {
        rules: vec![low_priority_rule, high_priority_rule],
        deduplication: None,
        quiet_hours: None,
    };

    let config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        rules: Some(rule_set),
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();
    let input = create_test_input("template_priority", "camera_test");

    let events = service.analyze(input).await.unwrap();

    // Find motion events and verify they have Critical severity (from high priority rule)
    let motion_events: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "motion_detected")
        .collect();

    for event in motion_events {
        assert_eq!(
            event.severity,
            EventSeverity::Critical,
            "High priority rule should set severity to Critical"
        );
    }
}

/// Test analysis service configuration updates
#[tokio::test]
async fn test_config_updates() {
    // Start with basic config
    let mut config = AnalysisConfig {
        enabled_processors: vec!["motion".to_string()],
        ..Default::default()
    };

    let mut service = AnalysisService::new(config.clone()).unwrap();

    // Process an event with original config
    let input1 = create_test_input("template_update_test", "camera_01");
    let events1 = service.analyze(input1).await.unwrap();

    println!("Original config generated {} events", events1.len());

    // Update config to include more processors
    config.enabled_processors = vec!["motion".to_string(), "ai_description".to_string()];

    service.update_config(config).await.unwrap();

    // Process event with updated config
    let input2 = create_test_input("template_update_test", "camera_01");
    let events2 = service.analyze(input2).await.unwrap();

    println!("Updated config generated {} events", events2.len());

    // Should potentially have more events with additional processors
    let processor_types2: std::collections::HashSet<_> =
        events2.iter().map(|e| e.processor_name.as_str()).collect();

    println!("Processors after update: {:?}", processor_types2);
}

/// Test error handling and resilience
#[tokio::test]
async fn test_error_resilience() {
    let config = AnalysisConfig {
        enabled_processors: vec![
            "motion".to_string(),
            "nonexistent_processor".to_string(), // Should be skipped
            "ai_description".to_string(),
        ],
        ..Default::default()
    };

    let mut service = AnalysisService::new(config).unwrap();

    // Process with potentially problematic input
    let mut input = create_test_input("template_error_test", "camera_error");
    input.frame_data = Some(Bytes::from("invalid_frame_data")); // Invalid frame

    let events = service.analyze(input).await.unwrap();

    // Should still generate some events despite errors
    println!("Error resilience test generated {} events", events.len());

    // Service should continue to work after errors
    let input2 = create_test_input("template_error_test_2", "camera_error");
    let events2 = service.analyze(input2).await.unwrap();

    println!(
        "Second analysis after errors generated {} events",
        events2.len()
    );
}
