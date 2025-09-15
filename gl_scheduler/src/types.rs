//! ABOUTME: Core data types and structures for the job scheduling system
//! ABOUTME: Defines job definitions, execution tracking, and scheduling metadata

use chrono::{DateTime, Utc};
use gl_core::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Job definition that describes how and when to run a job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDefinition {
    /// Unique job identifier
    pub id: String,

    /// Human-readable job name
    pub name: String,

    /// Job description
    pub description: Option<String>,

    /// Job type (must match a registered handler)
    pub job_type: String,

    /// Cron schedule expression (e.g., "0 */5 * * * *" for every 5 minutes)
    pub schedule: String,

    /// Job parameters passed to the handler
    pub parameters: serde_json::Value,

    /// Whether this job is enabled
    pub enabled: bool,

    /// Maximum number of retry attempts on failure
    pub max_retries: u32,

    /// Job timeout in seconds (overrides global setting)
    pub timeout_seconds: Option<u64>,

    /// Job priority (higher numbers = higher priority)
    pub priority: i32,

    /// Job tags for categorization and filtering
    pub tags: Vec<String>,

    /// User who created this job
    pub created_by: String,

    /// When this job was created
    pub created_at: DateTime<Utc>,

    /// When this job was last modified
    pub updated_at: DateTime<Utc>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl JobDefinition {
    /// Create a new job definition
    pub fn new(
        name: String,
        job_type: String,
        schedule: String,
        parameters: serde_json::Value,
        created_by: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Id::new().to_string(),
            name,
            description: None,
            job_type,
            schedule,
            parameters,
            enabled: true,
            max_retries: 3,
            timeout_seconds: None,
            priority: 0,
            tags: Vec::new(),
            created_by,
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
        }
    }

    /// Builder method to set description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Builder method to set enabled status
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Builder method to set max retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Builder method to set timeout
    pub fn with_timeout_seconds(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    /// Builder method to set priority
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Builder method to add tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Builder method to add a single tag
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }

    /// Builder method to add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Validate the job definition
    pub fn validate(&self) -> gl_core::Result<()> {
        if self.name.is_empty() {
            return Err(gl_core::Error::Validation(
                "Job name cannot be empty".to_string(),
            ));
        }

        if self.job_type.is_empty() {
            return Err(gl_core::Error::Validation(
                "Job type cannot be empty".to_string(),
            ));
        }

        if self.schedule.is_empty() {
            return Err(gl_core::Error::Validation(
                "Job schedule cannot be empty".to_string(),
            ));
        }

        // Validate cron expression
        if cron::Schedule::from_str(&self.schedule).is_err() {
            return Err(gl_core::Error::Validation(format!(
                "Invalid cron schedule: {}",
                self.schedule
            )));
        }

        if self.created_by.is_empty() {
            return Err(gl_core::Error::Validation(
                "Created by cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

/// Job execution record that tracks a specific run of a job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobExecution {
    /// Unique execution identifier
    pub id: String,

    /// Reference to the job definition
    pub job_id: String,

    /// Execution status
    pub status: crate::JobStatus,

    /// When execution started
    pub started_at: DateTime<Utc>,

    /// When execution completed (if finished)
    pub completed_at: Option<DateTime<Utc>>,

    /// Execution duration in milliseconds
    pub duration_ms: Option<u64>,

    /// Job execution result/output
    pub result: Option<serde_json::Value>,

    /// Error message if failed
    pub error: Option<String>,

    /// Retry attempt number (0 for first attempt)
    pub retry_count: u32,

    /// Hostname where job was executed
    pub executed_on: Option<String>,

    /// Additional execution metadata
    pub metadata: HashMap<String, String>,
}

impl JobExecution {
    /// Create a new job execution record
    pub fn new(job_id: String) -> Self {
        Self {
            id: Id::new().to_string(),
            job_id,
            status: crate::JobStatus::Pending,
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            result: None,
            error: None,
            retry_count: 0,
            executed_on: hostname::get().ok().and_then(|h| h.into_string().ok()),
            metadata: HashMap::new(),
        }
    }

    /// Mark execution as started
    pub fn start(&mut self) {
        self.status = crate::JobStatus::Running;
        self.started_at = Utc::now();
    }

    /// Mark execution as completed successfully
    pub fn complete_success(&mut self, result: serde_json::Value) {
        let now = Utc::now();
        self.status = crate::JobStatus::Completed;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
        self.result = Some(result);
    }

    /// Mark execution as failed
    pub fn complete_failure(&mut self, error: String) {
        let now = Utc::now();
        self.status = crate::JobStatus::Failed;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
        self.error = Some(error);
    }

    /// Mark execution as cancelled
    pub fn cancel(&mut self) {
        let now = Utc::now();
        self.status = crate::JobStatus::Cancelled;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
    }

    /// Mark execution as timed out
    pub fn timeout(&mut self) {
        let now = Utc::now();
        self.status = crate::JobStatus::TimedOut;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
        self.error = Some("Job execution timed out".to_string());
    }

    /// Get execution duration in milliseconds
    pub fn get_duration_ms(&self) -> Option<u64> {
        if let Some(duration) = self.duration_ms {
            Some(duration)
        } else if let Some(completed_at) = self.completed_at {
            Some((completed_at - self.started_at).num_milliseconds() as u64)
        } else {
            Some((Utc::now() - self.started_at).num_milliseconds() as u64)
        }
    }

    /// Check if execution is finished (completed, failed, cancelled, or timed out)
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            crate::JobStatus::Completed
                | crate::JobStatus::Failed
                | crate::JobStatus::Cancelled
                | crate::JobStatus::TimedOut
        )
    }
}

/// Job queue statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobQueueStats {
    /// Number of jobs currently pending
    pub pending_jobs: u64,

    /// Number of jobs currently running
    pub running_jobs: u64,

    /// Total jobs completed today
    pub completed_today: u64,

    /// Total jobs failed today
    pub failed_today: u64,

    /// Average execution time in milliseconds
    pub avg_execution_time_ms: Option<f64>,

    /// Queue throughput (jobs per hour)
    pub throughput_per_hour: Option<f64>,

    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl Default for JobQueueStats {
    fn default() -> Self {
        Self::new()
    }
}

impl JobQueueStats {
    pub fn new() -> Self {
        Self {
            pending_jobs: 0,
            running_jobs: 0,
            completed_today: 0,
            failed_today: 0,
            avg_execution_time_ms: None,
            throughput_per_hour: None,
            updated_at: Utc::now(),
        }
    }
}

/// Job schedule presets for common scenarios
pub struct SchedulePresets;

impl SchedulePresets {
    /// Every minute
    pub const EVERY_MINUTE: &'static str = "0 * * * * *";

    /// Every 5 minutes
    pub const EVERY_5_MINUTES: &'static str = "0 */5 * * * *";

    /// Every 15 minutes
    pub const EVERY_15_MINUTES: &'static str = "0 */15 * * * *";

    /// Every 30 minutes
    pub const EVERY_30_MINUTES: &'static str = "0 */30 * * * *";

    /// Every hour
    pub const HOURLY: &'static str = "0 0 * * * *";

    /// Every 6 hours
    pub const EVERY_6_HOURS: &'static str = "0 0 */6 * * *";

    /// Daily at midnight
    pub const DAILY: &'static str = "0 0 0 * * *";

    /// Daily at 2 AM (good for maintenance)
    pub const DAILY_2AM: &'static str = "0 0 2 * * *";

    /// Weekly on Sunday at 3 AM
    pub const WEEKLY: &'static str = "0 0 3 * * SUN";

    /// Monthly on the 1st at 4 AM
    pub const MONTHLY: &'static str = "0 0 4 1 * *";

    /// Get a schedule description for common patterns
    pub fn describe(schedule: &str) -> Option<&'static str> {
        match schedule {
            Self::EVERY_MINUTE => Some("Every minute"),
            Self::EVERY_5_MINUTES => Some("Every 5 minutes"),
            Self::EVERY_15_MINUTES => Some("Every 15 minutes"),
            Self::EVERY_30_MINUTES => Some("Every 30 minutes"),
            Self::HOURLY => Some("Hourly"),
            Self::EVERY_6_HOURS => Some("Every 6 hours"),
            Self::DAILY => Some("Daily at midnight"),
            Self::DAILY_2AM => Some("Daily at 2 AM"),
            Self::WEEKLY => Some("Weekly on Sunday"),
            Self::MONTHLY => Some("Monthly on the 1st"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_definition_creation() {
        let job = JobDefinition::new(
            "Test Job".to_string(),
            "test_type".to_string(),
            SchedulePresets::EVERY_5_MINUTES.to_string(),
            serde_json::json!({"param": "value"}),
            "test_user".to_string(),
        );

        assert_eq!(job.name, "Test Job");
        assert_eq!(job.job_type, "test_type");
        assert_eq!(job.schedule, SchedulePresets::EVERY_5_MINUTES);
        assert!(job.enabled);
        assert_eq!(job.max_retries, 3);
        assert_eq!(job.priority, 0);
    }

    #[test]
    fn test_job_definition_validation() {
        let job = JobDefinition::new(
            "Valid Job".to_string(),
            "valid_type".to_string(),
            SchedulePresets::DAILY.to_string(),
            serde_json::json!({}),
            "user".to_string(),
        );

        assert!(job.validate().is_ok());

        // Test invalid cron schedule
        let invalid_job = JobDefinition::new(
            "Invalid Job".to_string(),
            "type".to_string(),
            "invalid cron".to_string(),
            serde_json::json!({}),
            "user".to_string(),
        );

        assert!(invalid_job.validate().is_err());
    }

    #[test]
    fn test_job_execution_lifecycle() {
        let mut execution = JobExecution::new("job_123".to_string());

        assert_eq!(execution.status, crate::JobStatus::Pending);
        assert_eq!(execution.retry_count, 0);

        execution.start();
        assert_eq!(execution.status, crate::JobStatus::Running);

        execution.complete_success(serde_json::json!({"result": "success"}));
        assert_eq!(execution.status, crate::JobStatus::Completed);
        assert!(execution.is_finished());
        assert!(execution.result.is_some());
        assert!(execution.duration_ms.is_some());
    }

    #[test]
    fn test_schedule_presets() {
        assert_eq!(SchedulePresets::EVERY_5_MINUTES, "0 */5 * * * *");
        assert_eq!(SchedulePresets::DAILY_2AM, "0 0 2 * * *");

        assert_eq!(
            SchedulePresets::describe(SchedulePresets::EVERY_5_MINUTES),
            Some("Every 5 minutes")
        );
        assert_eq!(SchedulePresets::describe("invalid"), None);
    }
}
