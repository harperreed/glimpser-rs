//! ABOUTME: Stub AI client that returns canned responses for testing
//! ABOUTME: No network calls, deterministic responses for CI/development

use async_trait::async_trait;
use gl_core::Result;
use tracing::debug;

use crate::{
    AiClient, ClassifyEventRequest, ClassifyEventResponse, DescribeFrameRequest,
    DescribeFrameResponse, EventClassification, SummarizeRequest, SummarizeResponse,
};

/// Stub AI client that returns predetermined responses
pub struct StubClient;

impl StubClient {
    pub fn new() -> Self {
        debug!("Creating stub AI client");
        Self
    }

    /// Generate a deterministic but somewhat realistic summary
    fn generate_stub_summary(&self, text: &str, max_length: Option<usize>) -> String {
        let words: Vec<&str> = text.split_whitespace().collect();
        let target_length = max_length.unwrap_or(50).min(words.len());

        if words.len() <= target_length {
            return text.to_string();
        }

        // Take first portion and add ellipsis
        let summary_words = &words[..target_length];
        let mut summary = summary_words.join(" ");

        // Add contextual ending based on content
        if text.to_lowercase().contains("error") || text.to_lowercase().contains("failed") {
            summary.push_str("... [Error summary]");
        } else if text.to_lowercase().contains("success")
            || text.to_lowercase().contains("complete")
        {
            summary.push_str("... [Success summary]");
        } else {
            summary.push_str("...");
        }

        summary
    }

    /// Generate a deterministic frame description based on image size
    fn generate_stub_description(&self, image_size: usize, format: &str) -> (String, Vec<String>) {
        let objects = match image_size % 5 {
            0 => vec!["person".to_string(), "chair".to_string()],
            1 => vec![
                "car".to_string(),
                "tree".to_string(),
                "building".to_string(),
            ],
            2 => vec!["dog".to_string(), "grass".to_string()],
            3 => vec![
                "monitor".to_string(),
                "keyboard".to_string(),
                "desk".to_string(),
            ],
            _ => vec!["outdoor scene".to_string()],
        };

        let description = match format {
            "jpeg" => {
                if objects.contains(&"person".to_string()) {
                    "A person sitting on a chair in an indoor setting with good lighting."
                } else if objects.contains(&"car".to_string()) {
                    "An outdoor scene with a car parked near a tree and building in the background."
                } else if objects.contains(&"dog".to_string()) {
                    "A dog playing on grass in what appears to be a park or yard setting."
                } else if objects.contains(&"monitor".to_string()) {
                    "A workspace setup with a computer monitor, keyboard, and organized desk area."
                } else {
                    "An outdoor landscape scene with natural elements and clear visibility."
                }
            }
            "png" => {
                "A high-quality image with clear details and sharp edges, likely a screenshot or graphic."
            }
            _ => {
                "An image in an uncommon format with basic visual content."
            }
        };

        (description.to_string(), objects)
    }

    /// Generate event classification based on event type
    fn classify_stub_event(
        &self,
        event_type: &str,
        confidence: f64,
    ) -> (EventClassification, f64, String) {
        let (classification, adjusted_confidence, reasoning) = match event_type.to_lowercase().as_str() {
            s if s.contains("motion") => (
                EventClassification::Motion,
                confidence.max(0.8),
                "Motion detected based on pixel differences between frames."
            ),
            s if s.contains("person") || s.contains("human") => (
                EventClassification::Person,
                confidence.max(0.85),
                "Human figure detected with high confidence based on body shape and movement patterns."
            ),
            s if s.contains("car") || s.contains("vehicle") => (
                EventClassification::Vehicle,
                confidence.max(0.9),
                "Vehicle detected based on shape, size, and movement characteristics."
            ),
            s if s.contains("animal") || s.contains("dog") || s.contains("cat") => (
                EventClassification::Animal,
                confidence.max(0.75),
                "Animal detected based on movement patterns and body characteristics."
            ),
            s if s.contains("fire") || s.contains("smoke") => (
                EventClassification::Fire,
                confidence.max(0.95),
                "Fire or smoke detected based on color patterns and movement characteristics - requires immediate attention."
            ),
            s if s.contains("suspicious") || s.contains("unusual") => (
                EventClassification::Suspicious,
                confidence.max(0.7),
                "Unusual activity pattern detected that may require investigation."
            ),
            s if s.contains("normal") || s.contains("regular") => (
                EventClassification::Normal,
                confidence.max(0.8),
                "Normal activity detected, no unusual patterns or events."
            ),
            _ => (
                EventClassification::Unknown,
                confidence.min(0.5),
                "Event type could not be classified with sufficient confidence."
            ),
        };

        (classification, adjusted_confidence, reasoning.to_string())
    }
}

impl Default for StubClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AiClient for StubClient {
    async fn summarize(&self, request: SummarizeRequest) -> Result<SummarizeResponse> {
        debug!(
            "Stub client summarizing text of {} characters",
            request.text.len()
        );

        // Simulate processing delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let summary = self.generate_stub_summary(&request.text, request.max_length);

        Ok(SummarizeResponse {
            original_length: request.text.len(),
            summary_length: summary.len(),
            summary,
            confidence: Some(0.85), // Stub confidence
        })
    }

