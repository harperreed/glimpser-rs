//! ABOUTME: Auto-update system with signature verification
//! ABOUTME: Manages application updates and rollbacks safely

use bytes::Bytes;
use chrono::{DateTime, Utc};
use gl_core::{Id, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, error, info};

pub mod github;
pub mod health;
pub mod signature;
pub mod strategy;

pub use github::{GitHubRelease, GitHubReleaseChecker};
pub use signature::SignatureVerifier;
pub use strategy::{create_strategy, SidecarStrategy, UpdateStrategy, UpdateStrategyImpl};

pub use health::HealthChecker;
#[cfg(feature = "self-replace")]
pub use strategy::SelfReplaceStrategy;

/// Configuration for the update system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// GitHub repository (owner/repo format)
    pub repository: String,
    /// Current version of the application
    pub current_version: String,
    /// Public key for signature verification (hex encoded)
    pub public_key: String,
    /// Update strategy to use
    pub strategy: UpdateStrategyType,
    /// Check interval in seconds
    pub check_interval_seconds: u64,
    /// Health check timeout in seconds after update
    pub health_check_timeout_seconds: u64,
    /// Health check endpoint URL
    pub health_check_url: String,
    /// Binary name to update
    pub binary_name: String,
    /// Installation directory
    pub install_dir: PathBuf,
    /// Whether to auto-apply updates
    pub auto_apply: bool,
    /// GitHub API token (optional, for rate limiting)
    pub github_token: Option<String>,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            repository: "owner/repo".to_string(),
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            public_key: String::new(),
            strategy: UpdateStrategyType::Sidecar,
            check_interval_seconds: 3600, // 1 hour
            health_check_timeout_seconds: 30,
            health_check_url: "http://localhost:9000/healthz".to_string(),
            binary_name: "glimpser".to_string(),
            install_dir: PathBuf::from("/usr/local/bin"),
            auto_apply: false,
            github_token: None,
        }
    }
}

/// Available update strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateStrategyType {
    /// Sidecar process manages updates
    Sidecar,
    /// Application replaces itself
    SelfReplace,
}

/// Update information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Update ID
    pub id: String,
    /// New version
    pub version: String,
    /// Release notes
    pub notes: String,
    /// Download URL
    pub download_url: String,
    /// Signature for verification
    pub signature: String,
    /// Release timestamp
    pub published_at: DateTime<Utc>,
    /// Whether this is a security update
    pub is_security: bool,
}

impl UpdateInfo {
    pub fn new(
        version: String,
        notes: String,
        download_url: String,
        signature: String,
        published_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Id::new().to_string(),
            version,
            notes,
            download_url,
            signature,
            published_at,
            is_security: false,
        }
    }

    pub fn with_security_flag(mut self, is_security: bool) -> Self {
        self.is_security = is_security;
        self
    }
}

/// Result of an update check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    /// Whether an update is available
    pub update_available: bool,
    /// Current version
    pub current_version: String,
    /// Available update (if any)
    pub update_info: Option<UpdateInfo>,
    /// Check timestamp
    pub checked_at: DateTime<Utc>,
    /// Any errors during check
    pub error: Option<String>,
}

impl UpdateCheckResult {
    pub fn no_update(current_version: String) -> Self {
        Self {
            update_available: false,
            current_version,
            update_info: None,
            checked_at: Utc::now(),
            error: None,
        }
    }

    pub fn available(current_version: String, update_info: UpdateInfo) -> Self {
        Self {
            update_available: true,
            current_version,
            update_info: Some(update_info),
            checked_at: Utc::now(),
            error: None,
        }
    }

    pub fn error(current_version: String, error: String) -> Self {
        Self {
            update_available: false,
            current_version,
            update_info: None,
            checked_at: Utc::now(),
            error: Some(error),
        }
    }
}

/// Status of an update operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum UpdateStatus {
    /// No update in progress
    Idle,
    /// Checking for updates
    Checking,
    /// Update available, waiting for approval
    Pending,
    /// Downloading update
    Downloading,
    /// Verifying signature
    Verifying,
    /// Installing update
    Installing,
    /// Performing health check
    HealthChecking,
    /// Update completed successfully
    Success,
    /// Update failed, rolling back
    RollingBack,
    /// Update failed completely
    Failed,
}

