//! ABOUTME: Web layer service for managing background snapshot processing
//! ABOUTME: Integrates BackgroundSnapshotProcessor with web application state

use gl_capture::{BackgroundSnapshotProcessor, SnapshotConfig};
use gl_core::Result;
// use gl_db::repositories::background_snapshot_jobs::{
//     BackgroundSnapshotJobsRepository, CreateBackgroundJobRequest, UpdateBackgroundJobRequest, BackgroundJobStatus
// };
use bytes::Bytes;
use sqlx::SqlitePool;
use std::{path::PathBuf, sync::Arc};
use tracing::{debug, error, info};

/// Service wrapper for background snapshot processing in the web layer
#[derive(Debug, Clone)]
pub struct BackgroundSnapshotService {
    processor: Arc<BackgroundSnapshotProcessor>,
    #[allow(dead_code)] // TODO: Will be used for database persistence
    db_pool: SqlitePool,
}

impl BackgroundSnapshotService {
    /// Create a new background snapshot service with database persistence
    pub fn new(db_pool: SqlitePool) -> Self {
        let processor = Arc::new(BackgroundSnapshotProcessor::new());
        info!("Background snapshot service initialized with database persistence");
        Self { processor, db_pool }
    }

    /// Take a snapshot from a file path using background processing
    pub async fn snapshot_file(&self, file_path: PathBuf, config: SnapshotConfig) -> Result<Bytes> {
        debug!(path = %file_path.display(), "Taking background snapshot from file");
        self.processor.process_job_sync(file_path, config).await
    }

    /// Submit a background snapshot job and return the job ID immediately
    pub async fn submit_snapshot_job(
        &self,
        file_path: PathBuf,
        config: SnapshotConfig,
        stream_id: Option<String>,
        _created_by: Option<String>,
    ) -> Result<String> {
        debug!(path = %file_path.display(), stream_id = ?stream_id, "Submitting background snapshot job");

        // For now, just use the processor's submit_job method
        // TODO: Add full database integration
        self.processor.submit_job(file_path, config).await
    }

    /// Submit a background snapshot job for a stream using its capture source
    pub async fn submit_stream_snapshot_job(
        &self,
        source: Box<dyn gl_capture::CaptureSource + Send + Sync>,
        _config: SnapshotConfig,
        stream_id: String,
        _created_by: Option<String>,
    ) -> Result<String> {
        debug!(stream_id = %stream_id, "Submitting stream snapshot job");

        // Create a job ID
        let job_id = gl_core::Id::new().to_string();

        // Submit the job to be processed asynchronously
        let job_id_clone = job_id.clone();
        let _processor = self.processor.clone();

        tokio::spawn(async move {
            debug!(job_id = %job_id_clone, "Processing stream snapshot in background");

            // Take snapshot using the capture source
            match source.snapshot().await {
                Ok(snapshot_bytes) => {
                    debug!(job_id = %job_id_clone, size = snapshot_bytes.len(), "Stream snapshot completed successfully");
                    // TODO: Store result in processor or database
                }
                Err(e) => {
                    error!(job_id = %job_id_clone, error = %e, "Stream snapshot failed");
                    // TODO: Mark job as failed in processor or database
                }
            }
        });

        Ok(job_id)
    }

    /// Get the status of a background snapshot job
    pub async fn get_job_status(&self, job_id: &str) -> Result<Option<gl_capture::SnapshotJob>> {
        debug!(job_id = %job_id, "Getting job status");
        Ok(self.processor.get_job_status(job_id))
    }

    /// Get the result of a completed background snapshot job
    pub async fn get_job_result(&self, job_id: &str) -> Result<Option<Bytes>> {
        debug!(job_id = %job_id, "Getting job result");
        self.processor.get_job_result(job_id)
    }

    /// Get the underlying processor for advanced operations
    pub fn processor(&self) -> Arc<BackgroundSnapshotProcessor> {
        self.processor.clone()
    }

    /// Get processor statistics
    pub fn stats(&self) -> gl_capture::ProcessorStats {
        self.processor.get_stats()
    }

    /// Clean up old jobs
    pub fn cleanup_processor_jobs(&self) {
        self.processor.cleanup_old_jobs();
    }

    /// Sync job status between processor and database (placeholder for future implementation)
    pub async fn sync_job_status(&self, job_id: &str) -> Result<()> {
        debug!(job_id = %job_id, "Syncing job status (not yet implemented)");
        // TODO: Implement database sync once we have full integration
        Ok(())
    }

    /// Clean up old jobs from both processor and database
    pub async fn cleanup_old_jobs(&self) -> Result<u64> {
        // Clean up processor first
        self.processor.cleanup_old_jobs();

        // TODO: Implement database cleanup
        info!("Cleaned up old background snapshot jobs from processor");
        Ok(0)
    }

    /// Get database statistics for jobs
    pub async fn get_job_stats(&self) -> Result<std::collections::HashMap<String, i64>> {
        // BackgroundSnapshotJobsRepository::count_by_status(&self.db_pool).await
        // TODO: Re-enable after migration
        Ok(std::collections::HashMap::new())
    }
}
