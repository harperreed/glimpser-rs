//! ABOUTME: Data models for web API with validation and OpenAPI schemas
//! ABOUTME: Defines request/response structures with serde and validation

use serde::{Deserialize, Serialize};
use std::str::FromStr;
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
    pub role: String,
    pub is_active: bool,
    pub created_at: String,
}

/// Template information for admin endpoints
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TemplateInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub template_type: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
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
    pub role: String,
    pub exp: usize, // expiration timestamp
    pub iat: usize, // issued at timestamp
}

/// User roles enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

impl FromStr for Role {
    type Err = String;

    fn from_str(role: &str) -> Result<Self, Self::Err> {
        match role.to_lowercase().as_str() {
            "admin" => Ok(Role::Admin),
            "operator" => Ok(Role::Operator),
            "viewer" => Ok(Role::Viewer),
            _ => Err(format!("Invalid role: {}", role)),
        }
    }
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Viewer => "viewer",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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