impl UpdateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Checking => "checking",
            Self::Pending => "pending",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Installing => "installing",
            Self::HealthChecking => "health_checking",
            Self::Success => "success",
            Self::RollingBack => "rolling_back",
            Self::Failed => "failed",
        }
    }
}

/// Result of an update operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    /// Operation success
    pub success: bool,
    /// Previous version
    pub previous_version: String,
    /// New version (if successful)
    pub new_version: Option<String>,
    /// Status of the operation
    pub status: UpdateStatus,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time
    pub completed_at: Option<DateTime<Utc>>,
    /// Steps taken during update
    pub steps: Vec<UpdateStep>,
}

/// Individual step in the update process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStep {
    /// Step name
    pub name: String,
    /// Step description
    pub description: String,
    /// Whether step succeeded
    pub success: bool,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Duration of step
    pub duration_ms: u64,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl UpdateStep {
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description,
            success: true,
            error: None,
            duration_ms: 0,
            timestamp: Utc::now(),
        }
    }

    pub fn failed(mut self, error: String) -> Self {
        self.success = false;
        self.error = Some(error);
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = duration.as_millis() as u64;
        self
    }
}

impl UpdateResult {
    pub fn new(previous_version: String) -> Self {
        Self {
            success: false,
            previous_version,
            new_version: None,
            status: UpdateStatus::Idle,
            error: None,
            started_at: Utc::now(),
            completed_at: None,
            steps: Vec::new(),
        }
    }

    pub fn add_step(&mut self, step: UpdateStep) {
        self.steps.push(step);
    }

    pub fn complete_success(mut self, new_version: String) -> Self {
        self.success = true;
        self.new_version = Some(new_version);
        self.status = UpdateStatus::Success;
        self.completed_at = Some(Utc::now());
        self
    }

    pub fn complete_failure(mut self, error: String) -> Self {
        self.success = false;
        self.status = UpdateStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(Utc::now());
        self
    }
}

/// Main update service
pub struct UpdateService {
    config: UpdateConfig,
    release_checker: GitHubReleaseChecker,
    signature_verifier: SignatureVerifier,
    strategy: UpdateStrategyImpl,
    health_checker: HealthChecker,
    current_status: UpdateStatus,
}

impl UpdateService {
    /// Create a new update service
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let release_checker =
            GitHubReleaseChecker::new(config.repository.clone(), config.github_token.clone());

        let signature_verifier = SignatureVerifier::new(&config.public_key)?;

        let strategy = create_strategy(config.clone())?;

        let health_checker = HealthChecker::new(
            config.health_check_url.clone(),
            Duration::from_secs(config.health_check_timeout_seconds),
        );

        info!(
            "Update service initialized for {} (current: {})",
            config.repository, config.current_version
        );

