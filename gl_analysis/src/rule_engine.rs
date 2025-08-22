//! ABOUTME: Rule engine for evaluating conditions and actions based on events and context
//! ABOUTME: Handles YAML/JSON rules with thresholds, quiet hours, and deduplication logic

use crate::{AnalysisEvent, EventSeverity, ProcessorInput};
use chrono::{DateTime, Datelike, Utc};
use gl_core::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Rule engine for processing analysis rules
pub struct RuleEngine {
    rules: Option<RuleSet>,
    event_history: Vec<AnalysisEvent>,
    max_history_size: usize,
}

/// Set of rules for a template or global configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    /// Individual rules to evaluate
    pub rules: Vec<Rule>,
    /// Global deduplication settings
    pub deduplication: Option<DeduplicationConfig>,
    /// Global quiet hours
    pub quiet_hours: Option<QuietHoursConfig>,
}

/// Individual rule with conditions and actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Rule identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Rule description
    pub description: Option<String>,
    /// Conditions that must be met
    pub conditions: Vec<Condition>,
    /// Actions to take when conditions are met
    pub actions: Vec<Action>,
    /// Whether this rule is enabled
    pub enabled: bool,
    /// Rule priority (higher numbers run first)
    pub priority: i32,
}

/// Condition that can be evaluated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// Type of condition
    pub condition_type: ConditionType,
}

/// Types of conditions that can be evaluated
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConditionType {
    /// Event type matches pattern
    EventType { pattern: String, matches: bool },
    /// Severity comparison
    Severity {
        operator: ComparisonOperator,
        value: EventSeverity,
    },
    /// Confidence threshold
    Confidence {
        operator: ComparisonOperator,
        value: f64,
    },
    /// Time-based condition
    TimeWindow {
        start: String,
        end: String,
        days: Vec<u8>,
    },
    /// Event count in time window
    EventCount {
        event_type: Option<String>,
        count: u32,
        operator: ComparisonOperator,
        window_minutes: u32,
    },
    /// Metadata field condition
    Metadata {
        field: String,
        operator: ComparisonOperator,
        value: serde_json::Value,
    },
    /// Source ID condition
    SourceId { pattern: String, matches: bool },
}

/// Comparison operators for conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Contains,
    NotContains,
}

/// Actions to take when rule conditions are met
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Suppress notification
    SuppressNotification,
    /// Change event severity
    SetSeverity { severity: EventSeverity },
    /// Add metadata to event
    AddMetadata {
        key: String,
        value: serde_json::Value,
    },
    /// Mark event for deletion
    DeleteEvent,
    /// Set custom notification template
    SetNotificationTemplate { template: String },
    /// Rate limit notifications
    RateLimit { max_per_hour: u32 },
}

/// Deduplication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeduplicationConfig {
    /// Deduplication window in minutes
    pub window_minutes: u32,
    /// Event types to deduplicate
    pub event_types: Vec<String>,
    /// Fields to use for deduplication key
    pub key_fields: Vec<String>,
}

/// Quiet hours configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuietHoursConfig {
    /// Start time (HH:MM format)
    pub start_time: String,
    /// End time (HH:MM format)
    pub end_time: String,
    /// Days of week (0=Sunday, 6=Saturday)
    pub days: Vec<u8>,
    /// Actions to take during quiet hours
    pub actions: Vec<Action>,
}

impl RuleEngine {
    /// Create a new rule engine
    pub fn new(rules: Option<RuleSet>) -> Self {
        Self {
            rules,
            event_history: Vec::new(),
            max_history_size: 1000,
        }
    }

