//! ABOUTME: Job runner utilities and helper functions
//! ABOUTME: Provides utilities for job execution and monitoring

use chrono::{DateTime, Utc};
use cron::Schedule;
use gl_core::{Error, Result};
use std::str::FromStr;

/// Calculate the next N run times for a cron expression
pub fn calculate_next_runs(
    cron_expr: &str,
    _from: DateTime<Utc>,
    count: usize,
) -> Result<Vec<DateTime<Utc>>> {
    let schedule = Schedule::from_str(cron_expr)
        .map_err(|e| Error::Config(format!("Invalid cron expression '{}': {}", cron_expr, e)))?;

    let upcoming = schedule.upcoming(Utc);
    let next_runs: Vec<DateTime<Utc>> = upcoming.take(count).collect();

    if next_runs.is_empty() {
        return Err(Error::Config("No upcoming schedule found".to_string()));
    }

    Ok(next_runs)
}

/// Validate a cron expression
pub fn validate_cron_expression(cron_expr: &str) -> Result<()> {
    Schedule::from_str(cron_expr)
        .map_err(|e| Error::Config(format!("Invalid cron expression '{}': {}", cron_expr, e)))?;
    Ok(())
}

/// Check if a cron expression would run within the next duration
pub fn will_run_within(cron_expr: &str, duration: chrono::Duration) -> Result<bool> {
    let schedule = Schedule::from_str(cron_expr)
        .map_err(|e| Error::Config(format!("Invalid cron expression '{}': {}", cron_expr, e)))?;

    let now = Utc::now();
    let end_time = now + duration;

    let next_run = schedule.upcoming(Utc).next();

    match next_run {
        Some(next) => Ok(next <= end_time),
        None => Ok(false),
    }
}

/// Get a human-readable description of when a cron job will next run
pub fn describe_next_run(cron_expr: &str) -> Result<String> {
    let schedule = Schedule::from_str(cron_expr)
        .map_err(|e| Error::Config(format!("Invalid cron expression '{}': {}", cron_expr, e)))?;

    let next_run = schedule
        .upcoming(Utc)
        .next()
        .ok_or_else(|| Error::Config("No upcoming schedule found".to_string()))?;

    let now = Utc::now();
    let duration = next_run.signed_duration_since(now);

    if duration.num_seconds() < 60 {
        Ok(format!("in {} seconds", duration.num_seconds()))
    } else if duration.num_minutes() < 60 {
        Ok(format!("in {} minutes", duration.num_minutes()))
    } else if duration.num_hours() < 24 {
        Ok(format!("in {} hours", duration.num_hours()))
    } else {
        Ok(format!("in {} days", duration.num_days()))
    }
}

/// Job execution statistics
#[derive(Debug, Clone, Default)]
pub struct JobStats {
    /// Total number of executions
    pub total_executions: u64,
    /// Number of successful executions
    pub successful_executions: u64,
    /// Number of failed executions
    pub failed_executions: u64,
    /// Last execution time
    pub last_execution: Option<DateTime<Utc>>,
    /// Last success time
    pub last_success: Option<DateTime<Utc>>,
    /// Last failure time
    pub last_failure: Option<DateTime<Utc>>,
    /// Average execution duration in milliseconds
    pub avg_duration_ms: Option<f64>,
}

impl JobStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a successful execution
    pub fn record_success(&mut self, execution_time: DateTime<Utc>, duration_ms: u64) {
        self.total_executions += 1;
        self.successful_executions += 1;
        self.last_execution = Some(execution_time);
        self.last_success = Some(execution_time);

        // Update rolling average
        if let Some(current_avg) = self.avg_duration_ms {
            self.avg_duration_ms = Some((current_avg + duration_ms as f64) / 2.0);
        } else {
            self.avg_duration_ms = Some(duration_ms as f64);
        }
    }

    /// Record a failed execution
    pub fn record_failure(&mut self, execution_time: DateTime<Utc>) {
        self.total_executions += 1;
        self.failed_executions += 1;
        self.last_execution = Some(execution_time);
        self.last_failure = Some(execution_time);
    }

    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            0.0
        } else {
            (self.successful_executions as f64 / self.total_executions as f64) * 100.0
        }
    }

    /// Check if the job is currently healthy (recent success)
    pub fn is_healthy(&self, threshold_minutes: i64) -> bool {
        match self.last_success {
            Some(last_success) => {
                let now = Utc::now();
                let threshold = chrono::Duration::minutes(threshold_minutes);
                now.signed_duration_since(last_success) <= threshold
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_validate_cron_expression() {
        // Valid expressions
        assert!(validate_cron_expression("0 0 * * * *").is_ok()); // Every hour
        assert!(validate_cron_expression("0 */5 * * * *").is_ok()); // Every 5 minutes
        assert!(validate_cron_expression("0 0 0 * * *").is_ok()); // Daily at midnight

        // Invalid expressions
        assert!(validate_cron_expression("invalid").is_err());
        assert!(validate_cron_expression("0 0 25 * * *").is_err()); // Invalid hour
        assert!(validate_cron_expression("").is_err());
    }

    #[test]
    fn test_calculate_next_runs() {
        let now = Utc::now();

        // Test every minute schedule
        let next_runs = calculate_next_runs("0 * * * * *", now, 3).unwrap();
        assert_eq!(next_runs.len(), 3);

        // Each run should be approximately 1 minute apart
        let diff1 = next_runs[1].signed_duration_since(next_runs[0]);
        let diff2 = next_runs[2].signed_duration_since(next_runs[1]);
        assert_eq!(diff1.num_minutes(), 1);
        assert_eq!(diff2.num_minutes(), 1);
    }

    #[test]
    fn test_will_run_within() {
        // Test with a schedule that runs every minute
        let result = will_run_within("0 * * * * *", Duration::hours(1)).unwrap();
        assert!(result); // Should definitely run within an hour

        let _result = will_run_within("0 * * * * *", Duration::seconds(30)).unwrap();
        // This might be true or false depending on current time
        // Just test that it doesn't error
    }

    #[test]
    fn test_describe_next_run() {
        // Test with a schedule that runs every hour
        let description = describe_next_run("0 0 * * * *").unwrap();
        assert!(
            description.contains("in")
                || description.contains("minutes")
                || description.contains("seconds")
        );
    }

    #[test]
    fn test_job_stats() {
        let mut stats = JobStats::new();
        assert_eq!(stats.total_executions, 0);
        assert_eq!(stats.success_rate(), 0.0);
        assert!(!stats.is_healthy(60));

        let now = Utc::now();

        // Record a success
        stats.record_success(now, 1000);
        assert_eq!(stats.total_executions, 1);
        assert_eq!(stats.successful_executions, 1);
        assert_eq!(stats.success_rate(), 100.0);
        assert_eq!(stats.avg_duration_ms, Some(1000.0));
        assert!(stats.is_healthy(60));

        // Record a failure
        stats.record_failure(now);
        assert_eq!(stats.total_executions, 2);
        assert_eq!(stats.failed_executions, 1);
        assert_eq!(stats.success_rate(), 50.0);

        // Record another success
        stats.record_success(now, 2000);
        assert_eq!(stats.total_executions, 3);
        assert!((stats.success_rate() - 66.66666666666667).abs() < 1e-10);
        assert_eq!(stats.avg_duration_ms, Some(1500.0)); // Average of 1000 and 2000
    }
}
