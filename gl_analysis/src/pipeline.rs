//! ABOUTME: Analysis pipeline that orchestrates processor execution in sequence
//! ABOUTME: Chains together motion detection, AI analysis, and summarization processors

use crate::{Processor, ProcessorInput, AnalysisEvent, processors::*};
use gl_core::Result;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Analysis pipeline that chains processors together
pub struct AnalysisPipeline {
    processors: Vec<Box<dyn Processor>>,
}

impl AnalysisPipeline {
    /// Create a new analysis pipeline with specified processors
    pub fn new(
        processor_names: Vec<String>,
        processor_configs: HashMap<String, serde_json::Value>,
    ) -> Result<Self> {
        let mut processors: Vec<Box<dyn Processor>> = Vec::new();
        
        for name in processor_names {
            let config = processor_configs.get(&name);
            
            let processor: Box<dyn Processor> = match name.as_str() {
                "motion" => {
                    debug!("Creating motion processor");
                    Box::new(MotionProcessor::new(config.cloned())?)
                },
                "ai_description" => {
                    debug!("Creating AI description processor");
                    Box::new(AiDescriptionProcessor::new(config.cloned())?)
                },
                "summary" => {
                    debug!("Creating summary processor");
                    Box::new(SummaryProcessor::new(config.cloned())?)
                },
                _ => {
                    warn!("Unknown processor type: {}, skipping", name);
                    continue;
                }
            };
            
            processors.push(processor);
        }
        
        debug!("Created pipeline with {} processors", processors.len());
        Ok(Self { processors })
    }
    
    /// Process input through all processors in sequence
    pub async fn process(&mut self, mut input: ProcessorInput) -> Result<Vec<AnalysisEvent>> {
        let mut all_events = Vec::new();
        
        for processor in &mut self.processors {
            debug!("Running processor: {}", processor.name());
            
            match processor.process(input.clone()).await {
                Ok(events) => {
                    debug!("Processor {} generated {} events", processor.name(), events.len());
                    
                    // Add events to context for subsequent processors
                    input.context.previous_events.extend(events.clone());
                    all_events.extend(events);
                },
                Err(e) => {
                    warn!("Processor {} failed: {}", processor.name(), e);
                    // Continue with other processors rather than failing the entire pipeline
                }
            }
        }
        
        debug!("Pipeline completed with {} total events", all_events.len());
        Ok(all_events)
    }
    
    /// Reset all processors in the pipeline
    pub async fn reset(&mut self) -> Result<()> {
        debug!("Resetting all processors in pipeline");
        
        for processor in &mut self.processors {
            if let Err(e) = processor.reset().await {
                warn!("Failed to reset processor {}: {}", processor.name(), e);
            }
        }
        
        Ok(())
    }
    
    /// Get processor names in the pipeline
    pub fn processor_names(&self) -> Vec<&str> {
        self.processors.iter().map(|p| p.name()).collect()
    }
    
    /// Get number of processors
    pub fn processor_count(&self) -> usize {
        self.processors.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessorContext, EventSeverity};
    use bytes::Bytes;
    use chrono::Utc;
    use std::collections::HashMap;
    
    #[tokio::test]
    async fn test_pipeline_creation() {
        let processor_names = vec![
            "motion".to_string(),
            "ai_description".to_string(),
        ];
        let processor_configs = HashMap::new();
        
        let pipeline = AnalysisPipeline::new(processor_names, processor_configs);
        assert!(pipeline.is_ok());
        
        let pipeline = pipeline.unwrap();
        assert_eq!(pipeline.processor_count(), 2);
        
        let names = pipeline.processor_names();
        assert!(names.contains(&"motion"));
        assert!(names.contains(&"ai_description"));
    }
    
    #[tokio::test]
    async fn test_pipeline_with_unknown_processor() {
        let processor_names = vec![
            "motion".to_string(),
            "unknown_processor".to_string(),
        ];
        let processor_configs = HashMap::new();
        
        let pipeline = AnalysisPipeline::new(processor_names, processor_configs);
        assert!(pipeline.is_ok());
        
        let pipeline = pipeline.unwrap();
        // Should skip unknown processor
        assert_eq!(pipeline.processor_count(), 1);
        assert_eq!(pipeline.processor_names()[0], "motion");
    }
    
    #[tokio::test]
    async fn test_empty_pipeline() {
        let processor_names = Vec::new();
        let processor_configs = HashMap::new();
        
        let mut pipeline = AnalysisPipeline::new(processor_names, processor_configs).unwrap();
        assert_eq!(pipeline.processor_count(), 0);
        
        let input = ProcessorInput {
            template_id: "test".to_string(),
            frame_data: Some(Bytes::from("test")),
            frame_format: Some("jpeg".to_string()),
            text_content: None,
            context: ProcessorContext::new("test_source".to_string()),
            timestamp: Utc::now(),
        };
        
        let events = pipeline.process(input).await.unwrap();
        assert_eq!(events.len(), 0);
    }
    
    #[tokio::test]
    async fn test_pipeline_reset() {
        let processor_names = vec!["motion".to_string()];
        let processor_configs = HashMap::new();
        
        let mut pipeline = AnalysisPipeline::new(processor_names, processor_configs).unwrap();
        let result = pipeline.reset().await;
        assert!(result.is_ok());
    }
}