    /// Apply rules to events and return modified events
    pub async fn apply_rules(
        &mut self,
        input: &ProcessorInput,
        events: Vec<AnalysisEvent>,
    ) -> Result<Vec<AnalysisEvent>> {
        let Some(rule_set) = &self.rules else {
            debug!("No rules configured, returning events unchanged");
            self.update_history(&events);
            return Ok(events);
        };

        debug!(
            "Applying {} rules to {} events",
            rule_set.rules.len(),
            events.len()
        );

        // Sort rules by priority (highest first)
        let mut rules = rule_set.rules.clone();
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        // Apply each rule to each event
        let mut events_to_keep = Vec::new();

        let events_count = events.len();
        for mut event in events {
            let mut keep_event = true;

            // Apply individual rules
            for rule in &rules {
                if !rule.enabled {
                    continue;
                }

                if self.evaluate_rule_conditions(rule, &event, input).await? {
                    debug!(
                        "Rule '{}' matched for event {}",
                        rule.name, event.event_type
                    );

                    for action in &rule.actions {
                        match self.apply_action(action, &mut event).await {
                            Ok(should_keep) => {
                                if !should_keep {
                                    keep_event = false;
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to apply action in rule '{}': {}", rule.name, e);
                            }
                        }
                    }

                    if !keep_event {
                        break;
                    }
                }
            }

            // Apply global deduplication
            if keep_event {
                if let Some(dedup_config) = &rule_set.deduplication {
                    keep_event = self.check_deduplication(&event, dedup_config);
                }
            }

            // Apply global quiet hours
            if keep_event {
                if let Some(quiet_hours) = &rule_set.quiet_hours {
                    self.apply_quiet_hours(&mut event, quiet_hours);
                }
            }

            if keep_event {
                events_to_keep.push(event);
            }
        }

        self.update_history(&events_to_keep);

        debug!(
            "Rule engine kept {} out of {} events",
            events_to_keep.len(),
            events_count
        );

        Ok(events_to_keep)
    }

    /// Evaluate conditions for a rule
    async fn evaluate_rule_conditions(
        &self,
        rule: &Rule,
        event: &AnalysisEvent,
        _input: &ProcessorInput,
    ) -> Result<bool> {
        if rule.conditions.is_empty() {
            return Ok(true);
        }

        // All conditions must be true (AND logic)
        for condition in &rule.conditions {
            if !self
                .evaluate_condition(&condition.condition_type, event)
                .await?
            {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Evaluate a single condition
    async fn evaluate_condition(
        &self,
        condition: &ConditionType,
        event: &AnalysisEvent,
    ) -> Result<bool> {
        match condition {
            ConditionType::EventType { pattern, matches } => {
                let pattern_matches = if pattern.contains('*') {
                    // Simple wildcard matching
                    let pattern_regex = pattern.replace('*', ".*");
                    regex::Regex::new(&pattern_regex)
                        .map_err(|e| gl_core::Error::Validation(format!("Invalid pattern: {}", e)))?
                        .is_match(&event.event_type)
                } else {
                    event.event_type == *pattern
                };
                Ok(pattern_matches == *matches)
            }

            ConditionType::Severity { operator, value } => {
                Ok(self.compare_severity(&event.severity, operator, value))
            }

            ConditionType::Confidence { operator, value } => {
                Ok(self.compare_numeric(event.confidence, operator, *value))
            }

            ConditionType::TimeWindow { start, end, days } => {
                Ok(self.is_in_time_window(&event.timestamp, start, end, days))
            }

            ConditionType::EventCount {
                event_type,
                count,
                operator,
                window_minutes,
            } => {
                let matching_count =
                    self.count_recent_events(event_type.as_deref(), *window_minutes);
                Ok(self.compare_numeric(matching_count as f64, operator, *count as f64))
            }

            ConditionType::Metadata {
                field,
                operator,
                value,
            } => {
                let event_value = event.metadata.get(field);
                Ok(self.compare_metadata_value(event_value, operator, value))
            }

            ConditionType::SourceId { pattern, matches } => {
                let pattern_matches = if pattern.contains('*') {
                    let pattern_regex = pattern.replace('*', ".*");
                    regex::Regex::new(&pattern_regex)
                        .map_err(|e| gl_core::Error::Validation(format!("Invalid pattern: {}", e)))?
                        .is_match(&event.source_id)
                } else {
                    event.source_id == *pattern
                };
                Ok(pattern_matches == *matches)
            }
        }
    }

    /// Apply an action to an event, returning whether to keep the event
    async fn apply_action(&self, action: &Action, event: &mut AnalysisEvent) -> Result<bool> {
        match action {
            Action::SuppressNotification => {
                debug!("Suppressing notification for event {}", event.event_type);
                event.should_notify = false;
                Ok(true)
            }

            Action::SetSeverity { severity } => {
                debug!(
                    "Changing severity from {:?} to {:?}",
                    event.severity, severity
                );
                event.severity = severity.clone();
                Ok(true)
            }

            Action::AddMetadata { key, value } => {
                debug!("Adding metadata {}={:?}", key, value);
                event.metadata.insert(key.clone(), value.clone());
                Ok(true)
            }

            Action::DeleteEvent => {
                debug!("Deleting event {}", event.event_type);
                Ok(false) // Don't keep the event
            }

            Action::SetNotificationTemplate { template } => {
                debug!("Setting notification template: {}", template);
                event
                    .metadata
                    .insert("notification_template".to_string(), template.clone().into());
                Ok(true)
            }

            Action::RateLimit { max_per_hour: _ } => {
                // Rate limiting would be handled by the notification system
                debug!("Rate limiting configured for event");
                Ok(true)
            }
        }
    }

    /// Check if event should be deduplicated
    fn check_deduplication(&self, event: &AnalysisEvent, config: &DeduplicationConfig) -> bool {
        if !config.event_types.contains(&event.event_type) {
            return true; // Not subject to deduplication
        }

        let cutoff_time = Utc::now() - chrono::Duration::minutes(config.window_minutes as i64);

        // Generate deduplication key
        let mut key_parts = Vec::new();
        for field in &config.key_fields {
            match field.as_str() {
                "event_type" => key_parts.push(event.event_type.clone()),
                "source_id" => key_parts.push(event.source_id.clone()),
                "template_id" => key_parts.push(event.template_id.clone()),
                _ => {
                    if let Some(value) = event.metadata.get(field) {
                        key_parts.push(value.to_string());
                    }
                }
            }
        }
        let dedup_key = key_parts.join("|");

        // Check if we have a recent event with the same key
        let has_recent_duplicate = self
            .event_history
            .iter()
            .filter(|e| e.timestamp > cutoff_time)
            .filter(|e| e.event_type == event.event_type)
            .any(|e| {
                // Reconstruct key for historical event
                let mut hist_key_parts = Vec::new();
                for field in &config.key_fields {
                    match field.as_str() {
                        "event_type" => hist_key_parts.push(e.event_type.clone()),
                        "source_id" => hist_key_parts.push(e.source_id.clone()),
                        "template_id" => hist_key_parts.push(e.template_id.clone()),
                        _ => {
                            if let Some(value) = e.metadata.get(field) {
                                hist_key_parts.push(value.to_string());
                            }
                        }
                    }
                }
                let hist_dedup_key = hist_key_parts.join("|");
                hist_dedup_key == dedup_key
            });

        if has_recent_duplicate {
            debug!(
                "Deduplicating event {} with key: {}",
                event.event_type, dedup_key
            );
            false
        } else {
            true
        }
    }

    /// Apply quiet hours configuration
    fn apply_quiet_hours(&self, event: &mut AnalysisEvent, config: &QuietHoursConfig) {
        if self.is_in_time_window(
            &event.timestamp,
            &config.start_time,
            &config.end_time,
            &config.days,
        ) {
            debug!("Applying quiet hours actions to event {}", event.event_type);

            // Apply quiet hours actions (typically suppress notifications)
            for action in &config.actions {
                match action {
                    Action::SuppressNotification => {
                        event.should_notify = false;
                    }
                    Action::SetSeverity { severity } => {
                        event.severity = severity.clone();
                    }
                    _ => {} // Other actions could be supported
                }
            }
        }
    }

    /// Helper function to compare severities
    fn compare_severity(
        &self,
        event_severity: &EventSeverity,
        op: &ComparisonOperator,
        target: &EventSeverity,
    ) -> bool {
        match op {
            ComparisonOperator::Equal => event_severity == target,
            ComparisonOperator::NotEqual => event_severity != target,
            ComparisonOperator::GreaterThan => event_severity > target,
            ComparisonOperator::GreaterThanOrEqual => event_severity >= target,
            ComparisonOperator::LessThan => event_severity < target,
            ComparisonOperator::LessThanOrEqual => event_severity <= target,
            _ => false,
        }
    }

    /// Helper function to compare numeric values
    fn compare_numeric(&self, event_value: f64, op: &ComparisonOperator, target: f64) -> bool {
        match op {
            ComparisonOperator::Equal => (event_value - target).abs() < f64::EPSILON,
            ComparisonOperator::NotEqual => (event_value - target).abs() > f64::EPSILON,
            ComparisonOperator::GreaterThan => event_value > target,
            ComparisonOperator::GreaterThanOrEqual => event_value >= target,
            ComparisonOperator::LessThan => event_value < target,
            ComparisonOperator::LessThanOrEqual => event_value <= target,
            _ => false,
        }
    }

    /// Helper function to check if timestamp is in time window
    fn is_in_time_window(
        &self,
        timestamp: &DateTime<Utc>,
        start: &str,
        end: &str,
        days: &[u8],
    ) -> bool {
        let weekday = timestamp.weekday().num_days_from_sunday() as u8;

        if !days.contains(&weekday) {
            return false;
        }

        let time_str = timestamp.format("%H:%M").to_string();

        if start <= end {
            // Same day window
            time_str.as_str() >= start && time_str.as_str() <= end
        } else {
            // Overnight window
            time_str.as_str() >= start || time_str.as_str() <= end
        }
    }

    /// Helper function to compare metadata values
    fn compare_metadata_value(
        &self,
        event_value: Option<&serde_json::Value>,
        op: &ComparisonOperator,
        target: &serde_json::Value,
    ) -> bool {
        let Some(event_value) = event_value else {
            return matches!(op, ComparisonOperator::NotEqual);
        };

        match op {
            ComparisonOperator::Equal => event_value == target,
            ComparisonOperator::NotEqual => event_value != target,
            ComparisonOperator::Contains => {
                if let (Some(event_str), Some(target_str)) = (event_value.as_str(), target.as_str())
                {
                    event_str.contains(target_str)
                } else {
                    false
                }
            }
            ComparisonOperator::NotContains => {
                if let (Some(event_str), Some(target_str)) = (event_value.as_str(), target.as_str())
                {
                    !event_str.contains(target_str)
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Count recent events matching criteria
    fn count_recent_events(&self, event_type: Option<&str>, window_minutes: u32) -> u32 {
        let cutoff_time = Utc::now() - chrono::Duration::minutes(window_minutes as i64);

        self.event_history
            .iter()
            .filter(|e| e.timestamp > cutoff_time)
            .filter(|e| event_type.map_or(true, |et| e.event_type == et))
            .count() as u32
    }

    /// Update event history
    fn update_history(&mut self, new_events: &[AnalysisEvent]) {
        self.event_history.extend(new_events.iter().cloned());

        // Keep only recent events to prevent unbounded growth
        if self.event_history.len() > self.max_history_size {
            let keep_from = self.event_history.len() - self.max_history_size;
            self.event_history.drain(0..keep_from);
        }
    }

    /// Clear event history
    pub fn clear_history(&mut self) {
        self.event_history.clear();
    }

    /// Get event history size
    pub fn history_size(&self) -> usize {
        self.event_history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisEvent, EventSeverity, ProcessorContext};
    use chrono::{TimeZone, Utc};

    fn create_test_event() -> AnalysisEvent {
        AnalysisEvent::new(
            "test_template".to_string(),
            "motion_detected".to_string(),
            EventSeverity::Medium,
            0.8,
            "Test motion event".to_string(),
            "motion".to_string(),
            "camera_01".to_string(),
        )
    }

    fn create_test_input() -> ProcessorInput {
        ProcessorInput {
            template_id: "test_template".to_string(),
            frame_data: None,
            frame_format: None,
            text_content: None,
            context: ProcessorContext::new("camera_01".to_string()),
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_rule_engine_no_rules() {
        let mut engine = RuleEngine::new(None);
        let events = vec![create_test_event()];
        let input = create_test_input();

        let result = engine.apply_rules(&input, events).await.unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_severity_condition() {
        let rule = Rule {
            id: "test_rule".to_string(),
            name: "Test Rule".to_string(),
            description: None,
            conditions: vec![Condition {
                condition_type: ConditionType::Severity {
                    operator: ComparisonOperator::GreaterThan,
                    value: EventSeverity::Low,
                },
            }],
            actions: vec![Action::SuppressNotification],
            enabled: true,
            priority: 0,
        };

        let rule_set = RuleSet {
            rules: vec![rule],
            deduplication: None,
            quiet_hours: None,
        };

        let mut engine = RuleEngine::new(Some(rule_set));
        let mut event = create_test_event();
        event.severity = EventSeverity::Medium; // Greater than Low

        let input = create_test_input();
        let result = engine.apply_rules(&input, vec![event]).await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(!result[0].should_notify); // Should be suppressed
    }

    #[tokio::test]
    async fn test_event_type_condition() {
        let rule = Rule {
            id: "motion_rule".to_string(),
            name: "Motion Rule".to_string(),
            description: None,
            conditions: vec![Condition {
                condition_type: ConditionType::EventType {
                    pattern: "motion_*".to_string(),
                    matches: true,
                },
            }],
            actions: vec![Action::SetSeverity {
                severity: EventSeverity::High,
            }],
            enabled: true,
            priority: 0,
        };

        let rule_set = RuleSet {
            rules: vec![rule],
            deduplication: None,
            quiet_hours: None,
        };

        let mut engine = RuleEngine::new(Some(rule_set));
        let event = create_test_event(); // event_type is "motion_detected"

        let input = create_test_input();
        let result = engine.apply_rules(&input, vec![event]).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].severity, EventSeverity::High);
    }

    #[tokio::test]
    async fn test_delete_event_action() {
        let rule = Rule {
            id: "delete_rule".to_string(),
            name: "Delete Rule".to_string(),
            description: None,
            conditions: vec![Condition {
                condition_type: ConditionType::Confidence {
                    operator: ComparisonOperator::LessThan,
                    value: 0.5,
                },
            }],
            actions: vec![Action::DeleteEvent],
            enabled: true,
            priority: 0,
        };

        let rule_set = RuleSet {
            rules: vec![rule],
            deduplication: None,
            quiet_hours: None,
        };

        let mut engine = RuleEngine::new(Some(rule_set));
        let mut event = create_test_event();
        event.confidence = 0.3; // Less than 0.5

        let input = create_test_input();
        let result = engine.apply_rules(&input, vec![event]).await.unwrap();

        assert_eq!(result.len(), 0); // Event should be deleted
    }

    #[tokio::test]
    async fn test_deduplication() {
        let dedup_config = DeduplicationConfig {
            window_minutes: 5,
            event_types: vec!["motion_detected".to_string()],
            key_fields: vec!["event_type".to_string(), "source_id".to_string()],
        };

        let rule_set = RuleSet {
            rules: vec![],
            deduplication: Some(dedup_config),
            quiet_hours: None,
        };

        let mut engine = RuleEngine::new(Some(rule_set));

        // First event should pass through
        let event1 = create_test_event();
        let input = create_test_input();
        let result1 = engine.apply_rules(&input, vec![event1]).await.unwrap();
        assert_eq!(result1.len(), 1);

        // Second identical event should be deduplicated
        let event2 = create_test_event();
        let result2 = engine.apply_rules(&input, vec![event2]).await.unwrap();
        assert_eq!(result2.len(), 0); // Should be deduplicated
    }

    #[test]
    fn test_time_window_evaluation() {
        let engine = RuleEngine::new(None);

        // Create a timestamp at 15:30
        let test_time = Utc.with_ymd_and_hms(2023, 12, 1, 15, 30, 0).unwrap(); // Friday
        let friday = 5u8;

        // Test within window
        assert!(engine.is_in_time_window(&test_time, "15:00", "16:00", &[friday]));

        // Test outside window
        assert!(!engine.is_in_time_window(&test_time, "16:00", "17:00", &[friday]));

        // Test wrong day
        assert!(!engine.is_in_time_window(&test_time, "15:00", "16:00", &[6u8])); // Saturday

        // Test overnight window
        assert!(engine.is_in_time_window(&test_time, "14:00", "16:00", &[friday]));
    }
}