        Ok(Self {
            config,
            release_checker,
            signature_verifier,
            strategy,
            health_checker,
            current_status: UpdateStatus::Idle,
        })
    }

    /// Check for available updates
    pub async fn check_for_updates(&mut self) -> Result<UpdateCheckResult> {
        info!("Checking for updates...");
        self.current_status = UpdateStatus::Checking;

        match self._check_for_updates().await {
            Ok(result) => {
                self.current_status = if result.update_available {
                    UpdateStatus::Pending
                } else {
                    UpdateStatus::Idle
                };
                Ok(result)
            }
            Err(e) => {
                self.current_status = UpdateStatus::Idle;
                let error_msg = format!("Update check failed: {}", e);
                error!("{}", error_msg);
                Ok(UpdateCheckResult::error(
                    self.config.current_version.clone(),
                    error_msg,
                ))
            }
        }
    }

    async fn _check_for_updates(&self) -> Result<UpdateCheckResult> {
        let latest_release = self.release_checker.get_latest_release().await?;

        // Check if this is a newer version
        if !self.is_newer_version(&latest_release.tag_name, &self.config.current_version) {
            debug!(
                "No update available: {} <= {}",
                latest_release.tag_name, self.config.current_version
            );
            return Ok(UpdateCheckResult::no_update(
                self.config.current_version.clone(),
            ));
        }

        // Find the binary asset
        let asset = latest_release
            .assets
            .iter()
            .find(|asset| asset.name.contains(&self.config.binary_name))
            .ok_or_else(|| {
                gl_core::Error::NotFound(format!(
                    "Binary asset '{}' not found in release",
                    self.config.binary_name
                ))
            })?;

        // Look for signature file
        let signature_asset = latest_release
            .assets
            .iter()
            .find(|a| a.name == format!("{}.sig", asset.name))
            .ok_or_else(|| gl_core::Error::NotFound("Signature file not found".to_string()))?;

        // Download signature
        let signature_bytes = self.release_checker.download_asset(signature_asset).await?;
        let signature = String::from_utf8(signature_bytes.to_vec())
            .map_err(|e| gl_core::Error::Validation(format!("Invalid signature format: {}", e)))?
            .trim()
            .to_string();

        let is_security = self.is_security_release(&latest_release);
        let update_info = UpdateInfo::new(
            latest_release.tag_name,
            latest_release.body.unwrap_or_default(),
            asset.browser_download_url.clone(),
            signature,
            latest_release.published_at.unwrap_or_else(Utc::now),
        )
        .with_security_flag(is_security);

        info!(
            "Update available: {} -> {}",
            self.config.current_version, update_info.version
        );

        Ok(UpdateCheckResult::available(
            self.config.current_version.clone(),
            update_info,
        ))
    }

    /// Apply an available update
    pub async fn apply_update(&mut self, update_info: UpdateInfo) -> Result<UpdateResult> {
        info!("Applying update to version: {}", update_info.version);

        let mut result = UpdateResult::new(self.config.current_version.clone());

        // Download
        self.current_status = UpdateStatus::Downloading;
        let step_start = std::time::Instant::now();
        match self.download_update(&update_info).await {
            Ok(binary_data) => {
                result.add_step(
                    UpdateStep::new(
                        "download".to_string(),
                        "Downloaded update binary".to_string(),
                    )
                    .with_duration(step_start.elapsed()),
                );

                // Verify signature
                self.current_status = UpdateStatus::Verifying;
                let step_start = std::time::Instant::now();
                match self
                    .signature_verifier
                    .verify(&binary_data, &update_info.signature)
                {
                    Ok(()) => {
                        result.add_step(
                            UpdateStep::new(
                                "verify".to_string(),
                                "Verified binary signature".to_string(),
                            )
                            .with_duration(step_start.elapsed()),
                        );

                        // Install
                        self.current_status = UpdateStatus::Installing;
                        let step_start = std::time::Instant::now();
                        match self.strategy.install_update(binary_data).await {
                            Ok(()) => {
                                result.add_step(
                                    UpdateStep::new(
                                        "install".to_string(),
                                        "Installed new binary".to_string(),
                                    )
                                    .with_duration(step_start.elapsed()),
                                );

                                // Health check
                                self.current_status = UpdateStatus::HealthChecking;
                                let step_start = std::time::Instant::now();
                                match self.health_checker.check_health().await {
                                    Ok(()) => {
                                        result.add_step(
                                            UpdateStep::new(
                                                "health_check".to_string(),
                                                "Health check passed".to_string(),
                                            )
                                            .with_duration(step_start.elapsed()),
                                        );

                                        info!("Update successful: {}", update_info.version);
                                        self.current_status = UpdateStatus::Success;
                                        self.config.current_version = update_info.version.clone();
                                        return Ok(result.complete_success(update_info.version));
                                    }
                                    Err(e) => {
                                        error!("Health check failed: {}", e);
                                        result.add_step(
                                            UpdateStep::new(
                                                "health_check".to_string(),
                                                "Health check failed".to_string(),
                                            )
                                            .failed(e.to_string())
                                            .with_duration(step_start.elapsed()),
                                        );

                                        // Rollback
                                        self.rollback(&mut result).await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Installation failed: {}", e);
                                result.add_step(
                                    UpdateStep::new(
                                        "install".to_string(),
                                        "Failed to install update".to_string(),
                                    )
                                    .failed(e.to_string())
                                    .with_duration(step_start.elapsed()),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!("Signature verification failed: {}", e);
                        result.add_step(
                            UpdateStep::new(
                                "verify".to_string(),
                                "Signature verification failed".to_string(),
                            )
                            .failed(e.to_string())
                            .with_duration(step_start.elapsed()),
                        );
                    }
                }
            }
            Err(e) => {
                error!("Download failed: {}", e);
                result.add_step(
                    UpdateStep::new(
                        "download".to_string(),
                        "Failed to download update".to_string(),
                    )
                    .failed(e.to_string())
                    .with_duration(step_start.elapsed()),
                );
            }
        }

        self.current_status = UpdateStatus::Failed;
        Ok(result.complete_failure("Update process failed".to_string()))
    }

    async fn download_update(&self, update_info: &UpdateInfo) -> Result<Bytes> {
        info!("Downloading update from: {}", update_info.download_url);

        let response = reqwest::get(&update_info.download_url)
            .await
            .map_err(|e| gl_core::Error::External(format!("Download request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "Download failed with status: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to read download: {}", e)))?;

        info!("Downloaded {} bytes", bytes.len());
        Ok(bytes)
    }

    async fn rollback(&mut self, result: &mut UpdateResult) {
        info!("Starting rollback...");
        self.current_status = UpdateStatus::RollingBack;

        let step_start = std::time::Instant::now();
        match self.strategy.rollback().await {
            Ok(()) => {
                info!("Rollback successful");
                result.add_step(
                    UpdateStep::new(
                        "rollback".to_string(),
                        "Rolled back to previous version".to_string(),
                    )
                    .with_duration(step_start.elapsed()),
                );
            }
            Err(e) => {
                error!("Rollback failed: {}", e);
                result.add_step(
                    UpdateStep::new("rollback".to_string(), "Rollback failed".to_string())
                        .failed(e.to_string())
                        .with_duration(step_start.elapsed()),
                );
            }
        }
    }

    /// Get current update status
    pub fn status(&self) -> UpdateStatus {
        self.current_status.clone()
    }

    /// Check if a version is newer than another
    fn is_newer_version(&self, new_version: &str, current_version: &str) -> bool {
        // Simple semantic version comparison
        // For production, should use a proper semver crate
        let clean_new = new_version.trim_start_matches('v');
        let clean_current = current_version.trim_start_matches('v');

        // Basic comparison - in production, use semver crate
        clean_new > clean_current
    }

    /// Check if this is a security release
    fn is_security_release(&self, release: &GitHubRelease) -> bool {
        if let Some(body) = &release.body {
            let body_lower = body.to_lowercase();
            body_lower.contains("security")
                || body_lower.contains("vulnerability")
                || body_lower.contains("cve-")
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert_eq!(config.repository, "owner/repo");
        assert_eq!(config.current_version, env!("CARGO_PKG_VERSION"));
        assert!(!config.auto_apply);
        assert_eq!(config.check_interval_seconds, 3600);
    }

    #[test]
    fn test_update_info_creation() {
        let info = UpdateInfo::new(
            "v1.2.3".to_string(),
            "Bug fixes".to_string(),
            "https://example.com/release".to_string(),
            "signature123".to_string(),
            Utc::now(),
        );

        assert_eq!(info.version, "v1.2.3");
        assert_eq!(info.notes, "Bug fixes");
        assert!(!info.is_security);
        assert!(!info.id.is_empty());
    }

    #[test]
    fn test_update_result_creation() {
        let mut result = UpdateResult::new("v1.0.0".to_string());
        assert_eq!(result.previous_version, "v1.0.0");
        assert!(!result.success);
        assert_eq!(result.status, UpdateStatus::Idle);

        result.add_step(UpdateStep::new("test".to_string(), "Test step".to_string()));
        assert_eq!(result.steps.len(), 1);
    }

    #[test]
    fn test_update_status_as_str() {
        assert_eq!(UpdateStatus::Idle.as_str(), "idle");
        assert_eq!(UpdateStatus::Downloading.as_str(), "downloading");
        assert_eq!(UpdateStatus::Success.as_str(), "success");
        assert_eq!(UpdateStatus::Failed.as_str(), "failed");
    }

    #[test]
    fn test_update_check_result() {
        let result = UpdateCheckResult::no_update("v1.0.0".to_string());
        assert!(!result.update_available);
        assert!(result.update_info.is_none());
        assert!(result.error.is_none());

        let info = UpdateInfo::new(
            "v1.1.0".to_string(),
            "New version".to_string(),
            "https://example.com".to_string(),
            "sig123".to_string(),
            Utc::now(),
        );
        let result = UpdateCheckResult::available("v1.0.0".to_string(), info);
        assert!(result.update_available);
        assert!(result.update_info.is_some());
    }
}
