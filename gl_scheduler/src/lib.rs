//! ABOUTME: Job scheduling system for automated tasks and background processing
//! ABOUTME: Provides cron-like scheduling, task queues, and asynchronous job execution

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gl_core::{Id, Result};
use gl_db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_cron_scheduler::JobScheduler as TokioCronScheduler;
use tracing::{debug, info, warn};

/// Result of a capture operation
#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub data: Vec<u8>,
    pub storage_path: String,
    pub storage_uri: String,
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub checksum: String,
}

/// Trait for capture operations that jobs can use
#[async_trait]
pub trait CaptureService: Send + Sync {
    /// Capture a frame from the specified stream
    async fn capture(&self, stream_id: &str) -> Result<CaptureResult>;
}

pub mod jobs;
pub mod storage;
pub mod types;

pub use jobs::*;
pub use storage::*;
pub use types::*;

/// Job scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum number of concurrent jobs
    pub max_concurrent_jobs: usize,
    /// Job timeout in seconds
    pub job_timeout_seconds: u64,
    /// Enable job persistence to database
    pub enable_persistence: bool,
    /// Job history retention days
    pub history_retention_days: u32,
    /// Enable job metrics collection
    pub enable_metrics: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 10,
            job_timeout_seconds: 300, // 5 minutes
            enable_persistence: true,
            history_retention_days: 30,
            enable_metrics: true,
        }
    }
}

/// Job execution status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is scheduled but not yet started
    Pending,
    /// Job is currently running
    Running,
    /// Job completed successfully
    Completed,
    /// Job failed with an error
    Failed,
    /// Job was cancelled
    Cancelled,
    /// Job timed out
    TimedOut,
    /// Job was retried
    Retried,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Retried => "retried",
        }
    }
}

/// Job execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// Job execution status
    pub status: JobStatus,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time (if completed)
    pub completed_at: Option<DateTime<Utc>>,
    /// Execution duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Result data (success output)
    pub output: Option<serde_json::Value>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Retry attempt number
    pub retry_count: u32,
}

impl Default for JobResult {
    fn default() -> Self {
        Self::new()
    }
}

impl JobResult {
    pub fn new() -> Self {
        Self {
            status: JobStatus::Pending,
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            output: None,
            error: None,
            retry_count: 0,
        }
    }

    pub fn with_success(mut self, output: serde_json::Value) -> Self {
        let now = Utc::now();
        self.status = JobStatus::Completed;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
        self.output = Some(output);
        self
    }

    pub fn with_error(mut self, error: String) -> Self {
        let now = Utc::now();
        self.status = JobStatus::Failed;
        self.completed_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds() as u64);
        self.error = Some(error);
        self
    }
}

/// Job execution context passed to job handlers
#[derive(Clone)]
pub struct JobContext {
    /// Unique job execution ID
    pub execution_id: String,
    /// Job definition ID
    pub job_id: String,
    /// Job parameters
    pub parameters: serde_json::Value,
    /// Execution metadata
    pub metadata: HashMap<String, String>,
    /// Cancellation channel receiver
    pub cancellation_token: tokio_util::sync::CancellationToken,
    /// Database connection
    pub db: Db,
    /// Capture service for taking snapshots
    pub capture_service: Arc<dyn CaptureService>,
}

