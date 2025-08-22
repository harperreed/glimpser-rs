//! ABOUTME: AI client abstraction with OpenAI and stub implementations
//! ABOUTME: Provides AI-powered analysis and content generation

use async_trait::async_trait;
use bytes::Bytes;
use gl_core::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

pub mod stub;
#[cfg(feature = "ai_online")]
pub mod openai;

pub use stub::StubClient;
#[cfg(feature = "ai_online")]
pub use openai::OpenAiClient;

/// Classification result for events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventClassification {
    /// Motion detected
    Motion,
    /// Person detected
    Person,
    /// Vehicle detected
    Vehicle,
    /// Animal detected
    Animal,
    /// Fire or smoke detected
    Fire,
    /// Suspicious activity
    Suspicious,
    /// Normal/no significant event
    Normal,
    /// Unknown or unclassifiable
    Unknown,
}

impl Default for EventClassification {
    fn default() -> Self {
        Self::Unknown
    }
}

/// Event data for classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventData {
    /// Type of event detected
    pub event_type: String,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
    /// Additional metadata
    pub metadata: serde_json::Value,
    /// Timestamp when event occurred
    pub timestamp: String,
    /// Source identifier (template_id, camera_id, etc.)
    pub source_id: String,
}

/// Request for text summarization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizeRequest {
    pub text: String,
    pub max_length: Option<usize>,
    pub style: Option<String>, // "brief", "detailed", "technical"
}

/// Response from text summarization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummarizeResponse {
    pub summary: String,
    pub original_length: usize,
    pub summary_length: usize,
    pub confidence: Option<f64>,
}

/// Request for frame description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeFrameRequest {
    #[serde(skip_serializing, skip_deserializing)]
    pub image_data: Bytes,
    pub image_format: String, // "jpeg", "png"
    pub detail_level: Option<String>, // "low", "high", "auto"
    pub focus: Option<String>, // "objects", "activity", "scene"
}

/// Response from frame description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescribeFrameResponse {
    pub description: String,
    pub objects_detected: Vec<String>,
    pub confidence: Option<f64>,
    pub processing_time_ms: Option<u64>,
}

/// Request for event classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyEventRequest {
    pub event_data: EventData,
    pub context: Option<String>,
    pub threshold: Option<f64>, // Minimum confidence threshold
}

/// Response from event classification  
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyEventResponse {
    pub classification: EventClassification,
    pub confidence: f64,
    pub reasoning: String,
    pub suggested_actions: Vec<String>,
}

/// Configuration for AI clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// API key for online services
    pub api_key: Option<String>,
    /// Base URL for API (defaults to OpenAI)
    pub base_url: Option<String>,
    /// Request timeout in seconds
    pub timeout_seconds: u64,
    /// Maximum retries for failed requests
    pub max_retries: u32,
    /// Model name to use (e.g., "gpt-4", "gpt-3.5-turbo")
    pub model: String,
    /// Whether to use online AI services
    pub use_online: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            timeout_seconds: 30,
            max_retries: 3,
            model: "gpt-3.5-turbo".to_string(),
            use_online: false, // Default to stub for safety
        }
    }
}

/// Trait for AI client implementations
#[async_trait]
pub trait AiClient: Send + Sync {
    /// Summarize the given text
    async fn summarize(&self, request: SummarizeRequest) -> Result<SummarizeResponse>;
    
    /// Describe the contents of a frame/image
    async fn describe_frame(&self, request: DescribeFrameRequest) -> Result<DescribeFrameResponse>;
    
    /// Classify an event based on provided data
    async fn classify_event(&self, request: ClassifyEventRequest) -> Result<ClassifyEventResponse>;
    
    /// Health check for the AI service
    async fn health_check(&self) -> Result<()>;
}

/// Create an AI client based on configuration
pub fn create_client(config: AiConfig) -> Box<dyn AiClient> {
    if config.use_online {
        #[cfg(feature = "ai_online")]
        {
            info!("Creating OpenAI client with model: {}", config.model);
            Box::new(OpenAiClient::new(config))
        }
        #[cfg(not(feature = "ai_online"))]
        {
            warn!("Online AI requested but ai_online feature not enabled, falling back to stub");
            Box::new(StubClient::new())
        }
    } else {
        debug!("Creating stub AI client");
        Box::new(StubClient::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_event_classification_serialization() {
        let classifications = vec![
            EventClassification::Motion,
            EventClassification::Person,
            EventClassification::Vehicle,
            EventClassification::Animal,
            EventClassification::Fire,
            EventClassification::Suspicious,
            EventClassification::Normal,
            EventClassification::Unknown,
        ];
        
        for classification in classifications {
            let json = serde_json::to_string(&classification).unwrap();
            let deserialized: EventClassification = serde_json::from_str(&json).unwrap();
            assert_eq!(classification, deserialized);
        }
    }
    
    #[test]
    fn test_ai_config_default() {
        let config = AiConfig::default();
        assert!(!config.use_online);
        assert_eq!(config.model, "gpt-3.5-turbo");
        assert_eq!(config.timeout_seconds, 30);
        assert_eq!(config.max_retries, 3);
        assert!(config.api_key.is_none());
    }
    
    #[test]
    fn test_event_data_creation() {
        let event = EventData {
            event_type: "motion_detected".to_string(),
            confidence: 0.85,
            metadata: serde_json::json!({"region": "front_door"}),
            timestamp: "2023-12-01T10:00:00Z".to_string(),
            source_id: "camera_01".to_string(),
        };
        
        assert_eq!(event.event_type, "motion_detected");
        assert_eq!(event.confidence, 0.85);
        assert_eq!(event.source_id, "camera_01");
    }
    
    #[test]
    fn test_summarize_request_creation() {
        let request = SummarizeRequest {
            text: "This is a long text that needs to be summarized for better readability and understanding.".to_string(),
            max_length: Some(50),
            style: Some("brief".to_string()),
        };
        
        assert!(request.text.len() > 50);
        assert_eq!(request.max_length, Some(50));
        assert_eq!(request.style, Some("brief".to_string()));
    }
    
    #[test]
    fn test_describe_frame_request_creation() {
        let request = DescribeFrameRequest {
            image_data: Bytes::from("fake_jpeg_data"),
            image_format: "jpeg".to_string(),
            detail_level: Some("high".to_string()),
            focus: Some("objects".to_string()),
        };
        
        assert_eq!(request.image_format, "jpeg");
        assert_eq!(request.detail_level, Some("high".to_string()));
        assert_eq!(request.focus, Some("objects".to_string()));
    }
    
    #[tokio::test]
    async fn test_create_client_stub_default() {
        let config = AiConfig::default(); // use_online = false by default
        let client = create_client(config);
        
        // Should create stub client successfully
        let health_result = client.health_check().await;
        assert!(health_result.is_ok());
    }
    
    #[tokio::test]
    async fn test_create_client_online_behavior() {
        let config = AiConfig {
            use_online: true,
            ..Default::default()
        };
        
        let client = create_client(config);
        
        #[cfg(feature = "ai_online")]
        {
            // With ai_online feature, should create OpenAI client that requires API key
            let health_result = client.health_check().await;
            assert!(health_result.is_err()); // Should fail without API key
        }
        
        #[cfg(not(feature = "ai_online"))]
        {
            // Without ai_online feature, should fall back to stub
            let health_result = client.health_check().await;
            assert!(health_result.is_ok());
        }
    }
}