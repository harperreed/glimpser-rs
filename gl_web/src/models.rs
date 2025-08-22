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
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
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
    
    pub fn with_details(error: impl Into<String>, message: impl Into<String>, details: serde_json::Value) -> Self {
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
    pub sub: String,      // user ID
    pub email: String,
    pub role: String,
    pub exp: usize,       // expiration timestamp
    pub iat: usize,       // issued at timestamp
}

/// User roles enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

impl Role {
    pub fn from_str(role: &str) -> Option<Self> {
        match role.to_lowercase().as_str() {
            "admin" => Some(Role::Admin),
            "operator" => Some(Role::Operator),
            "viewer" => Some(Role::Viewer),
            _ => None,
        }
    }
    
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