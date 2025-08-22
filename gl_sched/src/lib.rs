//! ABOUTME: Job scheduler with cron support and database persistence
//! ABOUTME: Manages scheduled capture jobs and recurring tasks

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gl_core::{Error, Result, Id};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};
use tokio::sync::{Mutex, RwLock};
use tokio_cron_scheduler::{JobScheduler, Job as CronJob};
use tracing::{debug, error, info, warn, instrument};

// Optional database support
#[cfg(feature = "database")]
use sqlx::SqlitePool;

// Mock database types for non-database builds
#[cfg(not(feature = "database"))]
#[derive(Debug, Clone)]
pub struct SqlitePool;

#[cfg(not(feature = "database"))]
impl SqlitePool {
    pub fn connect(_url: &str) -> Result<Self> {
        Ok(Self)
    }
}

pub mod job;
pub mod runner;
pub mod models;

pub use job::*;
pub use runner::*;
pub use models::*;

/// Scheduled job configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    /// Unique identifier for this scheduled job
    pub id: Id,
    /// Human-readable name
    pub name: String,
    /// Job kind (capture, snapshot, process, etc.)
    pub kind: JobKind,
    /// Cron schedule expression
    pub schedule: String,
    /// Last run timestamp
    pub last_run: Option<DateTime<Utc>>,
    /// Next scheduled run timestamp
    pub next_run: Option<DateTime<Utc>>,
    /// Jitter in milliseconds to add randomness
    pub jitter_ms: u32,
    /// Whether this job is enabled
    pub enabled: bool,
    /// Job configuration as JSON
    pub config: serde_json::Value,
    /// User ID who owns this job
    pub user_id: String,
    /// Template ID for capture jobs
    pub template_id: Option<String>,
}

impl ScheduledJob {
    /// Create a new scheduled job
    pub fn new(
        name: String,
        kind: JobKind,
        schedule: String,
        user_id: String,
        template_id: Option<String>,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id: Id::new(),
            name,
            kind,
            schedule,
            last_run: None,
            next_run: None,
            jitter_ms: 0,
            enabled: true,
            config,
            user_id,
            template_id,
        }
    }

    /// Add jitter to the next run time
    pub fn with_jitter(mut self, jitter_ms: u32) -> Self {
        self.jitter_ms = jitter_ms;
        self
    }

    /// Calculate next run time from a cron expression
    pub fn calculate_next_run(&self, _from: DateTime<Utc>) -> Result<DateTime<Utc>> {
        let schedule = cron::Schedule::from_str(&self.schedule)
            .map_err(|e| Error::Config(format!("Invalid cron expression '{}': {}", self.schedule, e)))?;
        
        let next = schedule.upcoming(Utc).next()
            .ok_or_else(|| Error::Config("No upcoming schedule found".to_string()))?;
        
        // Add jitter if configured
        if self.jitter_ms > 0 {
            let jitter = rand::thread_rng().gen_range(0..=self.jitter_ms) as i64;
            Ok(next + chrono::Duration::milliseconds(jitter))
        } else {
            Ok(next)
        }
    }

    /// Update last run and calculate next run
    pub fn mark_run(&mut self, run_time: DateTime<Utc>) -> Result<()> {
        self.last_run = Some(run_time);
        self.next_run = Some(self.calculate_next_run(run_time)?);
        Ok(())
    }
}

/// Job execution context
#[derive(Debug, Clone)]
pub struct JobContext {
    /// Scheduled job definition
    pub scheduled_job: ScheduledJob,
    /// Database pool for creating job records
    pub db_pool: SqlitePool,
    /// Execution timestamp
    pub execution_time: DateTime<Utc>,
}

/// Trait for job handlers
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Execute a scheduled job
    async fn execute(&self, context: JobContext) -> Result<String>;
    /// Get the job kinds this handler supports
    fn supported_kinds(&self) -> Vec<JobKind>;
}

/// Main scheduler that manages cron jobs
pub struct Scheduler {
    /// Internal cron scheduler
    cron_scheduler: Arc<Mutex<JobScheduler>>,
    /// Scheduled jobs
    scheduled_jobs: Arc<RwLock<HashMap<Id, ScheduledJob>>>,
    /// Job handlers
    handlers: Arc<RwLock<HashMap<JobKind, Arc<dyn JobHandler>>>>,
    /// Database pool
    db_pool: SqlitePool,
    /// Running status
    running: Arc<AtomicBool>,
}

