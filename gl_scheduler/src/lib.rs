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

pub mod distributed_lock;
pub mod jobs;
pub mod storage;
pub mod types;

pub use distributed_lock::*;
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
    /// Enable distributed locking (prevents duplicate execution across instances)
    pub enable_distributed_locking: bool,
    /// Distributed lock lease duration in seconds
    pub lock_lease_seconds: i64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_jobs: 10,
            job_timeout_seconds: 300, // 5 minutes
            enable_persistence: true,
            history_retention_days: 30,
            enable_metrics: true,
            enable_distributed_locking: true, // Enable by default for safety
            lock_lease_seconds: 360,          // 6 minutes (job timeout + buffer)
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

/// Main job scheduler
pub struct JobScheduler {
    config: SchedulerConfig,
    cron_scheduler: TokioCronScheduler,
    job_storage: Arc<dyn JobStorage>,
    running_jobs: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    job_handlers: Arc<RwLock<HashMap<String, Arc<dyn JobHandler>>>>,
    metrics: JobMetrics,
    db: Db,
    capture_service: Arc<dyn CaptureService>,
    lock_manager: Option<Arc<DistributedLockManager>>,
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

        // Initialize distributed lock manager if enabled
        let lock_manager = if config.enable_distributed_locking {
            let lock_config = LockConfig {
                default_lease_seconds: config.lock_lease_seconds,
                enable_cleanup: true,
                cleanup_interval_seconds: 60,
            };
            let manager = DistributedLockManager::new(db.pool().clone(), lock_config);
            info!(
                "Distributed locking enabled for instance: {}",
                manager.instance_id()
            );
            Some(Arc::new(manager))
        } else {
            warn!("Distributed locking is DISABLED - not safe for multi-instance deployments!");
            None
        };

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
            lock_manager,
        })
    }

    /// Start the job scheduler
    pub async fn start(&self) -> Result<()> {
        info!("Starting job scheduler");

        self.cron_scheduler
            .start()
            .await
            .map_err(|e| gl_core::Error::Config(format!("Failed to start scheduler: {}", e)))?;

        // Start lock cleanup task if distributed locking is enabled
        if let Some(ref lock_manager) = self.lock_manager {
            let lock_manager_clone = lock_manager.clone();
            let retention_days = self.config.history_retention_days;

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // Run every minute
                let mut cleanup_counter: u32 = 0;

                loop {
                    interval.tick().await;

                    // Expire stale locks (locks that have passed their lease)
                    if let Err(e) = lock_manager_clone.expire_stale_locks().await {
                        warn!("Failed to expire stale locks: {}", e);
                    }

                    // Cleanup old locks (every hour)
                    cleanup_counter += 1;
                    if cleanup_counter >= 60 {
                        if let Err(e) = lock_manager_clone.cleanup_old_locks(retention_days).await {
                            warn!("Failed to cleanup old locks: {}", e);
                        }
                        cleanup_counter = 0;
                    }
                }
            });

            info!("Lock cleanup task started");
        }

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

        // Cancel all running jobs
        let running_jobs = self.running_jobs.read().await;
        for (job_id, handle) in running_jobs.iter() {
            debug!("Cancelling running job: {}", job_id);
            handle.abort();
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

        // Try to acquire distributed lock if enabled
        let lock = if let Some(ref lock_manager) = self.lock_manager {
            match lock_manager
                .try_acquire_lock(&job_id, &execution_id, Some(self.config.lock_lease_seconds))
                .await?
            {
                Some(lock) => {
                    info!(
                        "Acquired lock for job {} (execution {})",
                        job_id, execution_id
                    );
                    Some(lock)
                }
                None => {
                    info!(
                        "Job {} is already locked by another instance, skipping execution",
                        job_id
                    );
                    return Err(gl_core::Error::External(format!(
                        "Job {} is currently being executed by another instance",
                        job_id
                    )));
                }
            }
        } else {
            None
        };

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

        // Execute in background task
        let job_storage = self.job_storage.clone();
        let config = self.config.clone();
        let metrics = self.metrics.clone();
        let running_jobs = self.running_jobs.clone();
        let lock_manager = self.lock_manager.clone();

        let handle = tokio::spawn(async move {
            let mut result = JobResult::new();
            result.status = JobStatus::Running;

            if config.enable_persistence {
                let _ = job_storage
                    .save_job_result(&job_id, &execution_id_for_task, &result)
                    .await;
            }

            // Execute the job with timeout
            let execution_result = tokio::time::timeout(
                std::time::Duration::from_secs(config.job_timeout_seconds),
                handler.execute(context),
            )
            .await;

            match execution_result {
                Ok(Ok(output)) => {
                    result = result.with_success(output);
                    metrics
                        .jobs_completed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Ok(Err(e)) => {
                    result = result.with_error(e.to_string());
                    metrics
                        .jobs_failed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Err(_) => {
                    result.status = JobStatus::TimedOut;
                    result.error = Some("Job execution timed out".to_string());
                    result.completed_at = Some(Utc::now());
                    metrics
                        .jobs_failed
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }

            if config.enable_persistence {
                let _ = job_storage
                    .save_job_result(&job_id, &execution_id_for_task, &result)
                    .await;
            }

            // Release the distributed lock if we acquired it
            if let Some(ref lock) = lock {
                if let Some(ref manager) = lock_manager {
                    if let Err(e) = manager.release_lock(&lock.id).await {
                        warn!("Failed to release lock for job {}: {}", lock.job_id, e);
                    } else {
                        info!("Released lock for job {}", lock.job_id);
                    }
                }
            }

            // Remove from running jobs
            running_jobs.write().await.remove(&execution_id_for_task);

            info!(
                "Job {} completed with status: {:?}",
                execution_id_for_task, result.status
            );
        });

        // Track running job
        self.running_jobs
            .write()
            .await
            .insert(execution_id.clone(), handle);

        Ok(execution_id)
    }

    /// Get job metrics
    pub fn get_metrics(&self) -> JobMetrics {
        self.metrics.clone()
    }

    /// Get distributed lock statistics
    pub async fn get_lock_stats(&self) -> Result<Option<LockStats>> {
        if let Some(ref lock_manager) = self.lock_manager {
            Ok(Some(lock_manager.get_lock_stats().await?))
        } else {
            Ok(None)
        }
    }

    /// Get the instance ID (useful for debugging multi-instance deployments)
    pub fn get_instance_id(&self) -> Option<String> {
        self.lock_manager
            .as_ref()
            .map(|lm| lm.instance_id().to_string())
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
        if let Some(handle) = running_jobs.remove(execution_id) {
            handle.abort();
            info!("Cancelled job: {}", execution_id);

            if self.config.enable_persistence {
                let mut result = JobResult::new();
                result.status = JobStatus::Cancelled;
                result.completed_at = Some(Utc::now());
                self.job_storage
                    .save_job_result("unknown", execution_id, &result)
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
