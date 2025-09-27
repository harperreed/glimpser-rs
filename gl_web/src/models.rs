//! ABOUTME: Data models for web API with validation and OpenAPI schemas
//! ABOUTME: Defines request/response structures with serde and validation

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

/// Request body for user login
#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,

    #[validate(length(min = 1))]
    pub password: String,
}

/// Request body for first admin user signup
#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct SignupRequest {
    #[validate(length(min = 1))]
    pub username: String,

    #[validate(email)]
    pub email: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters long"))]
    pub password: String,
}

/// Response for successful login
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub user: UserInfo,
}

/// User information response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub email: String,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: String,
}

/// Stream information for admin endpoints (settings UI)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AdminStreamInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub stream_type: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
    pub status: String, // "active" or "inactive"
}

/// Generic API response wrapper
#[derive(Debug, Serialize, Deserialize, ToSchema)]
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

/// Standard error response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ErrorResponse {
    pub fn new(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        error: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            error: error.into(),
            message: message.into(),
            details: Some(details),
        }
    }
}

/// JWT claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user ID
    pub email: String,
    pub exp: usize,  // expiration timestamp
    pub iat: usize,  // issued at timestamp
    pub iss: String, // issuer
}

// Role enum removed - using simple is_admin boolean instead

/// RFC 7807 Problem Details response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProblemDetails {
    /// The problem type URI (required)
    #[serde(rename = "type")]
    pub problem_type: String,

    /// Human-readable summary of the problem (required)
    pub title: String,

    /// HTTP status code (optional but recommended)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,

    /// Human-readable explanation specific to this occurrence
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,

    /// URI reference that identifies the specific occurrence
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// Additional problem-specific extension fields
    #[serde(flatten)]
    pub extensions: serde_json::Map<String, serde_json::Value>,
}

impl ProblemDetails {
    /// Create a new problem details response
    pub fn new(problem_type: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            problem_type: problem_type.into(),
            title: title.into(),
            status: None,
            detail: None,
            instance: None,
            extensions: serde_json::Map::new(),
        }
    }

    /// Set the HTTP status code
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Set the detail message
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set the instance URI
    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    /// Add an extension field
    pub fn with_extension(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    /// Create a validation error problem
    pub fn validation_error(detail: impl Into<String>) -> Self {
        Self::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.5.1",
            "Bad Request",
        )
        .with_status(400)
        .with_detail(detail.into())
    }

    /// Create a rate limit error problem
    pub fn rate_limit_error(retry_after: Option<u64>) -> Self {
        let mut problem = Self::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.6.4",
            "Too Many Requests",
        )
        .with_status(429)
        .with_detail("Rate limit exceeded");

        if let Some(retry_after) = retry_after {
            problem = problem
                .with_extension("retry_after", serde_json::Value::Number(retry_after.into()));
        }

        problem
    }

    /// Create a payload too large error problem
    pub fn payload_too_large_error(max_size: u64) -> Self {
        Self::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.5.11",
            "Payload Too Large",
        )
        .with_status(413)
        .with_detail(format!(
            "Request payload exceeds maximum size of {} bytes",
            max_size
        ))
        .with_extension("max_size", serde_json::Value::Number(max_size.into()))
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TemplateKind {
    Rtsp(RtspTemplate),
    Ffmpeg(FfmpegTemplate),
    File(FileTemplate),
    Website(WebsiteTemplate),
    Yt(YtTemplate),
}

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct RtspTemplate {
    #[validate(length(min = 1))]
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct FfmpegTemplate {
    #[serde(rename = "source_url")]
    #[validate(length(min = 1))]
    pub source_url: String,
}

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct FileTemplate {
    #[serde(rename = "file_path")]
    #[validate(length(min = 1))]
    pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct WebsiteTemplate {
    #[validate(url)]
    pub url: String,
    #[serde(default)]
    pub headless: Option<bool>,
    #[serde(default)]
    pub stealth: Option<bool>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(rename = "element_selector")]
    #[serde(default)]
    pub element_selector: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Validate, ToSchema)]
pub struct YtTemplate {
    #[validate(url)]
    pub url: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub is_live: Option<bool>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub options: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Stream information response matching frontend expectations
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StreamInfo {
    pub id: String,
    pub name: String,
    pub source: String,
    pub status: StreamStatus,
    pub resolution: String,
    pub fps: u32,
    pub last_frame_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

/// Stream status enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StreamStatus {
    Active,
    Inactive,
    Error,
    Starting,
    Stopping,
}

impl StreamStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamStatus::Active => "active",
            StreamStatus::Inactive => "inactive",
            StreamStatus::Error => "error",
            StreamStatus::Starting => "starting",
            StreamStatus::Stopping => "stopping",
        }
    }
}

impl std::fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Validation error details for RFC 7807 responses
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ValidationError {
    pub field: String,
    pub code: String,
    pub message: String,
    pub value: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_rtsp_template() {
        let json = r#"{"kind":"rtsp","url":"rtsp://example"}"#;
        let config: TemplateKind = serde_json::from_str(json).unwrap();
        match config {
            TemplateKind::Rtsp(t) => assert_eq!(t.url, "rtsp://example"),
            _ => panic!("expected rtsp"),
        }
    }

    #[test]
    fn deserialize_file_template() {
        let json = r#"{"kind":"file","file_path":"/tmp/video.mp4"}"#;
        let config: TemplateKind = serde_json::from_str(json).unwrap();
        match config {
            TemplateKind::File(t) => assert_eq!(t.file_path, "/tmp/video.mp4"),
            _ => panic!("expected file"),
        }
    }

    #[test]
    fn deserialize_ffmpeg_template() {
        let json = r#"{"kind":"ffmpeg","source_url":"rtsp://cam"}"#;
        let config: TemplateKind = serde_json::from_str(json).unwrap();
        match config {
            TemplateKind::Ffmpeg(t) => assert_eq!(t.source_url, "rtsp://cam"),
            _ => panic!("expected ffmpeg"),
        }
    }

    #[test]
    fn deserialize_website_template() {
        let json = r#"{"kind":"website","url":"https://example.com","width":800,"height":600}"#;
        let config: TemplateKind = serde_json::from_str(json).unwrap();
        match config {
            TemplateKind::Website(t) => {
                assert_eq!(t.url, "https://example.com");
                assert_eq!(t.width, Some(800));
                assert_eq!(t.height, Some(600));
            }
            _ => panic!("expected website"),
        }
    }

    #[test]
    fn deserialize_yt_template() {
        let json = r#"{"kind":"yt","url":"https://youtu.be/test"}"#;
        let config: TemplateKind = serde_json::from_str(json).unwrap();
        match config {
            TemplateKind::Yt(t) => assert_eq!(t.url, "https://youtu.be/test"),
            _ => panic!("expected yt"),
        }
    }
}
