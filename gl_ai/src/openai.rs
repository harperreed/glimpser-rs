//! ABOUTME: OpenAI client implementation with authentication and retry logic  
//! ABOUTME: Provides online AI services via OpenAI API with proper error handling

use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn, error};

use crate::{
    AiClient, AiConfig, ClassifyEventRequest, ClassifyEventResponse, DescribeFrameRequest, 
    DescribeFrameResponse, EventClassification, SummarizeRequest, SummarizeResponse
};

/// OpenAI API client with authentication and retry logic
pub struct OpenAiClient {
    client: Client,
    config: AiConfig,
    base_url: String,
}

/// OpenAI API request for text summarization
#[derive(Debug, Serialize)]
struct OpenAiSummarizeRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    max_tokens: Option<u32>,
    temperature: f32,
}

/// OpenAI API request for vision/image analysis
#[derive(Debug, Serialize)]
struct OpenAiVisionRequest {
    model: String,
    messages: Vec<OpenAiVisionMessage>,
    max_tokens: Option<u32>,
    temperature: f32,
}

/// OpenAI message format
#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

/// OpenAI vision message with image content
#[derive(Debug, Serialize)]
struct OpenAiVisionMessage {
    role: String,
    content: Vec<OpenAiContent>,
}

/// OpenAI content for vision requests
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OpenAiContent {
    Text { text: String },
    ImageUrl { image_url: OpenAiImageUrl },
}

/// OpenAI image URL format
#[derive(Debug, Serialize)]
struct OpenAiImageUrl {
    url: String,
}

/// OpenAI API response format
#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

/// OpenAI choice in response
#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    finish_reason: Option<String>,
}

/// OpenAI response message
#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

/// OpenAI usage statistics
#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    total_tokens: u32,
    prompt_tokens: u32,
    completion_tokens: u32,
}

impl OpenAiClient {
    pub fn new(config: AiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");
        
        let base_url = config.base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
        
        debug!("Created OpenAI client with base URL: {}", base_url);
        
        Self {
            client,
            config,
            base_url,
        }
    }
    
    /// Execute a request with retry logic
    async fn execute_with_retry<T>(&self, request_builder: RequestBuilder) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let mut last_error = None;
        
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let delay = Duration::from_millis(100 * (1 << attempt.min(5))); // Exponential backoff
                debug!("Retrying request in {:?} (attempt {})", delay, attempt + 1);
                sleep(delay).await;
            }
            
            let request = match request_builder.try_clone() {
                Some(req) => req,
                None => {
                    error!("Failed to clone request for retry");
                    return Err(Error::Config("Unable to retry request - body not cloneable".to_string()));
                }
            };
            
            match self.execute_request::<T>(request).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!("Request attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "All retry attempts failed"))))
    }
    
    /// Execute a single HTTP request
    async fn execute_request<T>(&self, request: RequestBuilder) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let response = request.send().await
            .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("HTTP request failed: {}", e))))?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Error::Database(format!("OpenAI API error ({}): {}", status, error_text)));
        }
        
        let response_text = response.text().await
            .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read response: {}", e))))?;
        
        serde_json::from_str::<T>(&response_text)
            .map_err(|e| Error::Database(format!("Failed to parse OpenAI response: {}", e)))
    }
    
    /// Create authenticated request builder
    fn create_request(&self, endpoint: &str) -> RequestBuilder {
        let url = format!("{}/{}", self.base_url, endpoint.trim_start_matches('/'));
        let mut builder = self.client.post(&url)
            .header("Content-Type", "application/json");
        
        if let Some(api_key) = &self.config.api_key {
            builder = builder.header("Authorization", format!("Bearer {}", api_key));
        }
        
        builder
    }
    
    /// Convert image bytes to data URL
    fn image_to_data_url(&self, image_data: &Bytes, format: &str) -> Result<String> {
        let mime_type = match format.to_lowercase().as_str() {
            "jpeg" | "jpg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => return Err(Error::Validation(format!("Unsupported image format: {}", format))),
        };
        
        use base64::Engine;
        let base64_data = base64::engine::general_purpose::STANDARD.encode(image_data);
        Ok(format!("data:{};base64,{}", mime_type, base64_data))
    }
    
    /// Parse event classification from AI response
    fn parse_classification(&self, response: &str) -> (EventClassification, f64, Vec<String>) {
        let lower_response = response.to_lowercase();
        
        // Extract classification
        let classification = if lower_response.contains("fire") || lower_response.contains("smoke") {
            EventClassification::Fire
        } else if lower_response.contains("person") || lower_response.contains("human") {
            EventClassification::Person
        } else if lower_response.contains("vehicle") || lower_response.contains("car") {
            EventClassification::Vehicle
        } else if lower_response.contains("animal") {
            EventClassification::Animal
        } else if lower_response.contains("motion") {
            EventClassification::Motion
        } else if lower_response.contains("suspicious") {
            EventClassification::Suspicious
        } else if lower_response.contains("normal") {
            EventClassification::Normal
        } else {
            EventClassification::Unknown
        };
        
        // Extract confidence (look for percentages or decimal values)
        let confidence = if let Some(pct_match) = response.match_indices('%').next() {
            // Look for number before %
            let before_pct = &response[..pct_match.0];
            if let Some(num_start) = before_pct.rfind(|c: char| !c.is_numeric() && c != '.') {
                before_pct[num_start + 1..].parse::<f64>().unwrap_or(0.75) / 100.0
            } else {
                0.75
            }
        } else if response.contains("confidence") {
            // Look for decimal after "confidence"
            0.8 // Default for confidence mentions
        } else {
            0.7 // Default confidence
        };
        
        // Generate suggested actions based on classification
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
            _ => vec!["Continue monitoring".to_string()],
        };
        
        (classification, confidence.clamp(0.0, 1.0), suggested_actions)
    }
}

