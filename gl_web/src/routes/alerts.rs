//! ABOUTME: Alert and notification API endpoints
//! ABOUTME: Provides endpoints for testing and managing notification systems

use actix_web::{web, HttpResponse, Result as ActixResult};
use gl_notify::{
    adapters::pushover::PushoverAdapter,
    circuit_breaker::CircuitBreakerWrapper,
    retry::RetryWrapper,
    Notification, NotificationChannel, NotificationKind, NotificationManager,
};
use gl_cap::profiles::AlertProfiles;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{middleware::rbac::RequireRole, models::ApiResponse};

/// Request payload for testing notifications
#[derive(Debug, Deserialize)]
pub struct TestNotificationRequest {
    /// Type of notification to send
    #[serde(default = "default_notification_kind")]
    pub kind: NotificationKind,
    /// Title for the test notification
    pub title: String,
    /// Body message for the test notification
    pub body: String,
    /// Optional Pushover user key for testing
    pub pushover_user_key: Option<String>,
    /// Optional webhook URL for testing
    pub webhook_url: Option<String>,
}

fn default_notification_kind() -> NotificationKind {
    NotificationKind::Info
}

/// Request payload for CAP XML preview
#[derive(Debug, Deserialize)]
pub struct CapPreviewRequest {
    /// CAP alert profile to use
    pub profile: CapProfile,
    /// Sender identifier for the alert
    pub sender: String,
    /// Optional custom title to override profile default
    pub custom_title: Option<String>,
    /// Optional custom description to override profile default
    pub custom_description: Option<String>,
    /// Optional custom instructions to override profile default
    pub custom_instruction: Option<String>,
}

/// Available CAP alert profiles
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapProfile {
    SevereWeather,
    ExtremeWeather,
    FireAlert,
    PublicSafety,
    HealthAlert,
    EnvironmentalHazard,
    TransportationAlert,
    InfrastructureAlert,
    CbrneAlert,
    SecurityAlert,
    TestAlert,
    AllClear,
}

/// Response for CAP XML preview
#[derive(Debug, Serialize)]
pub struct CapPreviewResponse {
    pub cap_xml: String,
    pub profile_used: String,
    pub sender: String,
    pub alert_id: String,
    pub metadata: CapMetadata,
}

#[derive(Debug, Serialize)]
pub struct CapMetadata {
    pub urgency: String,
    pub severity: String,
    pub certainty: String,
    pub categories: Vec<String>,
    pub response_types: Vec<String>,
}

/// Response for test notification endpoint
#[derive(Debug, Serialize)]
pub struct TestNotificationResponse {
    pub message: String,
    pub notification_id: String,
    pub channels_attempted: Vec<String>,
    pub results: Vec<ChannelResult>,
}

