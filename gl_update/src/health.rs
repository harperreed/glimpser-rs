//! ABOUTME: Health checker for verifying successful updates
//! ABOUTME: Validates that updated application is running correctly

use gl_core::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Health check response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: Option<String>,
    pub timestamp: Option<String>,
    pub checks: Option<std::collections::HashMap<String, HealthCheckItem>>,
}

/// Individual health check item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckItem {
    pub status: String,
    pub message: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Health checker for post-update validation
pub struct HealthChecker {
    client: Client,
    health_url: String,
    timeout: Duration,
    retry_count: usize,
    retry_delay: Duration,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(health_url: String, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            health_url,
            timeout,
            retry_count: 3,
            retry_delay: Duration::from_secs(2),
        }
    }

    /// Configure retry parameters
    pub fn with_retries(mut self, count: usize, delay: Duration) -> Self {
        self.retry_count = count;
        self.retry_delay = delay;
        self
    }

    /// Perform health check with retries
    pub async fn check_health(&self) -> Result<()> {
        info!("Starting health check at: {}", self.health_url);

        for attempt in 1..=self.retry_count {
            debug!("Health check attempt {} of {}", attempt, self.retry_count);

            match self.perform_single_check().await {
                Ok(()) => {
                    info!("Health check passed on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Health check attempt {} failed: {}", attempt, e);

                    if attempt < self.retry_count {
                        debug!("Waiting {:?} before retry", self.retry_delay);
                        tokio::time::sleep(self.retry_delay).await;
                    } else {
                        error!("All health check attempts failed");
                        return Err(gl_core::Error::External(format!(
                            "Health check failed after {} attempts: {}",
                            self.retry_count, e
                        )));
                    }
                }
            }
        }

        unreachable!("Health check loop should have returned")
    }

    /// Perform a single health check
    async fn perform_single_check(&self) -> Result<()> {
        let start_time = std::time::Instant::now();

        let response = self
            .client
            .get(&self.health_url)
            .send()
            .await
            .map_err(|e| gl_core::Error::External(format!("Health check request failed: {}", e)))?;

        let duration = start_time.elapsed();
        debug!("Health check request completed in {:?}", duration);

        // Check HTTP status
        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "Health check returned non-success status: {}",
                response.status()
            )));
        }

        // Try to parse as JSON health response
        match response.json::<HealthResponse>().await {
            Ok(health_resp) => {
                debug!("Health response: {:?}", health_resp);
                self.validate_health_response(&health_resp)?;
            }
            Err(_) => {
                // If JSON parsing fails, just check that we got a 200 OK
                debug!("Health endpoint returned non-JSON response, but status was OK");
            }
        }

        Ok(())
    }

    /// Validate the health response structure
    fn validate_health_response(&self, response: &HealthResponse) -> Result<()> {
        // Check overall status
        if response.status.to_lowercase() != "ok" {
            return Err(gl_core::Error::External(format!(
                "Health check status is not OK: {}",
                response.status
            )));
        }

        // Check individual health checks if present
        if let Some(checks) = &response.checks {
            for (check_name, check_item) in checks {
                if check_item.status.to_lowercase() != "ok" {
                    return Err(gl_core::Error::External(format!(
                        "Health check '{}' failed with status: {}",
                        check_name, check_item.status
                    )));
                }
            }
        }

        info!("Health response validation passed");
        Ok(())
    }

    /// Perform extended health check with custom validation
    pub async fn check_health_with_validator<F>(&self, validator: F) -> Result<()>
    where
        F: Fn(&HealthResponse) -> Result<()>,
    {
        info!("Starting extended health check with custom validation");

        for attempt in 1..=self.retry_count {
            debug!(
                "Extended health check attempt {} of {}",
                attempt, self.retry_count
            );

            match self.perform_extended_check(&validator).await {
                Ok(()) => {
                    info!("Extended health check passed on attempt {}", attempt);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Extended health check attempt {} failed: {}", attempt, e);

                    if attempt < self.retry_count {
                        tokio::time::sleep(self.retry_delay).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        unreachable!()
    }

    async fn perform_extended_check<F>(&self, validator: &F) -> Result<()>
    where
        F: Fn(&HealthResponse) -> Result<()>,
    {
        let response = self
            .client
            .get(&self.health_url)
            .send()
            .await
            .map_err(|e| {
                gl_core::Error::External(format!("Extended health check request failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "Extended health check returned status: {}",
                response.status()
            )));
        }

        let health_resp: HealthResponse = response.json().await.map_err(|e| {
            gl_core::Error::External(format!("Failed to parse health response: {}", e))
        })?;

        // Run built-in validation
        self.validate_health_response(&health_resp)?;

        // Run custom validation
        validator(&health_resp)?;

        Ok(())
    }

    /// Check if a specific version is running
    pub async fn check_version(&self, expected_version: &str) -> Result<()> {
        info!("Checking for version: {}", expected_version);

        self.check_health_with_validator(|response| {
            if let Some(version) = &response.version {
                if version == expected_version {
                    info!("Version check passed: {}", version);
                    Ok(())
                } else {
                    Err(gl_core::Error::External(format!(
                        "Version mismatch: expected '{}', got '{}'",
                        expected_version, version
                    )))
                }
            } else {
                warn!("No version information in health response");
                // Don't fail if version is not reported, just warn
                Ok(())
            }
        })
        .await
    }

    /// Perform a basic connectivity test
    pub async fn connectivity_test(&self) -> Result<Duration> {
        debug!("Performing connectivity test to: {}", self.health_url);

        let start_time = std::time::Instant::now();

        let response = self
            .client
            .get(&self.health_url)
            .send()
            .await
            .map_err(|e| gl_core::Error::External(format!("Connectivity test failed: {}", e)))?;

        let duration = start_time.elapsed();

        if response.status().is_success() {
            info!("Connectivity test passed in {:?}", duration);
            Ok(duration)
        } else {
            Err(gl_core::Error::External(format!(
                "Connectivity test failed with status: {}",
                response.status()
            )))
        }
    }

    /// Get health URL
    pub fn health_url(&self) -> &str {
        &self.health_url
    }

    /// Get configured timeout
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// Create a health checker with common configurations
impl Default for HealthChecker {
    fn default() -> Self {
        Self::new(
            "http://localhost:9000/healthz".to_string(),
            Duration::from_secs(10),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_health_checker_creation() {
        let checker = HealthChecker::new(
            "http://example.com/health".to_string(),
            Duration::from_secs(30),
        );

        assert_eq!(checker.health_url(), "http://example.com/health");
        assert_eq!(checker.timeout(), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_health_checker_default() {
        let checker = HealthChecker::default();
        assert_eq!(checker.health_url(), "http://localhost:9000/healthz");
        assert_eq!(checker.timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn test_health_check_success() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "version": "1.2.3",
            "timestamp": "2023-12-01T10:00:00Z"
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        );

        let result = checker.check_health().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        )
        .with_retries(1, Duration::from_millis(100)); // Reduce retries for faster test

        let result = checker.check_health().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("status: 500"));
    }

    #[tokio::test]
    async fn test_health_check_with_checks() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "version": "1.2.3",
            "checks": {
                "database": {
                    "status": "ok",
                    "duration_ms": 15
                },
                "cache": {
                    "status": "ok",
                    "duration_ms": 3
                }
            }
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        );

        let result = checker.check_health().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_failed_check() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "checks": {
                "database": {
                    "status": "failed",
                    "message": "Connection timeout"
                }
            }
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        )
        .with_retries(1, Duration::from_millis(100));

        let result = checker.check_health().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("database"));
    }

    #[tokio::test]
    async fn test_version_check_success() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "version": "1.2.3"
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        );

        let result = checker.check_version("1.2.3").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_version_check_mismatch() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "version": "1.2.2"
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        )
        .with_retries(1, Duration::from_millis(100));

        let result = checker.check_version("1.2.3").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Version mismatch"));
    }

    #[tokio::test]
    async fn test_connectivity_test() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        );

        let result = checker.connectivity_test().await;
        assert!(result.is_ok());

        let duration = result.unwrap();
        assert!(duration < Duration::from_secs(1)); // Should be fast
    }

    #[tokio::test]
    async fn test_custom_validator() {
        let mock_server = MockServer::start().await;

        let health_response = serde_json::json!({
            "status": "ok",
            "version": "1.2.3",
            "timestamp": "expected_timestamp"
        });

        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
            .mount(&mock_server)
            .await;

        let checker = HealthChecker::new(
            format!("{}/health", mock_server.uri()),
            Duration::from_secs(5),
        );

        let result = checker
            .check_health_with_validator(|response| {
                // Custom validation: check for a specific timestamp field
                if let Some(timestamp) = &response.timestamp {
                    if timestamp == "expected_timestamp" {
                        Ok(())
                    } else {
                        Err(gl_core::Error::External(
                            "Timestamp validation failed".to_string(),
                        ))
                    }
                } else {
                    Err(gl_core::Error::External(
                        "Timestamp field not found".to_string(),
                    ))
                }
            })
            .await;

        assert!(result.is_ok());
    }
}
