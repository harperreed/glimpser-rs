//! ABOUTME: AI analysis endpoints for content summarization and image classification
//! ABOUTME: Provides AI-powered analysis services with proper authentication and error handling

use actix_web::{web, HttpResponse, Result as ActixResult};
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use gl_ai::{ClassifyEventRequest, DescribeFrameRequest, EventData, SummarizeRequest};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::{
    models::{ApiResponse, ErrorResponse},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct SummarizeApiRequest {
    pub text: String,
    pub max_length: Option<usize>,
    pub style: Option<String>, // "brief", "detailed", "technical"
}

#[derive(Debug, Serialize)]
pub struct SummarizeApiResponse {
    pub summary: String,
    pub original_length: usize,
    pub summary_length: usize,
    pub confidence: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct DescribeFrameApiRequest {
    pub image_base64: String,
    pub image_format: String,         // "jpeg", "png"
    pub detail_level: Option<String>, // "low", "high", "auto"
    pub focus: Option<String>,        // "objects", "activity", "scene"
}

#[derive(Debug, Serialize)]
pub struct DescribeFrameApiResponse {
    pub description: String,
    pub objects_detected: Vec<String>,
    pub confidence: Option<f64>,
    pub processing_time_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ClassifyEventApiRequest {
    pub event_type: String,
    pub confidence: f64,
    pub metadata: serde_json::Value,
    pub timestamp: String,
    pub source_id: String,
    pub context: Option<String>,
    pub threshold: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ClassifyEventApiResponse {
    pub classification: String,
    pub confidence: f64,
    pub reasoning: String,
    pub suggested_actions: Vec<String>,
}

/// Summarize text using AI
pub async fn summarize(
    state: web::Data<AppState>,
    request: web::Json<SummarizeApiRequest>,
) -> ActixResult<HttpResponse> {
    info!(
        text_length = request.text.len(),
        style = ?request.style,
        "Processing text summarization request"
    );

    if request.text.trim().is_empty() {
        return Ok(HttpResponse::BadRequest().json(ErrorResponse::new(
            "validation_error",
            "Text cannot be empty",
        )));
    }

    if request.text.len() > 50000 {
        return Ok(HttpResponse::BadRequest().json(ErrorResponse::new(
            "validation_error",
            "Text too long (max 50,000 characters)",
        )));
    }

    let ai_request = SummarizeRequest {
        text: request.text.clone(),
        max_length: request.max_length,
        style: request.style.clone(),
    };

    match state.ai_client.summarize(ai_request).await {
        Ok(response) => {
            info!(
                original_length = response.original_length,
                summary_length = response.summary_length,
                "Text summarization completed successfully"
            );

            Ok(
                HttpResponse::Ok().json(ApiResponse::success(SummarizeApiResponse {
                    summary: response.summary,
                    original_length: response.original_length,
                    summary_length: response.summary_length,
                    confidence: response.confidence,
                })),
            )
        }
        Err(e) => {
            error!("Text summarization failed: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "service_error",
                "AI summarization service unavailable",
            )))
        }
    }
}

/// Describe image content using AI vision
pub async fn describe_frame(
    state: web::Data<AppState>,
    request: web::Json<DescribeFrameApiRequest>,
) -> ActixResult<HttpResponse> {
    info!(
        image_format = %request.image_format,
        detail_level = ?request.detail_level,
        focus = ?request.focus,
        "Processing image description request"
    );

    // Decode base64 image
    let image_data = match general_purpose::STANDARD.decode(&request.image_base64) {
        Ok(data) => Bytes::from(data),
        Err(_) => {
            return Ok(HttpResponse::BadRequest().json(ErrorResponse::new(
                "validation_error",
                "Invalid base64 image data",
            )));
        }
    };

    if image_data.len() > 10_000_000 {
        // 10MB limit
        return Ok(HttpResponse::BadRequest().json(ErrorResponse::new(
            "validation_error",
            "Image too large (max 10MB)",
        )));
    }

    let ai_request = DescribeFrameRequest {
        image_data,
        image_format: request.image_format.clone(),
        detail_level: request.detail_level.clone(),
        focus: request.focus.clone(),
    };

    match state.ai_client.describe_frame(ai_request).await {
        Ok(response) => {
            info!(
                objects_count = response.objects_detected.len(),
                processing_time_ms = ?response.processing_time_ms,
                "Image description completed successfully"
            );

            Ok(
                HttpResponse::Ok().json(ApiResponse::success(DescribeFrameApiResponse {
                    description: response.description,
                    objects_detected: response.objects_detected,
                    confidence: response.confidence,
                    processing_time_ms: response.processing_time_ms,
                })),
            )
        }
        Err(e) => {
            error!("Image description failed: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "service_error",
                "AI vision service unavailable",
            )))
        }
    }
}

/// Classify security event using AI
pub async fn classify_event(
    state: web::Data<AppState>,
    request: web::Json<ClassifyEventApiRequest>,
) -> ActixResult<HttpResponse> {
    info!(
        event_type = %request.event_type,
        confidence = request.confidence,
        source_id = %request.source_id,
        "Processing event classification request"
    );

    let event_data = EventData {
        event_type: request.event_type.clone(),
        confidence: request.confidence,
        metadata: request.metadata.clone(),
        timestamp: request.timestamp.clone(),
        source_id: request.source_id.clone(),
    };

    let ai_request = ClassifyEventRequest {
        event_data,
        context: request.context.clone(),
        threshold: request.threshold,
    };

    match state.ai_client.classify_event(ai_request).await {
        Ok(response) => {
            info!(
                classification = ?response.classification,
                confidence = response.confidence,
                actions_count = response.suggested_actions.len(),
                "Event classification completed successfully"
            );

            Ok(
                HttpResponse::Ok().json(ApiResponse::success(ClassifyEventApiResponse {
                    classification: format!("{:?}", response.classification),
                    confidence: response.confidence,
                    reasoning: response.reasoning,
                    suggested_actions: response.suggested_actions,
                })),
            )
        }
        Err(e) => {
            error!("Event classification failed: {}", e);
            Ok(HttpResponse::InternalServerError().json(ErrorResponse::new(
                "service_error",
                "AI classification service unavailable",
            )))
        }
    }
}

/// Check AI service health
pub async fn health_check(state: web::Data<AppState>) -> ActixResult<HttpResponse> {
    debug!("Checking AI service health");

    match state.ai_client.health_check().await {
        Ok(()) => {
            info!("AI service health check passed");
            Ok(
                HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
                    "status": "healthy",
                    "service": "ai"
                }))),
            )
        }
        Err(e) => {
            error!("AI service health check failed: {}", e);
            Ok(HttpResponse::ServiceUnavailable().json(ErrorResponse::new(
                "health_check_failed",
                format!("AI service unhealthy: {}", e),
            )))
        }
    }
}

/// Configure AI routes
pub fn configure_ai_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/ai")
            .route("/summarize", web::post().to(summarize))
            .route("/describe", web::post().to(describe_frame))
            .route("/classify", web::post().to(classify_event))
            .route("/health", web::get().to(health_check)),
    );
}

// TODO: Add comprehensive tests for AI endpoints
// Tests require proper mocking of AppState and AI client dependencies
