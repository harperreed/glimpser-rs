//! ABOUTME: AI analysis endpoints for Axum server with JSON APIs
//! ABOUTME: Provides AI-powered analysis services with proper authentication and error handling

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::{IntoResponse, Json as ResponseJson},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use gl_ai::{ClassifyEventRequest, DescribeFrameRequest, EventData, SummarizeRequest};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::frontend::FrontendState;

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

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(error: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
        }
    }
}

/// Summarize text using AI
pub async fn summarize(
    State(frontend_state): State<FrontendState>,
    Json(request): Json<SummarizeApiRequest>,
) -> impl IntoResponse {
    info!(
        text_length = request.text.len(),
        style = ?request.style,
        "Processing text summarization request"
    );

    if request.text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            ResponseJson(ErrorResponse::new(
                "validation_error",
                "Text cannot be empty",
            )),
        )
            .into_response();
    }

    if request.text.len() > 50000 {
        return (
            StatusCode::BAD_REQUEST,
            ResponseJson(ErrorResponse::new(
                "validation_error",
                "Text too long (max 50,000 characters)",
            )),
        )
            .into_response();
    }

    let ai_request = SummarizeRequest {
        text: request.text.clone(),
        max_length: request.max_length,
        style: request.style.clone(),
    };

    match frontend_state
        .app_state
        .ai_client
        .summarize(ai_request)
        .await
    {
        Ok(response) => {
            info!(
                original_length = response.original_length,
                summary_length = response.summary_length,
                "Text summarization completed successfully"
            );

            (
                StatusCode::OK,
                ResponseJson(ApiResponse::success(SummarizeApiResponse {
                    summary: response.summary,
                    original_length: response.original_length,
                    summary_length: response.summary_length,
                    confidence: response.confidence,
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("Text summarization failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(ErrorResponse::new(
                    "service_error",
                    "AI summarization service unavailable",
                )),
            )
                .into_response()
        }
    }
}

/// Describe image content using AI vision
pub async fn describe_frame(
    State(frontend_state): State<FrontendState>,
    Json(request): Json<DescribeFrameApiRequest>,
) -> impl IntoResponse {
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
            return (
                StatusCode::BAD_REQUEST,
                ResponseJson(ErrorResponse::new(
                    "validation_error",
                    "Invalid base64 image data",
                )),
            )
                .into_response();
        }
    };

    if image_data.len() > 10_000_000 {
        // 10MB limit
        return (
            StatusCode::BAD_REQUEST,
            ResponseJson(ErrorResponse::new(
                "validation_error",
                "Image too large (max 10MB)",
            )),
        )
            .into_response();
    }

    let ai_request = DescribeFrameRequest {
        image_data,
        image_format: request.image_format.clone(),
        detail_level: request.detail_level.clone(),
        focus: request.focus.clone(),
    };

    match frontend_state
        .app_state
        .ai_client
        .describe_frame(ai_request)
        .await
    {
        Ok(response) => {
            info!(
                objects_count = response.objects_detected.len(),
                processing_time_ms = ?response.processing_time_ms,
                "Image description completed successfully"
            );

            (
                StatusCode::OK,
                ResponseJson(ApiResponse::success(DescribeFrameApiResponse {
                    description: response.description,
                    objects_detected: response.objects_detected,
                    confidence: response.confidence,
                    processing_time_ms: response.processing_time_ms,
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("Image description failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(ErrorResponse::new(
                    "service_error",
                    "AI vision service unavailable",
                )),
            )
                .into_response()
        }
    }
}

/// Classify security event using AI
pub async fn classify_event(
    State(frontend_state): State<FrontendState>,
    Json(request): Json<ClassifyEventApiRequest>,
) -> impl IntoResponse {
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

    match frontend_state
        .app_state
        .ai_client
        .classify_event(ai_request)
        .await
    {
        Ok(response) => {
            info!(
                classification = ?response.classification,
                confidence = response.confidence,
                actions_count = response.suggested_actions.len(),
                "Event classification completed successfully"
            );

            (
                StatusCode::OK,
                ResponseJson(ApiResponse::success(ClassifyEventApiResponse {
                    classification: format!("{:?}", response.classification),
                    confidence: response.confidence,
                    reasoning: response.reasoning,
                    suggested_actions: response.suggested_actions,
                })),
            )
                .into_response()
        }
        Err(e) => {
            error!("Event classification failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(ErrorResponse::new(
                    "service_error",
                    "AI classification service unavailable",
                )),
            )
                .into_response()
        }
    }
}

/// Check AI service health
pub async fn health_check(State(frontend_state): State<FrontendState>) -> impl IntoResponse {
    debug!("Checking AI service health");

    match frontend_state.app_state.ai_client.health_check().await {
        Ok(()) => {
            info!("AI service health check passed");
            (
                StatusCode::OK,
                ResponseJson(ApiResponse::success(serde_json::json!({
                    "status": "healthy",
                    "service": "ai"
                }))),
            )
                .into_response()
        }
        Err(e) => {
            error!("AI service health check failed: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                ResponseJson(ErrorResponse::new(
                    "health_check_failed",
                    format!("AI service unhealthy: {}", e),
                )),
            )
                .into_response()
        }
    }
}

/// Configure AI routes for Axum router
pub fn ai_routes() -> Router<FrontendState> {
    Router::new()
        .route("/api/ai/summarize", post(summarize))
        .route("/api/ai/describe", post(describe_frame))
        .route("/api/ai/classify", post(classify_event))
        .route("/api/ai/health", get(health_check))
}