#[async_trait]
impl AiClient for OpenAiClient {
    async fn summarize(&self, request: SummarizeRequest) -> Result<SummarizeResponse> {
        debug!("OpenAI client summarizing {} characters", request.text.len());
        
        let max_tokens = request.max_length.map(|len| (len as f32 * 1.3) as u32);
        let style_instruction = request.style.as_deref().unwrap_or("brief");
        
        let prompt = format!(
            "Summarize the following text in a {} style{}:\n\n{}",
            style_instruction,
            if let Some(max_len) = request.max_length {
                format!(", keeping it under {} characters", max_len)
            } else {
                String::new()
            },
            request.text
        );
        
        let openai_request = OpenAiSummarizeRequest {
            model: self.config.model.clone(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            max_tokens,
            temperature: 0.3,
        };
        
        let request_builder = self.create_request("/chat/completions")
            .json(&openai_request);
        
        let response: OpenAiResponse = self.execute_with_retry(request_builder).await?;
        
        let summary = response.choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .unwrap_or_else(|| "No summary generated".to_string());
        
        let confidence = if summary.len() < request.text.len() / 2 { 0.9 } else { 0.8 };
        
        Ok(SummarizeResponse {
            original_length: request.text.len(),
            summary_length: summary.len(),
            summary,
            confidence: Some(confidence),
        })
    }
    
    async fn describe_frame(&self, request: DescribeFrameRequest) -> Result<DescribeFrameResponse> {
        debug!("OpenAI client describing {} byte image", request.image_data.len());
        
        let data_url = self.image_to_data_url(&request.image_data, &request.image_format)?;
        let detail_level = request.detail_level.as_deref().unwrap_or("high");
        let focus = request.focus.as_deref().unwrap_or("objects");
        
        let prompt = format!(
            "Analyze this image with {} detail, focusing on {}. Provide a concise description and list any objects or people you can identify.",
            detail_level, focus
        );
        
        let openai_request = OpenAiVisionRequest {
            model: if self.config.model.starts_with("gpt-4") {
                "gpt-4-vision-preview".to_string()
            } else {
                "gpt-4-vision-preview".to_string() // Fallback to vision model
            },
            messages: vec![OpenAiVisionMessage {
                role: "user".to_string(),
                content: vec![
                    OpenAiContent::Text { text: prompt },
                    OpenAiContent::ImageUrl { 
                        image_url: OpenAiImageUrl { url: data_url }
                    },
                ],
            }],
            max_tokens: Some(500),
            temperature: 0.2,
        };
        
        let start_time = std::time::Instant::now();
        let request_builder = self.create_request("/chat/completions")
            .json(&openai_request);
        
        let response: OpenAiResponse = self.execute_with_retry(request_builder).await?;
        let processing_time = start_time.elapsed().as_millis() as u64;
        
        let description = response.choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .unwrap_or_else(|| "No description generated".to_string());
        
        // Extract objects from description (simple heuristic)
        let objects_detected: Vec<String> = description
            .split(&[',', ';', '.', '\n'][..])
            .filter_map(|s| {
                let trimmed = s.trim().to_lowercase();
                if trimmed.contains("person") || trimmed.contains("people") {
                    Some("person".to_string())
                } else if trimmed.contains("car") || trimmed.contains("vehicle") {
                    Some("vehicle".to_string())
                } else if trimmed.contains("building") {
                    Some("building".to_string())
                } else if trimmed.contains("tree") {
                    Some("tree".to_string())
                } else {
                    None
                }
            })
            .collect();
        
        Ok(DescribeFrameResponse {
            description,
            objects_detected,
            confidence: Some(0.85),
            processing_time_ms: Some(processing_time),
        })
    }
    
    async fn classify_event(&self, request: ClassifyEventRequest) -> Result<ClassifyEventResponse> {
        debug!("OpenAI client classifying event: {}", request.event_data.event_type);
        
        let context_str = request.context.as_deref().unwrap_or("security monitoring");
        let threshold = request.threshold.unwrap_or(0.7);
        
        let prompt = format!(
            "Classify this security event for {} context:\n\
            Event Type: {}\n\
            Confidence: {}\n\
            Metadata: {}\n\
            Timestamp: {}\n\
            Source: {}\n\n\
            Classify as one of: Motion, Person, Vehicle, Animal, Fire, Suspicious, Normal, Unknown\n\
            Provide confidence level and reasoning. Minimum confidence threshold is {}",
            context_str,
            request.event_data.event_type,
            request.event_data.confidence,
            request.event_data.metadata,
            request.event_data.timestamp,
            request.event_data.source_id,
            threshold
        );
        
        let openai_request = OpenAiSummarizeRequest {
            model: self.config.model.clone(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: prompt,
            }],
            max_tokens: Some(300),
            temperature: 0.1,
        };
        
        let request_builder = self.create_request("/chat/completions")
            .json(&openai_request);
        
        let response: OpenAiResponse = self.execute_with_retry(request_builder).await?;
        
        let response_text = response.choices
            .first()
            .map(|choice| choice.message.content.trim().to_string())
            .unwrap_or_else(|| "Unable to classify event".to_string());
        
        let (classification, confidence, suggested_actions) = self.parse_classification(&response_text);
        
        Ok(ClassifyEventResponse {
            classification,
            confidence,
            reasoning: response_text,
            suggested_actions,
        })
    }
    
