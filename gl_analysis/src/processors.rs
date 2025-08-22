//! ABOUTME: Individual processor implementations for motion, AI analysis, and summarization
//! ABOUTME: Implements the Processor trait for composable analysis pipeline components

use crate::{Processor, ProcessorInput, AnalysisEvent, EventSeverity};
use async_trait::async_trait;
use gl_ai::{AiClient, DescribeFrameRequest, SummarizeRequest, create_client, AiConfig};
use gl_core::Result;
use gl_vision::{MotionDetectionService, MotionConfig, MotionAlgorithm};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Motion detection processor
pub struct MotionProcessor {
    motion_service: MotionDetectionService,
    config: MotionProcessorConfig,
}

/// Configuration for motion processor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionProcessorConfig {
    /// Motion detection threshold
    pub threshold: f64,
    /// Minimum change area in pixels
    pub min_change_area: u32,
    /// Downscale factor for processing
    pub downscale_factor: u32,
    /// Motion detection algorithm
    pub algorithm: MotionAlgorithm,
}

impl Default for MotionProcessorConfig {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            min_change_area: 200,
            downscale_factor: 4,
            algorithm: MotionAlgorithm::PixelDiff,
        }
    }
}

impl MotionProcessor {
    pub fn new(config: Option<serde_json::Value>) -> Result<Self> {
        let config: MotionProcessorConfig = if let Some(config_value) = config {
            serde_json::from_value(config_value)
                .map_err(|e| gl_core::Error::Validation(format!("Invalid motion processor config: {}", e)))?
        } else {
            MotionProcessorConfig::default()
        };
        
        let motion_config = MotionConfig {
            algorithm: config.algorithm.clone(),
            threshold: config.threshold,
            downscale_factor: config.downscale_factor,
            max_width: 320,
            max_height: 240,
            min_change_area: config.min_change_area,
        };
        
        let motion_service = MotionDetectionService::new(motion_config)?;
        
        debug!("Created motion processor with threshold: {}", config.threshold);
        Ok(Self {
            motion_service,
            config,
        })
    }
}

#[async_trait]
impl Processor for MotionProcessor {
    async fn process(&mut self, input: ProcessorInput) -> Result<Vec<AnalysisEvent>> {
        let Some(frame_data) = &input.frame_data else {
            debug!("No frame data provided to motion processor");
            return Ok(Vec::new());
        };
        
        debug!("Processing frame for motion detection");
        let result = self.motion_service.detect_motion_from_bytes(frame_data)?;
        
        let mut events = Vec::new();
        
        if result.motion_detected {
            let event = AnalysisEvent::new(
                input.template_id.clone(),
                "motion_detected".to_string(),
                EventSeverity::Medium,
                result.confidence,
                format!("Motion detected with {:.1}% confidence. {} pixels changed out of {}.", 
                       result.confidence * 100.0, result.changed_pixels, result.total_pixels),
                self.name().to_string(),
                input.context.source_id.clone(),
            )
            .with_metadata("changed_pixels".to_string(), result.changed_pixels.into())
            .with_metadata("total_pixels".to_string(), result.total_pixels.into())
            .with_metadata("change_ratio".to_string(), result.change_ratio.into())
            .with_metadata("processing_time_ms".to_string(), result.processing_time_ms.into())
            .with_metadata("algorithm".to_string(), result.algorithm_used.into());
            
            events.push(event);
        }
        
        debug!("Motion processor generated {} events", events.len());
        Ok(events)
    }
    
    fn name(&self) -> &'static str {
        "motion"
    }
    
    async fn reset(&mut self) -> Result<()> {
        debug!("Resetting motion processor");
        self.motion_service.reset()?;
        Ok(())
    }
}

/// AI description processor
pub struct AiDescriptionProcessor {
    ai_client: Box<dyn AiClient>,
    config: AiDescriptionProcessorConfig,
}

/// Configuration for AI description processor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDescriptionProcessorConfig {
    /// Detail level for descriptions
    pub detail_level: String,
    /// Focus area for analysis
    pub focus: String,
    /// Whether to trigger on motion events only
    pub motion_only: bool,
}

impl Default for AiDescriptionProcessorConfig {
    fn default() -> Self {
        Self {
            detail_level: "high".to_string(),
            focus: "objects".to_string(),
            motion_only: true,
        }
    }
}

impl AiDescriptionProcessor {
    pub fn new(config: Option<serde_json::Value>) -> Result<Self> {
        let config: AiDescriptionProcessorConfig = if let Some(config_value) = config {
            serde_json::from_value(config_value)
                .map_err(|e| gl_core::Error::Validation(format!("Invalid AI description processor config: {}", e)))?
        } else {
            AiDescriptionProcessorConfig::default()
        };
        
        // Create AI client with default configuration (stub by default)
        let ai_config = AiConfig::default();
        let ai_client = create_client(ai_config);
        
        debug!("Created AI description processor with focus: {}", config.focus);
        Ok(Self { ai_client, config })
    }
}