impl JobContext {
    pub fn new(
        job_id: String,
        parameters: serde_json::Value,
        db: Db,
        capture_service: Arc<dyn CaptureService>,
    ) -> Self {
        Self {
            execution_id: Id::new().to_string(),
            job_id,
            parameters,
            metadata: HashMap::new(),
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            db,
            capture_service,
        }
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Running job handle with cancellation token
struct RunningJob {
    handle: tokio::task::JoinHandle<()>,
    cancellation_token: tokio_util::sync::CancellationToken,
}

/// Main job scheduler
pub struct JobScheduler {
    config: SchedulerConfig,
    cron_scheduler: TokioCronScheduler,
    job_storage: Arc<dyn JobStorage>,
    running_jobs: Arc<RwLock<HashMap<String, RunningJob>>>,
    job_handlers: Arc<RwLock<HashMap<String, Arc<dyn JobHandler>>>>,
    metrics: JobMetrics,
    db: Db,
    capture_service: Arc<dyn CaptureService>,
}

impl JobScheduler {
    /// Create a new job scheduler
    pub async fn new(
        config: SchedulerConfig,
        storage: Arc<dyn JobStorage>,
        db: Db,
        capture_service: Arc<dyn CaptureService>,
    ) -> Result<Self> {
        let cron_scheduler = TokioCronScheduler::new().await.map_err(|e| {
            gl_core::Error::Config(format!("Failed to create cron scheduler: {}", e))
        })?;

        info!("Job scheduler initialized with config: {:?}", config);

        Ok(Self {
            config,
            cron_scheduler,
            job_storage: storage,
            running_jobs: Arc::new(RwLock::new(HashMap::new())),
            job_handlers: Arc::new(RwLock::new(HashMap::new())),
            metrics: JobMetrics::new(),
            db,
            capture_service,
        })
    }

    /// Start the job scheduler
    pub async fn start(&self) -> Result<()> {
        info!("Starting job scheduler");

        self.cron_scheduler
            .start()
            .await
            .map_err(|e| gl_core::Error::Config(format!("Failed to start scheduler: {}", e)))?;

        // Load and schedule persisted jobs
        if self.config.enable_persistence {
            self.load_persisted_jobs().await?;
        }

        info!("Job scheduler started successfully");
        Ok(())
    }

    /// Stop the job scheduler
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping job scheduler");

        // Cancel all running jobs gracefully
        let running_jobs = self.running_jobs.read().await;

        // First, signal cancellation to all jobs
        for (job_id, running_job) in running_jobs.iter() {
            debug!("Cancelling running job: {}", job_id);
            running_job.cancellation_token.cancel();
        }

        // Give all jobs a grace period to clean up (wait once for all jobs)
        let job_count = running_jobs.len();
        drop(running_jobs); // Release lock during grace period

        if job_count > 0 {
            debug!("Waiting for {} jobs to clean up gracefully", job_count);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Now abort any jobs that didn't finish
        let mut running_jobs = self.running_jobs.write().await;
        for (job_id, running_job) in running_jobs.drain() {
            debug!("Force-aborting job: {}", job_id);
            running_job.handle.abort();
        }
        drop(running_jobs);

        self.cron_scheduler
            .shutdown()
            .await
            .map_err(|e| gl_core::Error::Config(format!("Failed to stop scheduler: {}", e)))?;

        info!("Job scheduler stopped");
        Ok(())
    }

    /// Register a job handler
    pub async fn register_handler(&self, job_type: String, handler: Arc<dyn JobHandler>) {
        let mut handlers = self.job_handlers.write().await;
        handlers.insert(job_type.clone(), handler);
        info!("Registered job handler for type: {}", job_type);
    }

    /// Schedule a one-time job
    pub async fn schedule_once(&self, job_def: JobDefinition) -> Result<String> {
        info!(
            "Scheduling one-time job: {} (executing immediately for now)",
            job_def.name
        );

        if self.config.enable_persistence {
            self.job_storage.save_job(&job_def).await?;
        }

        // For now, just execute immediately until we resolve the lifetime issues
        // TODO: Implement proper cron-based scheduling
        let execution_id = self.execute_now(job_def).await?;
        Ok(execution_id)
    }

    /// Schedule a recurring job
    pub async fn schedule_recurring(&self, job_def: JobDefinition) -> Result<String> {
        info!(
            "Scheduling recurring job: {} (executing immediately for now)",
            job_def.name
        );

        if self.config.enable_persistence {
            self.job_storage.save_job(&job_def).await?;
        }

        // For now, just execute immediately until we resolve the lifetime issues
        // TODO: Implement proper cron-based recurring scheduling
        let execution_id = self.execute_now(job_def).await?;
        Ok(execution_id)
    }

    /// Execute a job immediately
    pub async fn execute_now(&self, job_def: JobDefinition) -> Result<String> {
        info!("Executing job immediately: {}", job_def.name);

        let execution_id = Id::new().to_string();
        let execution_id_for_task = execution_id.clone();
        let job_id = job_def.id.clone();

        // Check if we have a handler for this job type
        let handlers = self.job_handlers.read().await;
        let handler = handlers.get(&job_def.job_type).cloned();
        drop(handlers);

        let handler = handler.ok_or_else(|| {
            gl_core::Error::NotFound(format!(
                "No handler registered for job type: {}",
                job_def.job_type
            ))
        })?;

        // Create job context
        let context = JobContext::new(
            job_id.clone(),
            job_def.parameters.clone(),
            self.db.clone(),
            self.capture_service.clone(),
        );

        // Clone cancellation token for timeout handling and tracking
        let cancellation_token = context.cancellation_token.clone();
        let cancellation_token_for_task = cancellation_token.clone();

        // Execute in background task
        let job_storage = self.job_storage.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        let running_jobs = self.running_jobs.clone();

        let handle = tokio::spawn(async move {
            let mut result = JobResult::new();
            result.status = JobStatus::Running;

            if config.enable_persistence {
                let _ = job_storage
                    .save_job_result(&execution_id_for_task, &result)
                    .await;
            }

            // Spawn a watchdog task to handle timeout
            let timeout_token = cancellation_token_for_task.clone();
            let timeout_seconds = config.job_timeout_seconds;
            let execution_id_for_timeout = execution_id_for_task.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(timeout_seconds)).await;
                if !timeout_token.is_cancelled() {
                    debug!(
                        "Job {} timeout watchdog firing, signaling cancellation",
                        execution_id_for_timeout
                    );
                    timeout_token.cancel();
                }
            });

            // Execute the job (it should check cancellation_token regularly)
            let execution_result = handler.execute(context).await;

            match execution_result {
                Ok(output) => {
                    if cancellation_token_for_task.is_cancelled() {
                        // Job completed but was cancelled/timed out
                        warn!(
                            "Job {} completed after cancellation/timeout",
                            execution_id_for_task
                        );
                        result.status = JobStatus::TimedOut;
                        result.error = Some("Job execution timed out".to_string());
                        result.completed_at = Some(Utc::now());
                        metrics
                            .jobs_failed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else {
                        // Job completed successfully - cancel token to stop watchdog early
                        cancellation_token_for_task.cancel();
                        result = result.with_success(output);
                        metrics
                            .jobs_completed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    // Job failed - ensure token is cancelled to stop watchdog
                    cancellation_token_for_task.cancel();

                    // Check if it's a cancellation error
                    if matches!(e, gl_core::Error::Cancelled(_)) {
                        result.status = JobStatus::TimedOut;
                        result.error = Some("Job was cancelled/timed out".to_string());
                        result.completed_at = Some(Utc::now());
                    } else {
                        result = result.with_error(e.to_string());
                    }
                    metrics
                        .jobs_failed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }

            if config.enable_persistence {
                let _ = job_storage
                    .save_job_result(&execution_id_for_task, &result)
                    .await;
            }

            // Remove from running jobs
            running_jobs.write().await.remove(&execution_id_for_task);

            info!(
                "Job {} completed with status: {:?}",
                execution_id_for_task, result.status
            );
        });

        // Track running job with cancellation token
        let running_job = RunningJob {
            handle,
            cancellation_token: cancellation_token.clone(),
        };
        self.running_jobs
            .write()
            .await
            .insert(execution_id.clone(), running_job);

        Ok(execution_id)
    }

    /// Get job metrics
    pub fn get_metrics(&self) -> JobMetrics {
        self.metrics.clone()
    }

    /// Get job execution history
    pub async fn get_job_history(
        &self,
        job_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<JobResult>> {
        self.job_storage.get_job_results(job_id, limit).await
    }

    /// Cancel a running job
    pub async fn cancel_job(&self, execution_id: &str) -> Result<()> {
        let mut running_jobs = self.running_jobs.write().await;
        if let Some(running_job) = running_jobs.remove(execution_id) {
            // Signal cancellation to allow job to clean up resources
            running_job.cancellation_token.cancel();

            // Give job a grace period to clean up
            drop(running_jobs); // Release lock during grace period
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            // Force abort if still running
            running_job.handle.abort();
            info!("Cancelled job: {}", execution_id);

            if self.config.enable_persistence {
                let mut result = JobResult::new();
                result.status = JobStatus::Cancelled;
                result.completed_at = Some(Utc::now());
                self.job_storage
                    .save_job_result(execution_id, &result)
                    .await?;
            }

            Ok(())
        } else {
            Err(gl_core::Error::NotFound(format!(
                "Job not found: {}",
                execution_id
            )))
        }
    }

    /// Load persisted jobs from storage
    async fn load_persisted_jobs(&self) -> Result<()> {
        debug!("Loading persisted jobs from storage");

        let jobs = self.job_storage.list_jobs().await?;
        info!("Found {} persisted jobs", jobs.len());

        for job_def in jobs {
            if job_def.enabled {
                match self.schedule_recurring(job_def.clone()).await {
                    Ok(_) => debug!("Restored job: {}", job_def.name),
                    Err(e) => warn!("Failed to restore job {}: {}", job_def.name, e),
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Mock capture service for testing
    struct MockCaptureService;

    #[async_trait]
    impl CaptureService for MockCaptureService {
        async fn capture(&self, _stream_id: &str) -> Result<CaptureResult> {
            Err(gl_core::Error::External("Mock capture".into()))
        }
    }

    // Mock job storage for testing
    struct MockJobStorage;

    #[async_trait]
    impl JobStorage for MockJobStorage {
        async fn save_job(&self, _job: &JobDefinition) -> Result<()> {
            Ok(())
        }

        async fn get_job(&self, _id: &str) -> Result<Option<JobDefinition>> {
            Ok(None)
        }

        async fn list_jobs(&self) -> Result<Vec<JobDefinition>> {
            Ok(vec![])
        }

        async fn list_jobs_filtered(
            &self,
            _enabled_only: bool,
            _job_type: Option<&str>,
            _tags: Option<&[String]>,
            _limit: Option<u32>,
        ) -> Result<Vec<JobDefinition>> {
            Ok(vec![])
        }

        async fn update_job(&self, _job: &JobDefinition) -> Result<()> {
            Ok(())
        }

        async fn delete_job(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn save_job_result(&self, _execution_id: &str, _result: &JobResult) -> Result<()> {
            Ok(())
        }

        async fn get_job_result(&self, _execution_id: &str) -> Result<Option<JobResult>> {
            Ok(None)
        }

        async fn get_job_results(
            &self,
            _job_id: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<JobResult>> {
            Ok(vec![])
        }

        async fn get_queue_stats(&self) -> Result<JobQueueStats> {
            Ok(JobQueueStats {
                pending_jobs: 0,
                running_jobs: 0,
                completed_today: 0,
                failed_today: 0,
                avg_execution_time_ms: None,
                throughput_per_hour: None,
                updated_at: chrono::Utc::now(),
            })
        }

        async fn cleanup_old_results(&self, _retention_days: u32) -> Result<u64> {
            Ok(0)
        }
    }

    // Test job that respects cancellation
    struct TestJobWithCancellation {
        cleanup_called: Arc<AtomicBool>,
    }

    #[async_trait]
    impl JobHandler for TestJobWithCancellation {
        async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
            // Simulate work with cancellation checking using tokio::select!
            for i in 0..100 {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                        debug!("Test job iteration {}", i);
                    }
                    _ = context.cancellation_token.cancelled() => {
                        // Simulate cleanup
                        info!("Job cancelled, cleaning up...");
                        self.cleanup_called.store(true, Ordering::SeqCst);
                        return Err(gl_core::Error::Cancelled("Job was cancelled".into()));
                    }
                }
            }
            Ok(serde_json::json!({"status": "completed"}))
        }

        fn job_type(&self) -> &'static str {
            "test_cancellation"
        }
    }

    // Test job that doesn't respect cancellation (bad practice)
    struct TestJobWithoutCancellation;

    #[async_trait]
    impl JobHandler for TestJobWithoutCancellation {
        async fn execute(&self, _context: JobContext) -> Result<serde_json::Value> {
            // Simulate long-running work without checking cancellation
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Ok(serde_json::json!({"status": "completed"}))
        }

        fn job_type(&self) -> &'static str {
            "test_no_cancellation"
        }
    }

    async fn create_test_scheduler() -> Result<JobScheduler> {
        let config = SchedulerConfig {
            max_concurrent_jobs: 5,
            job_timeout_seconds: 1, // Short timeout for testing
            enable_persistence: false,
            history_retention_days: 7,
            enable_metrics: true,
        };

        // Create test database
        let test_id = gl_core::Id::new().to_string();
        let db_path = format!("test_scheduler_{}.db", test_id);
        let _ = tokio::fs::remove_file(&db_path).await; // Clean up any existing
        let db = gl_db::Db::new(&db_path).await?;

        let storage = Arc::new(MockJobStorage) as Arc<dyn JobStorage>;
        let capture_service = Arc::new(MockCaptureService) as Arc<dyn CaptureService>;

        JobScheduler::new(config, storage, db, capture_service).await
    }

    #[tokio::test]
    async fn test_job_timeout_signals_cancellation() {
        let scheduler = create_test_scheduler().await.unwrap();
        let cleanup_called = Arc::new(AtomicBool::new(false));

        let handler = Arc::new(TestJobWithCancellation {
            cleanup_called: cleanup_called.clone(),
        });

        scheduler
            .register_handler("test_cancellation".to_string(), handler)
            .await;
        scheduler.start().await.unwrap();

        let job = JobDefinition::new(
            "Test Timeout".to_string(),
            "test_cancellation".to_string(),
            "0 0 * * * *".to_string(),
            serde_json::json!({}),
            "test_user".to_string(),
        );

        let execution_id = scheduler.execute_now(job).await.unwrap();

        // Wait for timeout + grace period + extra time for cleanup to execute
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Verify cleanup was called
        assert!(
            cleanup_called.load(Ordering::SeqCst),
            "Cleanup should have been called when job timed out"
        );

        // Verify job is no longer in running jobs
        let running_jobs = scheduler.running_jobs.read().await;
        assert!(
            !running_jobs.contains_key(&execution_id),
            "Job should be removed from running jobs after timeout"
        );
    }

    #[tokio::test]
    async fn test_cancel_job_signals_cancellation() {
        let scheduler = create_test_scheduler().await.unwrap();
        let cleanup_called = Arc::new(AtomicBool::new(false));

        let handler = Arc::new(TestJobWithCancellation {
            cleanup_called: cleanup_called.clone(),
        });

        scheduler
            .register_handler("test_cancellation".to_string(), handler)
            .await;
        scheduler.start().await.unwrap();

        let job = JobDefinition::new(
            "Test Cancel".to_string(),
            "test_cancellation".to_string(),
            "0 0 * * * *".to_string(),
            serde_json::json!({}),
            "test_user".to_string(),
        );

        let execution_id = scheduler.execute_now(job).await.unwrap();

        // Give job time to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Cancel the job
        scheduler.cancel_job(&execution_id).await.unwrap();

        // Wait for grace period
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Verify cleanup was called
        assert!(
            cleanup_called.load(Ordering::SeqCst),
            "Cleanup should have been called when job was cancelled"
        );
    }

    #[tokio::test]
    async fn test_scheduler_stop_signals_cancellation() {
        let mut scheduler = create_test_scheduler().await.unwrap();
        let cleanup_called = Arc::new(AtomicBool::new(false));

        let handler = Arc::new(TestJobWithCancellation {
            cleanup_called: cleanup_called.clone(),
        });

        scheduler
            .register_handler("test_cancellation".to_string(), handler)
            .await;
        scheduler.start().await.unwrap();

        let job = JobDefinition::new(
            "Test Stop".to_string(),
            "test_cancellation".to_string(),
            "0 0 * * * *".to_string(),
            serde_json::json!({}),
            "test_user".to_string(),
        );

        scheduler.execute_now(job).await.unwrap();

        // Give job time to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Stop the scheduler
        scheduler.stop().await.unwrap();

        // Verify cleanup was called
        assert!(
            cleanup_called.load(Ordering::SeqCst),
            "Cleanup should have been called when scheduler stopped"
        );
    }
}

/// Job execution metrics
#[derive(Debug)]
pub struct JobMetrics {
    pub jobs_scheduled: std::sync::atomic::AtomicU64,
    pub jobs_completed: std::sync::atomic::AtomicU64,
    pub jobs_failed: std::sync::atomic::AtomicU64,
    pub jobs_cancelled: std::sync::atomic::AtomicU64,
}

impl Clone for JobMetrics {
    fn clone(&self) -> Self {
        Self {
            jobs_scheduled: std::sync::atomic::AtomicU64::new(
                self.jobs_scheduled
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            jobs_completed: std::sync::atomic::AtomicU64::new(
                self.jobs_completed
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            jobs_failed: std::sync::atomic::AtomicU64::new(
                self.jobs_failed.load(std::sync::atomic::Ordering::Relaxed),
            ),
            jobs_cancelled: std::sync::atomic::AtomicU64::new(
                self.jobs_cancelled
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl Default for JobMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl JobMetrics {
    pub fn new() -> Self {
        Self {
            jobs_scheduled: std::sync::atomic::AtomicU64::new(0),
            jobs_completed: std::sync::atomic::AtomicU64::new(0),
            jobs_failed: std::sync::atomic::AtomicU64::new(0),
            jobs_cancelled: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn get_scheduled(&self) -> u64 {
        self.jobs_scheduled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_completed(&self) -> u64 {
        self.jobs_completed
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_failed(&self) -> u64 {
        self.jobs_failed.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn get_cancelled(&self) -> u64 {
        self.jobs_cancelled
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