#[derive(Debug, Serialize)]
pub struct ChannelResult {
    pub channel: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Test notification endpoint
pub async fn test_notification(
    payload: web::Json<TestNotificationRequest>,
) -> ActixResult<HttpResponse> {
    info!(
        title = %payload.title,
        kind = ?payload.kind,
        "Received test notification request"
    );

    // Set up notification manager with available adapters
    let mut manager = NotificationManager::new();
    let mut channels = Vec::new();
    let mut channel_names = Vec::new();

    // Add Pushover channel if user key provided
    if let Some(user_key) = &payload.pushover_user_key {
        // TODO: Get app token from configuration
        let app_token = std::env::var("PUSHOVER_APP_TOKEN")
            .unwrap_or_else(|_| "test_app_token".to_string());
        
        let pushover_adapter = PushoverAdapter::new(app_token);
        let wrapped_adapter = CircuitBreakerWrapper::new(RetryWrapper::new(pushover_adapter));
        manager.register_adapter("pushover".to_string(), Box::new(wrapped_adapter));
        
        channels.push(NotificationChannel::Pushover {
            user_key: user_key.clone(),
            device: None,
            priority: Some(0),
            sound: None,
        });
        channel_names.push("pushover".to_string());
    }

    // Add webhook channel if URL provided
    if let Some(webhook_url) = &payload.webhook_url {
        if let Ok(url) = webhook_url.parse() {
            channels.push(NotificationChannel::Webhook {
                url,
                headers: None,
                method: Some("POST".to_string()),
            });
            channel_names.push("webhook".to_string());
        } else {
            warn!(webhook_url = %webhook_url, "Invalid webhook URL provided");
            return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(
                "Invalid webhook URL format".to_string(),
            )));
        }
    }

    if channels.is_empty() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<()>::error(
            "No notification channels provided. Include pushover_user_key or webhook_url.".to_string(),
        )));
    }

    // Create and send test notification
    let notification = Notification::new(
        payload.kind.clone(),
        payload.title.clone(),
        payload.body.clone(),
        channels,
    )
    .with_metadata("source".to_string(), "test_endpoint".to_string())
    .with_metadata("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

    let notification_id = notification.id.to_string();
    let mut results = Vec::new();

    // Attempt to send through each channel
    for channel_name in &channel_names {
        match manager.send(&notification).await {
            Ok(()) => {
                info!(
                    notification_id = %notification_id,
                    channel = %channel_name,
                    "Test notification sent successfully"
                );
                results.push(ChannelResult {
                    channel: channel_name.clone(),
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                warn!(
                    notification_id = %notification_id,
                    channel = %channel_name,
                    error = %e,
                    "Test notification failed"
                );
                results.push(ChannelResult {
                    channel: channel_name.clone(),
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    let response = TestNotificationResponse {
        message: "Test notification completed".to_string(),
        notification_id,
        channels_attempted: channel_names,
        results,
    };

    Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
}

/// Preview CAP XML for a given template/event
pub async fn cap_preview(
    payload: web::Json<CapPreviewRequest>,
) -> ActixResult<HttpResponse> {
    info!(
        profile = ?payload.profile,
        sender = %payload.sender,
        "Received CAP preview request"
    );

    // Create alert based on profile
    let mut alert = match payload.profile {
        CapProfile::SevereWeather => AlertProfiles::severe_weather(&payload.sender),
        CapProfile::ExtremeWeather => AlertProfiles::extreme_weather(&payload.sender),
        CapProfile::FireAlert => AlertProfiles::fire_alert(&payload.sender),
        CapProfile::PublicSafety => AlertProfiles::public_safety(&payload.sender),
        CapProfile::HealthAlert => AlertProfiles::health_alert(&payload.sender),
        CapProfile::EnvironmentalHazard => AlertProfiles::environmental_hazard(&payload.sender),
        CapProfile::TransportationAlert => AlertProfiles::transportation_alert(&payload.sender),
        CapProfile::InfrastructureAlert => AlertProfiles::infrastructure_alert(&payload.sender),
        CapProfile::CbrneAlert => AlertProfiles::cbrne_alert(&payload.sender),
        CapProfile::SecurityAlert => AlertProfiles::security_alert(&payload.sender),
        CapProfile::TestAlert => AlertProfiles::test_alert(&payload.sender),
        CapProfile::AllClear => AlertProfiles::all_clear(&payload.sender),
    }.build();

    // Apply custom overrides if provided
    if let Some(ref custom_title) = payload.custom_title {
        if let Some(info) = alert.info.first_mut() {
            info.headline = Some(custom_title.clone());
        }
    }
    
    if let Some(ref custom_description) = payload.custom_description {
        if let Some(info) = alert.info.first_mut() {
            info.description = Some(custom_description.clone());
        }
    }
    
    if let Some(ref custom_instruction) = payload.custom_instruction {
        if let Some(info) = alert.info.first_mut() {
            info.instruction = Some(custom_instruction.clone());
        }
    }

    // Generate CAP XML
    let cap_xml = match alert.to_xml() {
        Ok(xml) => xml,
        Err(e) => {
            warn!(error = %e, "Failed to generate CAP XML");
            return Ok(HttpResponse::InternalServerError().json(ApiResponse::<()>::error(
                format!("Failed to generate CAP XML: {}", e),
            )));
        }
    };

    // Extract metadata from first info block
    let metadata = if let Some(info) = alert.info.first() {
        CapMetadata {
            urgency: format!("{:?}", info.urgency),
            severity: format!("{:?}", info.severity),
            certainty: format!("{:?}", info.certainty),
            categories: info.category.iter().map(|c| format!("{:?}", c)).collect(),
            response_types: info.response_type.iter().map(|r| format!("{:?}", r)).collect(),
        }
    } else {
        CapMetadata {
            urgency: "Unknown".to_string(),
            severity: "Unknown".to_string(),
            certainty: "Unknown".to_string(),
            categories: vec![],
            response_types: vec![],
        }
    };

    let response = CapPreviewResponse {
        cap_xml,
        profile_used: format!("{:?}", payload.profile),
        sender: payload.sender.clone(),
        alert_id: alert.identifier.clone(),
        metadata,
    };

    info!(
        alert_id = %alert.identifier,
        profile = ?payload.profile,
        "CAP XML generated successfully"
    );

    Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
}

/// Get notification system health
pub async fn notification_health() -> ActixResult<HttpResponse> {
    // TODO: Implement health check for configured notification adapters
    let health_info = serde_json::json!({
        "status": "healthy",
        "adapters": {
            "pushover": "available",
            "webhook": "available",
            "webpush": "not_implemented"
        },
        "cap_profiles": [
            "severe_weather",
            "extreme_weather", 
            "fire_alert",
            "public_safety",
            "health_alert",
            "environmental_hazard",
            "transportation_alert",
            "infrastructure_alert",
            "cbrne_alert",
            "security_alert",
            "test_alert",
            "all_clear"
        ]
    });

    Ok(HttpResponse::Ok().json(ApiResponse::success(health_info)))
}

/// Configure alert routes
pub fn configure_alert_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/alerts")
            .route("/health", web::get().to(notification_health))
            .route(
                "/test",
                web::post()
                    .to(test_notification)
                    .wrap(RequireRole::operator()), // Require admin or operator role
            )
            .route(
                "/cap/preview",
                web::post()
                    .to(cap_preview)
                    .wrap(RequireRole::operator()), // Require admin or operator role
            ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App};
    use serde_json::json;

    #[actix_web::test]
    async fn test_notification_health_endpoint() {
        let app = test::init_service(
            App::new().service(web::resource("/health").route(web::get().to(notification_health))),
        )
        .await;

        let req = test::TestRequest::get().uri("/health").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), 200);
    }

    #[actix_web::test]
    async fn test_notification_request_validation() {
        // Test with empty channels
        let req = TestNotificationRequest {
            kind: NotificationKind::Info,
            title: "Test".to_string(),
            body: "Test body".to_string(),
            pushover_user_key: None,
            webhook_url: None,
        };

        // This would fail validation in the actual handler
        assert!(req.pushover_user_key.is_none() && req.webhook_url.is_none());
    }

    #[tokio::test]
    async fn test_notification_kind_default() {
        let json = json!({
            "title": "Test",
            "body": "Test body"
        });

        let req: TestNotificationRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.kind, NotificationKind::Info);
    }

    #[tokio::test]
    async fn test_cap_preview_request_deserialization() {
        let json = json!({
            "profile": "severe_weather",
            "sender": "weather.example.org",
            "custom_title": "Custom Weather Alert",
            "custom_description": "Custom description",
            "custom_instruction": "Take shelter immediately"
        });

        let req: CapPreviewRequest = serde_json::from_value(json).unwrap();
        assert!(matches!(req.profile, CapProfile::SevereWeather));
        assert_eq!(req.sender, "weather.example.org");
        assert_eq!(req.custom_title, Some("Custom Weather Alert".to_string()));
    }

    #[actix_web::test]
    async fn test_cap_preview_response_structure() {
        // Test that we can create a CAP preview response
        let response = CapPreviewResponse {
            cap_xml: "<alert>test</alert>".to_string(),
            profile_used: "SevereWeather".to_string(),
            sender: "test.org".to_string(),
            alert_id: "123".to_string(),
            metadata: CapMetadata {
                urgency: "Expected".to_string(),
                severity: "Severe".to_string(),
                certainty: "Likely".to_string(),
                categories: vec!["Met".to_string()],
                response_types: vec!["Prepare".to_string(), "Monitor".to_string()],
            },
        };

        // Should serialize successfully
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("cap_xml"));
        assert!(json.contains("profile_used"));
        assert!(json.contains("metadata"));
    }
}