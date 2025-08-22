//! ABOUTME: Data models for the scheduler
//! ABOUTME: Defines job kinds and scheduling configuration structures

use serde::{Deserialize, Serialize};

/// Different kinds of jobs that can be scheduled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JobKind {
    /// Capture a snapshot from a source
    Snapshot,
    /// Start a continuous capture session
    Capture,
    /// Process existing capture data
    Process,
    /// Analyze captured content (AI, motion detection, etc.)
    Analyze,
    /// Clean up old data
    Cleanup,
    /// Send notifications
    Notify,
    /// Health check / monitoring
    HealthCheck,
}

impl std::fmt::Display for JobKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobKind::Snapshot => write!(f, "snapshot"),
            JobKind::Capture => write!(f, "capture"),
            JobKind::Process => write!(f, "process"),
            JobKind::Analyze => write!(f, "analyze"),
            JobKind::Cleanup => write!(f, "cleanup"),
            JobKind::Notify => write!(f, "notify"),
            JobKind::HealthCheck => write!(f, "health_check"),
        }
    }
}

/// Job execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is waiting to be executed
    Pending,
    /// Job is currently running
    Running,
    /// Job completed successfully
    Completed,
    /// Job failed with error
    Failed,
    /// Job was cancelled
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "pending"),
            JobStatus::Running => write!(f, "running"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Configuration for a snapshot job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotJobConfig {
    /// Template ID to capture from
    pub template_id: String,
    /// Output format (jpeg, png, etc.)
    pub format: Option<String>,
    /// Quality settings
    pub quality: Option<u8>,
}

/// Configuration for a capture job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureJobConfig {
    /// Template ID to capture from
    pub template_id: String,
    /// Duration to capture (in seconds)
    pub duration: Option<u32>,
    /// Maximum file size (in bytes)
    pub max_size: Option<u64>,
}

/// Configuration for a cleanup job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupJobConfig {
    /// Age threshold for cleanup (in days)
    pub age_days: u32,
    /// File types to clean up
    pub file_types: Vec<String>,
    /// Maximum number of files to delete in one run
    pub batch_size: Option<u32>,
}

/// Common cron schedule presets
pub struct CronPresets;

impl CronPresets {
    /// Every minute (for testing)
    pub const EVERY_MINUTE: &'static str = "0 * * * * *";
    /// Every 5 minutes
    pub const EVERY_5_MINUTES: &'static str = "0 */5 * * * *";
    /// Every 15 minutes
    pub const EVERY_15_MINUTES: &'static str = "0 */15 * * * *";
    /// Every 30 minutes
    pub const EVERY_30_MINUTES: &'static str = "0 */30 * * * *";
    /// Every hour
    pub const HOURLY: &'static str = "0 0 * * * *";
    /// Every day at midnight
    pub const DAILY: &'static str = "0 0 0 * * *";
    /// Every week on Sunday at midnight
    pub const WEEKLY: &'static str = "0 0 0 * * SUN";
    /// Every month on the 1st at midnight
    pub const MONTHLY: &'static str = "0 0 0 1 * *";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_job_kind_display() {
        assert_eq!(JobKind::Snapshot.to_string(), "snapshot");
        assert_eq!(JobKind::Capture.to_string(), "capture");
        assert_eq!(JobKind::Process.to_string(), "process");
        assert_eq!(JobKind::Analyze.to_string(), "analyze");
        assert_eq!(JobKind::Cleanup.to_string(), "cleanup");
        assert_eq!(JobKind::Notify.to_string(), "notify");
        assert_eq!(JobKind::HealthCheck.to_string(), "health_check");
    }

    #[test]
    fn test_job_status_display() {
        assert_eq!(JobStatus::Pending.to_string(), "pending");
        assert_eq!(JobStatus::Running.to_string(), "running");
        assert_eq!(JobStatus::Completed.to_string(), "completed");
        assert_eq!(JobStatus::Failed.to_string(), "failed");
        assert_eq!(JobStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn test_cron_presets() {
        // Test that the presets are valid cron expressions
        let presets = [
            CronPresets::EVERY_MINUTE,
            CronPresets::EVERY_5_MINUTES,
            CronPresets::EVERY_15_MINUTES,
            CronPresets::EVERY_30_MINUTES,
            CronPresets::HOURLY,
            CronPresets::DAILY,
            CronPresets::WEEKLY,
            CronPresets::MONTHLY,
        ];

        for preset in &presets {
            assert!(
                cron::Schedule::from_str(preset).is_ok(),
                "Invalid cron preset: {}",
                preset
            );
        }
    }
}