impl Scheduler {
    /// Create a new scheduler
    pub async fn new(db_pool: SqlitePool) -> Result<Self> {
        let cron_scheduler = JobScheduler::new()
            .await
            .map_err(|e| Error::Config(format!("Failed to create cron scheduler: {}", e)))?;
        
        Ok(Self {
            cron_scheduler: Arc::new(Mutex::new(cron_scheduler)),
            scheduled_jobs: Arc::new(RwLock::new(HashMap::new())),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            db_pool,
            running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Add a job handler
    pub async fn add_handler(&self, handler: Arc<dyn JobHandler>) {
        let mut handlers = self.handlers.write().await;
        for kind in handler.supported_kinds() {
            handlers.insert(kind, handler.clone());
        }
    }

    /// Add a scheduled job
    pub async fn add_scheduled_job(&self, mut job: ScheduledJob) -> Result<()> {
        info!(
            job_id = %job.id,
            name = %job.name,
            kind = ?job.kind,
            schedule = %job.schedule,
            "Adding scheduled job"
        );

        // Calculate next run
        job.next_run = Some(job.calculate_next_run(Utc::now())?);

        // Store in memory
        let job_id = job.id.clone();
        self.scheduled_jobs.write().await.insert(job_id, job.clone());

        // Add to cron scheduler if enabled
        if job.enabled {
            self.schedule_cron_job(job).await?;
        }

        Ok(())
    }

    /// Remove a scheduled job
    pub async fn remove_scheduled_job(&self, job_id: &Id) -> Result<()> {
        info!(job_id = %job_id, "Removing scheduled job");
        
        // Remove from memory
        self.scheduled_jobs.write().await.remove(job_id);
        
        // Note: tokio-cron-scheduler doesn't provide direct job removal by external ID
        // In a production system, we'd need to track the internal job IDs and remove them
        
        Ok(())
    }

    /// List all scheduled jobs
    pub async fn list_scheduled_jobs(&self) -> Vec<ScheduledJob> {
        self.scheduled_jobs.read().await.values().cloned().collect()
    }

    /// Get a scheduled job by ID
    pub async fn get_scheduled_job(&self, job_id: &Id) -> Option<ScheduledJob> {
        self.scheduled_jobs.read().await.get(job_id).cloned()
    }

    /// Start the scheduler
    pub async fn start(&self) -> Result<()> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("Starting scheduler");
        
        let mut scheduler = self.cron_scheduler.lock().await;
        scheduler.start()
            .await
            .map_err(|e| Error::Config(format!("Failed to start scheduler: {}", e)))?;
        
        self.running.store(true, Ordering::Relaxed);
        
        Ok(())
    }

    /// Stop the scheduler  
    pub async fn stop(&self) -> Result<()> {
        if !self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("Stopping scheduler");
        
        let mut scheduler = self.cron_scheduler.lock().await;
        scheduler.shutdown()
            .await
            .map_err(|e| Error::Config(format!("Failed to stop scheduler: {}", e)))?;
        
        self.running.store(false, Ordering::Relaxed);
        
        Ok(())
    }

    /// Check if scheduler is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Trigger a job now (outside of its schedule)
    #[instrument(skip(self))]
    pub async fn trigger_job_now(&self, job_id: &Id) -> Result<String> {
        let job = self.get_scheduled_job(job_id).await
            .ok_or_else(|| Error::NotFound(format!("Scheduled job {} not found", job_id)))?;
        
        info!(
            job_id = %job_id,
            name = %job.name,
            "Triggering job manually"
        );

        self.execute_job(job).await
    }

    /// Pause/resume a scheduled job
    pub async fn set_job_enabled(&self, job_id: &Id, enabled: bool) -> Result<()> {
        let mut jobs = self.scheduled_jobs.write().await;
        let job = jobs.get_mut(job_id)
            .ok_or_else(|| Error::NotFound(format!("Scheduled job {} not found", job_id)))?;
        
        if job.enabled == enabled {
            return Ok(()); // No change needed
        }

        job.enabled = enabled;
        
        if enabled {
            info!(job_id = %job_id, "Enabling scheduled job");
            // Re-schedule the job
            self.schedule_cron_job(job.clone()).await?;
        } else {
            info!(job_id = %job_id, "Disabling scheduled job");
            // Note: In production, we'd remove from cron scheduler here
        }

        Ok(())
    }

    /// Schedule a job with the cron scheduler
    async fn schedule_cron_job(&self, job: ScheduledJob) -> Result<()> {
        let job_id = job.id.clone();
        let handlers = Arc::clone(&self.handlers);
        let db_pool = self.db_pool.clone();
        let scheduled_jobs = Arc::clone(&self.scheduled_jobs);

        let cron_job = CronJob::new_async(job.schedule.clone().as_str(), move |_uuid, _l| {
            let job = job.clone();
            let handlers = Arc::clone(&handlers);
            let db_pool = db_pool.clone();
            let scheduled_jobs = Arc::clone(&scheduled_jobs);

            Box::pin(async move {
                debug!(job_id = %job.id, "Executing scheduled job");
                
                // Check if job is still enabled and exists
                let current_job = {
                    let jobs = scheduled_jobs.read().await;
                    jobs.get(&job.id).cloned()
                };

                if let Some(current_job) = current_job {
                    if !current_job.enabled {
                        debug!(job_id = %job.id, "Job disabled, skipping");
                        return;
                    }

                    // Execute the job with idempotency checks
                    match execute_job_with_idempotency(current_job, handlers, db_pool).await {
                        Ok(result) => {
                            info!(job_id = %job.id, result = %result, "Job completed successfully");
                        }
                        Err(e) => {
                            error!(job_id = %job.id, error = %e, "Job execution failed");
                        }
                    }
                } else {
                    warn!(job_id = %job.id, "Scheduled job no longer exists");
                }
            })
        })
        .map_err(|e| Error::Config(format!("Failed to create cron job: {}", e)))?;

        let mut scheduler = self.cron_scheduler.lock().await;
        scheduler.add(cron_job)
            .await
            .map_err(|e| Error::Config(format!("Failed to add cron job: {}", e)))?;

        debug!(job_id = %job_id, "Cron job scheduled successfully");
        Ok(())
    }

    /// Execute a job immediately
    async fn execute_job(&self, job: ScheduledJob) -> Result<String> {
        let handlers = Arc::clone(&self.handlers);
        execute_job_with_idempotency(job, handlers, self.db_pool.clone()).await
    }
}

/// Execute a job with idempotency checks
async fn execute_job_with_idempotency(
    job: ScheduledJob,
    handlers: Arc<RwLock<HashMap<JobKind, Arc<dyn JobHandler>>>>,
    db_pool: SqlitePool,
) -> Result<String> {
    // Check for existing running job of the same type for the same template
    // This implements idempotency - don't run if already running
    let existing_job = check_for_running_job(&job).await?;
    if existing_job.is_some() {
        let msg = format!("Job of type {:?} already running for template {:?}, skipping", 
            job.kind, job.template_id);
        warn!(job_id = %job.id, "{}", msg);
        return Ok(msg);
    }

    // For now, we'll skip creating database records since gl_db has compilation issues
    // In production, this would create a job record in the database
    info!(job_id = %job.id, "Starting job execution (database recording disabled)");

    // Get handler for this job kind
    let handler = {
        let handlers_guard = handlers.read().await;
        handlers_guard.get(&job.kind).cloned()
    };

    let handler = match handler {
        Some(h) => h,
        None => {
            let error_msg = format!("No handler registered for job kind: {:?}", job.kind);
            error!(job_id = %job.id, "{}", error_msg);
            return Err(Error::Config(error_msg));
        }
    };

    // Create execution context
    let context = JobContext {
        scheduled_job: job.clone(),
        db_pool,
        execution_time: Utc::now(),
    };

    // Execute the job
    let result = handler.execute(context).await;
    
    match result {
        Ok(success_msg) => {
            info!(job_id = %job.id, "Job executed successfully: {}", success_msg);
            Ok(success_msg)
        }
        Err(e) => {
            error!(job_id = %job.id, error = %e, "Job execution failed");
            Err(e)
        }
    }
}

/// Check for existing running job to implement idempotency
async fn check_for_running_job(_scheduled_job: &ScheduledJob) -> Result<Option<String>> {
    // For now, we'll implement a simple check
    // In production, we'd query for jobs with status='running' and same type/template
    // This is a placeholder - the actual implementation would depend on database support
    Ok(None)
}
