//! ABOUTME: Integration tests for CAP message serialization and golden files
//! ABOUTME: Tests XML serialization, parsing, and validation against known good examples

use chrono::{DateTime, Utc};
use gl_cap::{
    builder::AlertBuilder, profiles::AlertProfiles, validation::Validate, Alert, Category,
    Certainty, MsgType, ResponseType, Scope, Severity, Status, Urgency,
};

/// Test parsing and validating the test alert golden file
#[test]
fn test_parse_test_alert_golden_file() {
    let xml = include_str!("fixtures/test_alert.xml");

    let alert = Alert::from_xml(xml).expect("Should parse XML");

    // Verify basic structure
    assert_eq!(alert.identifier, "test-alert-123");
    assert_eq!(alert.sender, "test.example.org");
    assert_eq!(alert.status, Status::Test);
    assert_eq!(alert.msg_type, MsgType::Alert);
    assert_eq!(alert.scope, Scope::Public);

    // Verify info block
    assert_eq!(alert.info.len(), 1);
    let info = &alert.info[0];
    assert_eq!(info.event, "Test Event");
    assert_eq!(info.urgency, Urgency::Future);
    assert_eq!(info.severity, Severity::Minor);
    assert_eq!(info.certainty, Certainty::Unknown);
    assert_eq!(
        info.headline,
        Some("TEST ALERT - NO ACTION REQUIRED".to_string())
    );

    // Validate against our validation rules
    assert!(alert.validate().is_ok(), "Alert should pass validation");
}

/// Test parsing the severe weather alert golden file
#[test]
fn test_parse_severe_weather_golden_file() {
    let xml = include_str!("fixtures/severe_weather.xml");

    let alert = Alert::from_xml(xml).expect("Should parse XML");

    // Verify structure
    assert_eq!(alert.identifier, "severe-weather-456");
    assert_eq!(alert.sender, "weather.example.org");
    assert_eq!(alert.status, Status::Actual);

    let info = &alert.info[0];
    assert_eq!(info.event, "Severe Weather Alert");
    assert_eq!(info.urgency, Urgency::Expected);
    assert_eq!(info.severity, Severity::Severe);
    assert_eq!(info.certainty, Certainty::Likely);
    assert!(info.category.iter().any(|c| c.category == Category::Met));
    assert!(info
        .response_type
        .iter()
        .any(|r| r.response_type == ResponseType::Prepare));
    assert!(info
        .response_type
        .iter()
        .any(|r| r.response_type == ResponseType::Monitor));

    // Verify area information
    assert_eq!(info.area.len(), 1);
    let area = &info.area[0];
    assert_eq!(area.area_desc, "Downtown Area");
    assert_eq!(area.circle.len(), 1);
    assert_eq!(area.circle[0], "42.0,-71.0 10.0");
    assert_eq!(area.geocode.len(), 1);
    assert_eq!(area.geocode[0].value_name, "FIPS6");
    assert_eq!(area.geocode[0].value, "25025");

    // Validate
    assert!(alert.validate().is_ok());
}

/// Test XML serialization round-trip (parse -> serialize -> parse)
#[test]
fn test_xml_roundtrip_test_alert() {
    let original_xml = include_str!("fixtures/test_alert.xml");

    // Parse original
    let alert = Alert::from_xml(original_xml).expect("Should parse original XML");

    // Serialize back to XML
    let serialized_xml = alert.to_xml().expect("Should serialize to XML");

    // Parse serialized version
    let reparsed_alert = Alert::from_xml(&serialized_xml).expect("Should parse serialized XML");

    // Verify key fields match
    assert_eq!(alert.identifier, reparsed_alert.identifier);
    assert_eq!(alert.sender, reparsed_alert.sender);
    assert_eq!(alert.status, reparsed_alert.status);
    assert_eq!(alert.msg_type, reparsed_alert.msg_type);
    assert_eq!(alert.scope, reparsed_alert.scope);

    // Verify info matches
    assert_eq!(alert.info.len(), reparsed_alert.info.len());
    if !alert.info.is_empty() {
        let original_info = &alert.info[0];
        let reparsed_info = &reparsed_alert.info[0];

        assert_eq!(original_info.event, reparsed_info.event);
        assert_eq!(original_info.urgency, reparsed_info.urgency);
        assert_eq!(original_info.severity, reparsed_info.severity);
        assert_eq!(original_info.certainty, reparsed_info.certainty);
        assert_eq!(original_info.headline, reparsed_info.headline);
    }
}