    async fn describe_frame(&self, request: DescribeFrameRequest) -> Result<DescribeFrameResponse> {
        debug!(
            "Stub client describing frame of {} bytes in {} format",
            request.image_data.len(),
            request.image_format
        );

        // Simulate processing delay based on image size
        let delay_ms = (request.image_data.len() / 1000).clamp(50, 500);
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms as u64)).await;

        let (description, objects) =
            self.generate_stub_description(request.image_data.len(), &request.image_format);

        Ok(DescribeFrameResponse {
            description,
            objects_detected: objects,
            confidence: Some(0.8),
            processing_time_ms: Some(delay_ms as u64),
        })
    }

    async fn classify_event(&self, request: ClassifyEventRequest) -> Result<ClassifyEventResponse> {
        debug!(
            "Stub client classifying event: {} with confidence {}",
            request.event_data.event_type, request.event_data.confidence
        );

        // Simulate processing delay
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let (classification, confidence, reasoning) = self.classify_stub_event(
            &request.event_data.event_type,
            request.event_data.confidence,
        );

        let suggested_actions = match classification {
            EventClassification::Fire => vec![
                "Immediately alert fire department".to_string(),
                "Evacuate area if safe to do so".to_string(),
                "Monitor situation continuously".to_string(),
            ],
            EventClassification::Suspicious => vec![
                "Review additional camera angles".to_string(),
                "Consider alerting security personnel".to_string(),
                "Log event for pattern analysis".to_string(),
            ],
            EventClassification::Person | EventClassification::Vehicle => vec![
                "Log event for traffic analysis".to_string(),
                "Continue monitoring".to_string(),
            ],
            EventClassification::Motion | EventClassification::Animal => vec![
                "Continue monitoring".to_string(),
                "Log event for pattern tracking".to_string(),
            ],
            EventClassification::Normal => vec!["No action required".to_string()],
            EventClassification::Unknown => vec![
                "Review event data".to_string(),
                "Consider manual classification".to_string(),
            ],
        };

        Ok(ClassifyEventResponse {
            classification,
            confidence,
            reasoning,
            suggested_actions,
        })
    }

    async fn health_check(&self) -> Result<()> {
        debug!("Stub client health check - always healthy");
        // Stub is always healthy
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EventData;
    use bytes::Bytes;

    #[tokio::test]
    async fn test_stub_summarize() {
        let client = StubClient::new();
        let request = SummarizeRequest {
            text: "This is a very long piece of text that needs to be summarized into something much shorter for better readability and comprehension by users.".to_string(),
            max_length: Some(20),
            style: Some("brief".to_string()),
        };

        let response = client.summarize(request).await.unwrap();

        assert!(response.summary.len() < response.original_length);
        assert!(response.summary_length > 0);
        assert!(response.confidence.is_some());
        assert!(response.confidence.unwrap() > 0.0);
    }

    #[tokio::test]
    async fn test_stub_describe_frame() {
        let client = StubClient::new();
        let request = DescribeFrameRequest {
            image_data: Bytes::from(vec![0u8; 1000]), // 1KB fake image
            image_format: "jpeg".to_string(),
            detail_level: Some("high".to_string()),
            focus: Some("objects".to_string()),
        };

        let response = client.describe_frame(request).await.unwrap();

        assert!(!response.description.is_empty());
        assert!(!response.objects_detected.is_empty());
        assert!(response.confidence.is_some());
        assert!(response.processing_time_ms.is_some());
    }

    #[tokio::test]
    async fn test_stub_classify_event_motion() {
        let client = StubClient::new();
        let event_data = EventData {
            event_type: "motion_detected".to_string(),
            confidence: 0.7,
            metadata: serde_json::json!({"region": "entrance"}),
            timestamp: "2023-12-01T10:00:00Z".to_string(),
            source_id: "camera_01".to_string(),
        };

        let request = ClassifyEventRequest {
            event_data,
            context: None,
            threshold: Some(0.8),
        };

        let response = client.classify_event(request).await.unwrap();

        assert_eq!(response.classification, EventClassification::Motion);
        assert!(response.confidence >= 0.8);
        assert!(!response.reasoning.is_empty());
        assert!(!response.suggested_actions.is_empty());
    }

    #[tokio::test]
    async fn test_stub_classify_event_fire() {
        let client = StubClient::new();
        let event_data = EventData {
            event_type: "fire_detected".to_string(),
            confidence: 0.9,
            metadata: serde_json::json!({"temperature": "high"}),
            timestamp: "2023-12-01T10:00:00Z".to_string(),
            source_id: "sensor_01".to_string(),
        };

        let request = ClassifyEventRequest {
            event_data,
            context: None,
            threshold: None,
        };

        let response = client.classify_event(request).await.unwrap();

        assert_eq!(response.classification, EventClassification::Fire);
        assert!(response.confidence >= 0.9);
        assert!(response
            .suggested_actions
            .contains(&"Immediately alert fire department".to_string()));
    }

    #[tokio::test]
    async fn test_stub_health_check() {
        let client = StubClient::new();
        let result = client.health_check().await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_stub_generate_summary() {
        let client = StubClient::new();
        let text = "This is a test text that should be summarized properly";

        let summary = client.generate_stub_summary(text, Some(5));
        assert!(summary.len() < text.len());
        assert!(summary.ends_with("..."));
    }

    #[test]
    fn test_stub_generate_description_deterministic() {
        let client = StubClient::new();

        // Same input should give same output
        let (desc1, objs1) = client.generate_stub_description(100, "jpeg");
        let (desc2, objs2) = client.generate_stub_description(100, "jpeg");

        assert_eq!(desc1, desc2);
        assert_eq!(objs1, objs2);
    }

    #[test]
    fn test_stub_classify_different_events() {
        let client = StubClient::new();

        let (class1, _, _) = client.classify_stub_event("person_detected", 0.8);
        let (class2, _, _) = client.classify_stub_event("vehicle_moving", 0.7);
        let (class3, _, _) = client.classify_stub_event("fire_alarm", 0.9);

        assert_eq!(class1, EventClassification::Person);
        assert_eq!(class2, EventClassification::Vehicle);
        assert_eq!(class3, EventClassification::Fire);
    }
}