#[async_trait]
impl Processor for AiDescriptionProcessor {
    async fn process(&mut self, input: ProcessorInput) -> Result<Vec<AnalysisEvent>> {
        // Check if we should only process on motion events
        if self.config.motion_only {
            let has_motion = input.context.previous_events.iter()
                .any(|event| event.event_type == "motion_detected");
            
            if !has_motion {
                debug!("No motion event found, skipping AI description");
                return Ok(Vec::new());
            }
        }
        
        let Some(frame_data) = &input.frame_data else {
            debug!("No frame data provided to AI description processor");
            return Ok(Vec::new());
        };
        
        let Some(frame_format) = &input.frame_format else {
            debug!("No frame format provided to AI description processor");
            return Ok(Vec::new());
        };
        
        debug!("Processing frame for AI description");
        
        let request = DescribeFrameRequest {
            image_data: frame_data.clone(),
            image_format: frame_format.clone(),
            detail_level: Some(self.config.detail_level.clone()),
            focus: Some(self.config.focus.clone()),
        };
        
        match self.ai_client.describe_frame(request).await {
            Ok(response) => {
                let mut events = Vec::new();
                
                // Create description event
                let description_event = AnalysisEvent::new(
                    input.template_id.clone(),
                    "frame_described".to_string(),
                    EventSeverity::Low,
                    response.confidence.unwrap_or(0.8),
                    response.description.clone(),
                    self.name().to_string(),
                    input.context.source_id.clone(),
                )
                .with_metadata("description".to_string(), response.description.clone().into())
                .with_metadata("objects_detected".to_string(), 
                             serde_json::to_value(response.objects_detected.clone()).unwrap())
                .with_metadata("processing_time_ms".to_string(), 
                             response.processing_time_ms.unwrap_or(0).into());
                
                events.push(description_event);
                
                // Create specific events for detected objects
                for object in response.objects_detected {
                    if ["person", "car", "fire", "animal"].contains(&object.as_str()) {
                        let severity = match object.as_str() {
                            "fire" => EventSeverity::Critical,
                            "person" => EventSeverity::High,
                            "car" => EventSeverity::Medium,
                            _ => EventSeverity::Low,
                        };
                        
                        let object_event = AnalysisEvent::new(
                            input.template_id.clone(),
                            format!("{}_detected", object),
                            severity,
                            response.confidence.unwrap_or(0.8),
                            format!("{} detected in frame: {}", 
                                   object.to_uppercase(), response.description),
                            self.name().to_string(),
                            input.context.source_id.clone(),
                        )
                        .with_metadata("object_type".to_string(), object.into())
                        .with_metadata("description_context".to_string(), response.description.clone().into());
                        
                        events.push(object_event);
                    }
                }
                
                debug!("AI description processor generated {} events", events.len());
                Ok(events)
            },
            Err(e) => {
                warn!("AI description failed: {}", e);
                Ok(Vec::new())
            }
        }
    }
    
    fn name(&self) -> &'static str {
        "ai_description"
    }
}

/// Summary processor that consolidates events
pub struct SummaryProcessor {
    ai_client: Box<dyn AiClient>,
    config: SummaryProcessorConfig,
}

/// Configuration for summary processor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryProcessorConfig {
    /// Maximum length for summaries
    pub max_length: usize,
    /// Summary style
    pub style: String,
    /// Minimum events to trigger summary
    pub min_events: usize,
}

impl Default for SummaryProcessorConfig {
    fn default() -> Self {
        Self {
            max_length: 200,
            style: "brief".to_string(),
            min_events: 2,
        }
    }
}

impl SummaryProcessor {
    pub fn new(config: Option<serde_json::Value>) -> Result<Self> {
        let config: SummaryProcessorConfig = if let Some(config_value) = config {
            serde_json::from_value(config_value)
                .map_err(|e| gl_core::Error::Validation(format!("Invalid summary processor config: {}", e)))?
        } else {
            SummaryProcessorConfig::default()
        };
        
        // Create AI client with default configuration (stub by default)
        let ai_config = AiConfig::default();
        let ai_client = create_client(ai_config);
        
        debug!("Created summary processor with max length: {}", config.max_length);
        Ok(Self { ai_client, config })
    }
}