/// Test XML roundtrip with builder-created alert
#[test]
fn test_xml_roundtrip_builder_alert() {
    let original_alert = AlertBuilder::new("test.example.org")
        .identifier("roundtrip-test-789")
        .status(Status::Test)
        .add_info(|info| {
            info.event("Roundtrip Test")
                .urgency(Urgency::Future)
                .severity(Severity::Minor)
                .certainty(Certainty::Possible)
                .headline("Roundtrip Test Alert")
                .description("Testing XML roundtrip functionality")
                .add_category(Category::Safety)
                .add_area(|area| {
                    area.area_desc("Test Area")
                        .add_polygon("40.0,-74.0 40.1,-74.0 40.1,-73.9 40.0,-73.9 40.0,-74.0")
                })
        })
        .build();

    // Serialize
    let xml = original_alert.to_xml().expect("Should serialize to XML");

    // Parse back
    let parsed_alert = Alert::from_xml(&xml).expect("Should parse XML");

    // Verify key fields
    assert_eq!(original_alert.identifier, parsed_alert.identifier);
    assert_eq!(original_alert.sender, parsed_alert.sender);
    assert_eq!(original_alert.status, parsed_alert.status);

    // Verify validation passes
    assert!(parsed_alert.validate().is_ok());
}

/// Test that profile-generated alerts serialize properly
#[test]
fn test_profiles_xml_serialization() {
    let profiles = vec![
        ("test", AlertProfiles::test_alert("test.example.org")),
        (
            "severe_weather",
            AlertProfiles::severe_weather("weather.example.org"),
        ),
        ("fire", AlertProfiles::fire_alert("fire.example.org")),
        (
            "public_safety",
            AlertProfiles::public_safety("safety.example.org"),
        ),
    ];

    for (profile_name, builder) in profiles {
        let alert = builder
            .add_circular_area("Test Area", 42.0, -71.0, 10.0)
            .build();

        // Should serialize without error
        let xml = alert
            .to_xml()
            .unwrap_or_else(|e| panic!("Profile {} should serialize to XML: {}", profile_name, e));

        // Should parse back without error
        let parsed = Alert::from_xml(&xml)
            .unwrap_or_else(|e| panic!("Profile {} XML should parse: {}", profile_name, e));

        // Should validate
        parsed
            .validate()
            .unwrap_or_else(|e| panic!("Profile {} should validate: {}", profile_name, e));

        // Basic structure should match
        assert_eq!(alert.identifier, parsed.identifier);
        assert_eq!(alert.sender, parsed.sender);
        assert_eq!(alert.status, parsed.status);
    }
}

/// Test timestamp handling in XML serialization
#[test]
fn test_timestamp_serialization() {
    let test_time: DateTime<Utc> = "2023-06-15T14:30:00Z"
        .parse()
        .expect("Should parse timestamp");

    let alert = AlertBuilder::new("time.example.org")
        .add_info(|info| {
            info.event("Timestamp Test")
                .urgency(Urgency::Future)
                .severity(Severity::Minor)
                .certainty(Certainty::Unknown)
                .effective(test_time)
                .expires(test_time + chrono::Duration::hours(1))
        })
        .build();

    let xml = alert.to_xml().expect("Should serialize");
    let parsed = Alert::from_xml(&xml).expect("Should parse");

    let info = &parsed.info[0];
    assert_eq!(info.effective, Some(test_time));
    assert_eq!(info.expires, Some(test_time + chrono::Duration::hours(1)));
}

/// Test XML structure contains required namespaces and elements
#[test]
fn test_xml_structure_requirements() {
    let alert = AlertProfiles::test_alert("namespace.example.org").build();
    let xml = alert.to_xml().expect("Should serialize");

    // Should contain CAP namespace
    assert!(xml.contains("urn:oasis:names:tc:emergency:cap:1.2"));

    // Should contain required elements
    assert!(xml.contains("<identifier>"));
    assert!(xml.contains("<sender>"));
    assert!(xml.contains("<sent>"));
    assert!(xml.contains("<status>"));
    assert!(xml.contains("<msgType>"));
    assert!(xml.contains("<scope>"));
    assert!(xml.contains("<info>"));
    assert!(xml.contains("<event>"));
    assert!(xml.contains("<urgency>"));
    assert!(xml.contains("<severity>"));
    assert!(xml.contains("<certainty>"));
}

/// Benchmark-style test to ensure reasonable performance
#[test]
fn test_serialization_performance() {
    let alert = AlertProfiles::severe_weather("perf.example.org")
        .add_circular_area("Performance Test Area", 42.0, -71.0, 50.0)
        .build();

    let start = std::time::Instant::now();

    // Serialize and parse multiple times
    for _ in 0..100 {
        let xml = alert.to_xml().expect("Should serialize");
        let _parsed = Alert::from_xml(&xml).expect("Should parse");
    }

    let elapsed = start.elapsed();

    // Should complete 100 roundtrips in reasonable time (less than 1 second)
    assert!(
        elapsed.as_secs() < 1,
        "100 roundtrips should complete in under 1 second, took: {:?}",
        elapsed
    );
}