    async fn health_check(&self) -> Result<()> {
        debug!("OpenAI client health check");
        
        if self.config.api_key.is_none() {
            return Err(Error::Config("OpenAI API key not configured".to_string()));
        }
        
        // Simple test request to verify connectivity
        let test_request = OpenAiSummarizeRequest {
            model: self.config.model.clone(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            max_tokens: Some(1),
            temperature: 0.0,
        };
        
        let request_builder = self.create_request("/chat/completions")
            .json(&test_request);
        
        let _: OpenAiResponse = self.execute_request(request_builder).await?;
        
        debug!("OpenAI client health check passed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    
    fn create_test_config() -> AiConfig {
        AiConfig {
            api_key: Some("test-key".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            timeout_seconds: 30,
            max_retries: 3,
            model: "gpt-3.5-turbo".to_string(),
            use_online: true,
        }
    }
    
    #[test]
    fn test_openai_client_creation() {
        let config = create_test_config();
        let client = OpenAiClient::new(config);
        
        assert_eq!(client.base_url, "https://api.openai.com/v1");
        assert_eq!(client.config.model, "gpt-3.5-turbo");
        assert_eq!(client.config.max_retries, 3);
    }
    
    #[test]
    fn test_image_to_data_url() {
        let config = create_test_config();
        let client = OpenAiClient::new(config);
        let image_data = Bytes::from("fake_jpeg_data");
        
        let result = client.image_to_data_url(&image_data, "jpeg");
        assert!(result.is_ok());
        
        let data_url = result.unwrap();
        assert!(data_url.starts_with("data:image/jpeg;base64,"));
    }
    
    #[test]
    fn test_parse_classification_fire() {
        let config = create_test_config();
        let client = OpenAiClient::new(config);
        
        let response = "This appears to be a fire event with 95% confidence. Immediate action required.";
        let (classification, confidence, actions) = client.parse_classification(response);
        
        assert_eq!(classification, EventClassification::Fire);
        assert!(confidence > 0.9);
        assert!(actions.contains(&"Immediately alert fire department".to_string()));
    }
    
    #[test]
    fn test_parse_classification_person() {
        let config = create_test_config();
        let client = OpenAiClient::new(config);
        
        let response = "A person is detected in the frame with high confidence level of 85%.";
        let (classification, confidence, _) = client.parse_classification(response);
        
        assert_eq!(classification, EventClassification::Person);
        assert!((confidence - 0.85).abs() < 0.01);
    }
    
    #[test]
    fn test_parse_classification_unknown() {
        let config = create_test_config();
        let client = OpenAiClient::new(config);
        
        let response = "Unable to determine the nature of this event clearly.";
        let (classification, _, _) = client.parse_classification(response);
        
        assert_eq!(classification, EventClassification::Unknown);
    }
    
    #[test]
    fn test_default_base_url() {
        let config = AiConfig {
            base_url: None,
            ..create_test_config()
        };
        
        let client = OpenAiClient::new(config);
        assert_eq!(client.base_url, "https://api.openai.com/v1");
    }
}