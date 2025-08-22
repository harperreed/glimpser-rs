//! ABOUTME: Job handlers for different types of scheduled tasks
//! ABOUTME: Implements the JobHandler trait for various job types

use async_trait::async_trait;
use gl_core::{Error, Result};
use tracing::{debug, info, instrument};

use crate::{CaptureJobConfig, JobContext, JobHandler, JobKind, SnapshotJobConfig};

/// Handler for snapshot jobs
pub struct SnapshotJobHandler;

#[async_trait]
impl JobHandler for SnapshotJobHandler {
    async fn execute(&self, context: JobContext) -> Result<String> {
        let config: SnapshotJobConfig =
            serde_json::from_value(context.scheduled_job.config.clone())
                .map_err(|e| Error::Config(format!("Invalid snapshot job config: {}", e)))?;

        info!(
            job_id = %context.scheduled_job.id,
            template_id = %config.template_id,
            "Executing snapshot job"
        );

        // For now, we'll create a simple file-based snapshot
        // In a real implementation, this would look up the template and use the appropriate capture source
        let result = format!(
            "Snapshot job completed for template {} at {}",
            config.template_id,
            context.execution_time.format("%Y-%m-%d %H:%M:%S UTC")
        );

        debug!(
            job_id = %context.scheduled_job.id,
            result = %result,
            "Snapshot job completed"
        );

        Ok(result)
    }

    fn supported_kinds(&self) -> Vec<JobKind> {
        vec![JobKind::Snapshot]
    }
}

/// Handler for capture jobs (continuous recording)
pub struct CaptureJobHandler;

#[async_trait]
impl JobHandler for CaptureJobHandler {
    async fn execute(&self, context: JobContext) -> Result<String> {
        let config: CaptureJobConfig = serde_json::from_value(context.scheduled_job.config.clone())
            .map_err(|e| Error::Config(format!("Invalid capture job config: {}", e)))?;

        info!(
            job_id = %context.scheduled_job.id,
            template_id = %config.template_id,
            duration = ?config.duration,
            "Executing capture job"
        );

        // For now, simulate capture work
        let duration = config.duration.unwrap_or(60); // Default 60 seconds

        // In a real implementation, this would:
        // 1. Look up the template configuration
        // 2. Create the appropriate capture source
        // 3. Start recording for the specified duration
        // 4. Save the result to storage

        let result = format!(
            "Capture job completed for template {} (duration: {}s) at {}",
            config.template_id,
            duration,
            context.execution_time.format("%Y-%m-%d %H:%M:%S UTC")
        );

        debug!(
            job_id = %context.scheduled_job.id,
            result = %result,
            "Capture job completed"
        );

        Ok(result)
    }

    fn supported_kinds(&self) -> Vec<JobKind> {
        vec![JobKind::Capture]
    }
}

/// Handler for cleanup jobs
pub struct CleanupJobHandler;

#[async_trait]
impl JobHandler for CleanupJobHandler {
    #[instrument(skip(self, context))]
    async fn execute(&self, context: JobContext) -> Result<String> {
        info!(
            job_id = %context.scheduled_job.id,
            "Executing cleanup job"
        );

        // Simulate cleanup work
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let result = format!(
            "Cleanup job completed at {}",
            context.execution_time.format("%Y-%m-%d %H:%M:%S UTC")
        );

        debug!(result = %result, "Cleanup job completed");
        Ok(result)
    }

    fn supported_kinds(&self) -> Vec<JobKind> {
        vec![JobKind::Cleanup]
    }
}

/// Handler for health check jobs
pub struct HealthCheckJobHandler;

#[async_trait]
impl JobHandler for HealthCheckJobHandler {
    #[instrument(skip(self, context))]
    async fn execute(&self, context: JobContext) -> Result<String> {
        info!(
            job_id = %context.scheduled_job.id,
            "Executing health check job"
        );

        // Simulate health check work
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let result = format!(
            "Health check completed at {}",
            context.execution_time.format("%Y-%m-%d %H:%M:%S UTC")
        );

        debug!(result = %result, "Health check completed");
        Ok(result)
    }

    fn supported_kinds(&self) -> Vec<JobKind> {
        vec![JobKind::HealthCheck]
    }
}

/// Create default job handlers for common job types
pub fn create_default_handlers() -> Vec<std::sync::Arc<dyn JobHandler>> {
    vec![
        std::sync::Arc::new(SnapshotJobHandler),
        std::sync::Arc::new(CaptureJobHandler),
        std::sync::Arc::new(CleanupJobHandler),
        std::sync::Arc::new(HealthCheckJobHandler),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobKind, ScheduledJob};
    use chrono::Utc;

    async fn create_test_context(kind: JobKind, config: serde_json::Value) -> JobContext {
        let scheduled_job = ScheduledJob::new(
            "test_job".to_string(),
            kind,
            "0 * * * * *".to_string(),
            "test_user".to_string(),
            Some("test_template".to_string()),
            config,
        );

        // Create a mock database pool for testing
        let db_pool =
            crate::SqlitePool::connect(":memory:").expect("Failed to create mock database pool");

        JobContext {
            scheduled_job,
            db_pool,
            execution_time: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_snapshot_job_handler() {
        let handler = SnapshotJobHandler;
        assert_eq!(handler.supported_kinds(), vec![JobKind::Snapshot]);

        let config = serde_json::json!({
            "template_id": "test_template",
            "format": "jpeg",
            "quality": 85
        });

        let context = create_test_context(JobKind::Snapshot, config).await;
        let result = handler.execute(context).await.unwrap();

        assert!(result.contains("Snapshot job completed"));
        assert!(result.contains("test_template"));
    }

    #[tokio::test]
    async fn test_capture_job_handler() {
        let handler = CaptureJobHandler;
        assert_eq!(handler.supported_kinds(), vec![JobKind::Capture]);

        let config = serde_json::json!({
            "template_id": "test_template",
            "duration": 30,
            "max_size": 1000000
        });

        let context = create_test_context(JobKind::Capture, config).await;
        let result = handler.execute(context).await.unwrap();

        assert!(result.contains("Capture job completed"));
        assert!(result.contains("test_template"));
        assert!(result.contains("30s"));
    }

    #[tokio::test]
    async fn test_cleanup_job_handler() {
        let handler = CleanupJobHandler;
        assert_eq!(handler.supported_kinds(), vec![JobKind::Cleanup]);

        let config = serde_json::json!({});
        let context = create_test_context(JobKind::Cleanup, config).await;
        let result = handler.execute(context).await.unwrap();

        assert!(result.contains("Cleanup job completed"));
    }

    #[tokio::test]
    async fn test_health_check_job_handler() {
        let handler = HealthCheckJobHandler;
        assert_eq!(handler.supported_kinds(), vec![JobKind::HealthCheck]);

        let config = serde_json::json!({});
        let context = create_test_context(JobKind::HealthCheck, config).await;
        let result = handler.execute(context).await.unwrap();

        assert!(result.contains("Health check completed"));
    }

    #[tokio::test]
    async fn test_create_default_handlers() {
        let handlers = create_default_handlers();
        assert_eq!(handlers.len(), 4);

        // Test that each handler supports the expected job kinds
        let mut supported_kinds = Vec::new();
        for handler in &handlers {
            supported_kinds.extend(handler.supported_kinds());
        }

        assert!(supported_kinds.contains(&JobKind::Snapshot));
        assert!(supported_kinds.contains(&JobKind::Capture));
        assert!(supported_kinds.contains(&JobKind::Cleanup));
        assert!(supported_kinds.contains(&JobKind::HealthCheck));
    }
}
