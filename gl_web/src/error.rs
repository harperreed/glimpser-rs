//! ABOUTME: Error handling utilities for RFC 7807 Problem Details responses
//! ABOUTME: Converts validation errors and other API errors to standardized format

use crate::models::{ProblemDetails, ValidationError};
use actix_web::{HttpResponse, ResponseError};
use std::fmt;
use validator::ValidationErrors;

/// API error wrapper for RFC 7807 Problem Details
#[derive(Debug)]
pub struct ApiError {
    pub problem: ProblemDetails,
    pub status_code: u16,
}

impl ApiError {
    /// Create a new API error
    pub fn new(problem: ProblemDetails) -> Self {
        let status_code = problem.status.unwrap_or(500);
        Self {
            problem,
            status_code,
        }
    }

    /// Create a validation error from validator::ValidationErrors
    pub fn validation(errors: ValidationErrors) -> Self {
        let validation_errors: Vec<ValidationError> = errors
            .field_errors()
            .into_iter()
            .flat_map(|(field, field_errors)| {
                field_errors.iter().map(move |error| ValidationError {
                    field: field.to_string(),
                    code: error.code.to_string(),
                    message: error
                        .message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| format!("Invalid value for field '{}'", field)),
                    value: error.params.get("value").cloned(),
                })
            })
            .collect();

        let problem = ProblemDetails::validation_error("Request validation failed").with_extension(
            "errors",
            serde_json::to_value(validation_errors).unwrap_or_default(),
        );

        Self::new(problem)
    }

    /// Create a bad request error
    pub fn bad_request(detail: impl Into<String>) -> Self {
        let problem = ProblemDetails::validation_error(detail.into());
        Self::new(problem)
    }

    /// Create an unauthorized error
    pub fn unauthorized(detail: impl Into<String>) -> Self {
        let problem = ProblemDetails::new(
            "https://datatracker.ietf.org/rfc/rfc7235.html#section-3.1",
            "Unauthorized",
        )
        .with_status(401)
        .with_detail(detail.into());

        Self::new(problem)
    }

    /// Create a forbidden error
    pub fn forbidden(detail: impl Into<String>) -> Self {
        let problem = ProblemDetails::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.5.3",
            "Forbidden",
        )
        .with_status(403)
        .with_detail(detail.into());

        Self::new(problem)
    }

    /// Create a not found error
    pub fn not_found(detail: impl Into<String>) -> Self {
        let problem = ProblemDetails::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.5.4",
            "Not Found",
        )
        .with_status(404)
        .with_detail(detail.into());

        Self::new(problem)
    }

    /// Create an internal server error
    pub fn internal_server_error(detail: impl Into<String>) -> Self {
        let problem = ProblemDetails::new(
            "https://datatracker.ietf.org/rfc/rfc7231.html#section-6.6.1",
            "Internal Server Error",
        )
        .with_status(500)
        .with_detail(detail.into());

        Self::new(problem)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.problem.title,
            self.problem
                .detail
                .as_ref()
                .unwrap_or(&"No details available".to_string())
        )
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        actix_web::http::StatusCode::from_u16(self.status_code)
            .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR)
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .content_type("application/problem+json")
            .json(&self.problem)
    }
}

/// Convert validator::ValidationErrors to ApiError
impl From<ValidationErrors> for ApiError {
    fn from(errors: ValidationErrors) -> Self {
        Self::validation(errors)
    }
}

/// Convert gl_core::Error to ApiError
impl From<gl_core::Error> for ApiError {
    fn from(error: gl_core::Error) -> Self {
        match error {
            gl_core::Error::NotFound(msg) => Self::not_found(msg),
            gl_core::Error::Validation(msg) => Self::bad_request(msg),
            gl_core::Error::Database(msg) => {
                Self::internal_server_error(format!("Database error: {}", msg))
            }
            gl_core::Error::Config(msg) => {
                Self::internal_server_error(format!("Configuration error: {}", msg))
            }
            gl_core::Error::External(msg) => {
                Self::internal_server_error(format!("External service error: {}", msg))
            }
            gl_core::Error::Io(e) => Self::internal_server_error(format!("IO error: {}", e)),
            gl_core::Error::Storage(msg) => {
                Self::internal_server_error(format!("Storage error: {}", msg))
            }
        }
    }
}

/// Result type alias for API handlers
pub type ApiResult<T> = Result<T, ApiError>;

/// Helper macro to convert Result<T, E> to ApiResult<T> where E: Into<ApiError>
#[macro_export]
macro_rules! api_result {
    ($result:expr) => {
        $result.map_err(Into::into)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_error() {
        let mut errors = ValidationErrors::new();
        let field_error = validator::ValidationError::new("required");
        errors.add("email", field_error);

        let api_error = ApiError::validation(errors);
        assert_eq!(api_error.status_code, 400);
        assert_eq!(api_error.problem.title, "Bad Request");
        assert!(api_error.problem.extensions.contains_key("errors"));
    }

    #[test]
    fn test_error_responses() {
        let bad_request = ApiError::bad_request("Invalid input");
        assert_eq!(bad_request.status_code, 400);

        let unauthorized = ApiError::unauthorized("Token required");
        assert_eq!(unauthorized.status_code, 401);

        let forbidden = ApiError::forbidden("Insufficient permissions");
        assert_eq!(forbidden.status_code, 403);

        let not_found = ApiError::not_found("Resource not found");
        assert_eq!(not_found.status_code, 404);

        let internal = ApiError::internal_server_error("Something went wrong");
        assert_eq!(internal.status_code, 500);
    }
}