#[async_trait]
impl Processor for SummaryProcessor {
    async fn process(&mut self, input: ProcessorInput) -> Result<Vec<AnalysisEvent>> {
        if input.context.previous_events.len() < self.config.min_events {
            debug!("Not enough events ({}) to generate summary, minimum is {}", 
                   input.context.previous_events.len(), self.config.min_events);
            return Ok(Vec::new());
        }
        
        // Collect all event descriptions
        let event_descriptions: Vec<String> = input.context.previous_events
            .iter()
            .map(|event| format!("{}: {}", event.event_type, event.description))
            .collect();
        
        let combined_text = event_descriptions.join(". ");
        
        debug!("Generating summary for {} events", input.context.previous_events.len());
        
        let request = SummarizeRequest {
            text: combined_text,
            max_length: Some(self.config.max_length),
            style: Some(self.config.style.clone()),
        };
        
        match self.ai_client.summarize(request).await {
            Ok(response) => {
                let summary_event = AnalysisEvent::new(
                    input.template_id.clone(),
                    "activity_summary".to_string(),
                    EventSeverity::Info,
                    response.confidence.unwrap_or(0.8),
                    response.summary.clone(),
                    self.name().to_string(),
                    input.context.source_id.clone(),
                )
                .with_metadata("summary".to_string(), response.summary.into())
                .with_metadata("original_length".to_string(), response.original_length.into())
                .with_metadata("summary_length".to_string(), response.summary_length.into())
                .with_metadata("events_summarized".to_string(), input.context.previous_events.len().into())
                .with_notification(false); // Summaries typically don't need notifications
                
                debug!("Summary processor generated 1 event");
                Ok(vec![summary_event])
            },
            Err(e) => {
                warn!("Summary generation failed: {}", e);
                Ok(Vec::new())
            }
        }
    }
    
    fn name(&self) -> &'static str {
        "summary"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessorContext, AnalysisEvent, EventSeverity};
    use bytes::Bytes;
    use chrono::Utc;
    
    #[tokio::test]
    async fn test_motion_processor_creation() {
        let config = serde_json::json!({
            "threshold": 0.15,
            "min_change_area": 300,
            "downscale_factor": 4,
            "algorithm": "PixelDiff"
        });
        
        let processor = MotionProcessor::new(Some(config));
        assert!(processor.is_ok());
        
        let processor = processor.unwrap();
        assert_eq!(processor.name(), "motion");
        assert_eq!(processor.config.threshold, 0.15);
        assert_eq!(processor.config.min_change_area, 300);
    }
    
    #[tokio::test]
    async fn test_ai_description_processor_creation() {
        let config = serde_json::json!({
            "detail_level": "low",
            "focus": "activity",
            "motion_only": false
        });
        
        let processor = AiDescriptionProcessor::new(Some(config));
        assert!(processor.is_ok());
        
        let processor = processor.unwrap();
        assert_eq!(processor.name(), "ai_description");
        assert_eq!(processor.config.detail_level, "low");
        assert_eq!(processor.config.focus, "activity");
        assert!(!processor.config.motion_only);
    }
    
    #[tokio::test]
    async fn test_summary_processor_creation() {
        let config = serde_json::json!({
            "max_length": 150,
            "style": "detailed",
            "min_events": 3
        });
        
        let processor = SummaryProcessor::new(Some(config));
        assert!(processor.is_ok());
        
        let processor = processor.unwrap();
        assert_eq!(processor.name(), "summary");
        assert_eq!(processor.config.max_length, 150);
        assert_eq!(processor.config.style, "detailed");
        assert_eq!(processor.config.min_events, 3);
    }
    
    #[tokio::test]
    async fn test_motion_processor_no_frame_data() {
        let mut processor = MotionProcessor::new(None).unwrap();
        
        let input = ProcessorInput {
            template_id: "test".to_string(),
            frame_data: None,
            frame_format: None,
            text_content: None,
            context: ProcessorContext::new("test_source".to_string()),
            timestamp: Utc::now(),
        };
        
        let events = processor.process(input).await.unwrap();
        assert_eq!(events.len(), 0);
    }
    
    #[tokio::test]
    async fn test_summary_processor_insufficient_events() {
        let mut processor = SummaryProcessor::new(None).unwrap();
        
        let mut context = ProcessorContext::new("test_source".to_string());
        context.previous_events.push(AnalysisEvent::new(
            "test".to_string(),
            "motion_detected".to_string(),
            EventSeverity::Medium,
            0.8,
            "Motion detected".to_string(),
            "motion".to_string(),
            "test_source".to_string(),
        ));
        
        let input = ProcessorInput {
            template_id: "test".to_string(),
            frame_data: None,
            frame_format: None,
            text_content: None,
            context,
            timestamp: Utc::now(),
        };
        
        let events = processor.process(input).await.unwrap();
        assert_eq!(events.len(), 0); // Not enough events for summary
    }
}