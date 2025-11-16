//! ABOUTME: Job handler definitions for different types of scheduled tasks
//! ABOUTME: Includes snapshot jobs with perceptual hash deduplication

use crate::JobContext;
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::Result;
use img_hash::{HashAlg, HasherConfig, ImageHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Job handler trait that all job types must implement
///
/// # Resource Cleanup
///
/// Job handlers MUST implement proper resource cleanup to prevent leaks when:
/// - The job times out
/// - The job is cancelled
/// - The scheduler is shutting down
///
/// ## Cleanup Guidelines
///
/// 1. **Check cancellation token regularly** during long-running operations:
///    ```rust,no_run
///    # use gl_core::Result;
///    # use gl_scheduler::JobContext;
///    # async fn example(context: JobContext) -> Result<()> {
///    if context.cancellation_token.is_cancelled() {
///        // Clean up resources (close files, connections, etc.)
///        return Err(gl_core::Error::Cancelled("Job was cancelled".into()));
///    }
///    # Ok(())
///    # }
///    ```
///
/// 2. **Use timeout-aware operations** where possible:
///    ```rust,no_run
///    # use gl_core::Result;
///    # use gl_scheduler::JobContext;
///    # async fn some_operation() -> Result<()> { Ok(()) }
///    # async fn example(context: JobContext) -> Result<()> {
///    tokio::select! {
///        result = some_operation() => {
///            result?; // Handle the result
///        },
///        _ = context.cancellation_token.cancelled() => {
///            // Clean up and return early
///            return Err(gl_core::Error::Cancelled("Job was cancelled".into()));
///        }
///    }
///    # Ok(())
///    # }
///    ```
///
/// 3. **Clean up resources in all code paths**:
///    - Use RAII patterns (Drop trait) for automatic cleanup
///    - Ensure cleanup happens on error, timeout, and cancellation
///    - Consider using `defer` patterns or scopeguard crate
///
/// 4. **Grace period**: When a timeout or cancellation occurs, the scheduler
///    provides a 500ms grace period for cleanup before forcibly aborting the task.
///
/// ## Example Implementation
///
/// ```rust,ignore
/// async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
///     // Open resource (file, connection, etc.)
///     let mut file = open_file().await?;
///
///     loop {
///         // Check for cancellation before each iteration
///         if context.cancellation_token.is_cancelled() {
///             file.close().await?;  // Clean up
///             return Err(gl_core::Error::Cancelled("Job cancelled".into()));
///         }
///
///         // Do work...
///         process_chunk(&mut file).await?;
///     }
///
///     file.close().await?;
///     Ok(serde_json::json!({"status": "success"}))
/// }
/// ```
#[async_trait]
pub trait JobHandler: Send + Sync {
    /// Execute the job with the given context
    ///
    /// # Cancellation
    ///
    /// Job implementations MUST check `context.cancellation_token.is_cancelled()`
    /// or use `tokio::select!` with `context.cancellation_token.cancelled()` to
    /// handle timeouts and cancellations gracefully.
    async fn execute(&self, context: JobContext) -> Result<serde_json::Value>;

    /// Get job type name
    fn job_type(&self) -> &'static str;

    /// Validate job parameters before execution
    fn validate_parameters(&self, parameters: &serde_json::Value) -> Result<()> {
        let _ = parameters; // Default implementation accepts any parameters
        Ok(())
    }
}

/// Smart snapshot job that uses perceptual hashing to avoid duplicate storage
pub struct SmartSnapshotJob {
    // Will hold reference to capture manager and storage
}

impl Default for SmartSnapshotJob {
    fn default() -> Self {
        Self::new()
    }
}

impl SmartSnapshotJob {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl JobHandler for SmartSnapshotJob {
    async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
        info!("Executing smart snapshot job: {}", context.job_id);

        // Check for cancellation at start
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled(
                "Job cancelled before start".into(),
            ));
        }

        // Extract parameters
        let params: SmartSnapshotParams =
            serde_json::from_value(context.parameters).map_err(|e| {
                gl_core::Error::Validation(format!("Invalid snapshot parameters: {}", e))
            })?;

        info!("Taking smart snapshot of stream: {}", params.stream_id);

        // Get threshold for hash comparison from database settings
        use gl_db::repositories::settings::SettingsRepository;
        let settings_repo = SettingsRepository::new(context.db.pool());
        let threshold = settings_repo.get_phash_threshold().await.unwrap_or(0.85); // Default to 0.85 if setting is not found

        // Check for cancellation before capture
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled(
                "Job cancelled before capture".into(),
            ));
        }

        // 1. Take screenshot using capture service
        let capture_result = match context.capture_service.capture(&params.stream_id).await {
            Ok(result) => result,
            Err(e) => {
                return Ok(serde_json::json!({
                    "stream_id": params.stream_id,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "status": "capture_failed",
                    "error": format!("Failed to capture snapshot: {}", e),
                    "hash_changed": false,
                    "image_stored": false
                }));
            }
        };

        // Check for cancellation after capture
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled(
                "Job cancelled after capture".into(),
            ));
        }

        // 2. Load image once and calculate both hash and dimensions efficiently
        let image_bytes = &capture_result.data;
        debug!(
            "Loading image for hash calculation: {} bytes, content_type: {}",
            image_bytes.len(),
            capture_result.content_type
        );

        let loaded_image =
            match img_hash::image::load_from_memory(image_bytes) {
                Ok(image) => image,
                Err(e) => {
                    warn!(
                    "Failed to load captured image for stream {}: {} (content_type: {}, {} bytes)",
                    params.stream_id, e, capture_result.content_type, image_bytes.len()
                );
                    return Ok(serde_json::json!({
                        "stream_id": params.stream_id,
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "status": "image_load_failed",
                        "error": format!("Failed to load image: {}", e),
                        "hash_changed": false,
                        "image_stored": false
                    }));
                }
            };

        let current_hash = match calculate_perceptual_hash_from_image(&loaded_image) {
            Ok(hash) => hash,
            Err(e) => {
                warn!(
                    "Failed to calculate perceptual hash for stream {}: {}",
                    params.stream_id, e
                );
                return Ok(serde_json::json!({
                    "stream_id": params.stream_id,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "status": "hash_calculation_failed",
                    "error": format!("Failed to calculate hash: {}", e),
                    "hash_changed": false,
                    "image_stored": false
                }));
            }
        };

        // 3. Query database for the most recent snapshot hash for this stream
        let previous_hash_query = sqlx::query_scalar::<_, String>(
            "SELECT perceptual_hash FROM snapshots
             WHERE stream_id = ? AND perceptual_hash IS NOT NULL
             ORDER BY captured_at DESC
             LIMIT 1",
        )
        .bind(&params.stream_id)
        .fetch_optional(context.db.pool())
        .await
        .map_err(|e| gl_core::Error::Database(format!("Failed to query previous hash: {}", e)))?;

        let mut hash_changed = true;
        let mut image_stored = false;
        let previous_hash_str = previous_hash_query.clone().unwrap_or_default();

        // 4. Compare hashes if we have a previous one
        if let Some(prev_hash) = &previous_hash_query {
            match compare_perceptual_hashes(prev_hash, &current_hash, threshold) {
                Ok(is_similar) => {
                    hash_changed = !is_similar;
                    let similarity =
                        calculate_similarity_score(prev_hash, &current_hash).unwrap_or(0.0);
                    debug!(
                        "Hash comparison: previous={}, current={}, similarity={:.3}, threshold={}, changed={}",
                        prev_hash, current_hash, similarity, threshold, hash_changed
                    );
                }
                Err(e) => {
                    warn!(
                        "Hash comparison failed: {}, treating as changed for safety",
                        e
                    );
                    hash_changed = true; // Default to changed on comparison error
                }
            }
        }

        let mut snapshot_id = None;

        // Check for cancellation before storage operation
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled(
                "Job cancelled before storage".into(),
            ));
        }

        // 5. Only store if hash changed significantly or no previous hash exists
        if hash_changed {
            // Get user_id from stream
            let user_id =
                sqlx::query_scalar::<_, String>("SELECT user_id FROM streams WHERE id = ?")
                    .bind(&params.stream_id)
                    .fetch_one(context.db.pool())
                    .await
                    .map_err(|e| {
                        gl_core::Error::Database(format!("Failed to get user_id: {}", e))
                    })?;

            // Store the file to disk since hash changed
            // Create directory per stream: ./data/artifacts/{stream_id}/
            let stream_artifacts_dir =
                std::path::PathBuf::from("./data/artifacts").join(&params.stream_id);
            std::fs::create_dir_all(&stream_artifacts_dir).map_err(|e| {
                gl_core::Error::Storage(format!("Failed to create stream directory: {}", e))
            })?;

            let gl_storage_config = gl_storage::StorageConfig {
                base_dir: Some(stream_artifacts_dir.clone()),
                ..Default::default()
            };
            let storage_manager =
                gl_storage::StorageManager::new(gl_storage_config).map_err(|e| {
                    gl_core::Error::Config(format!("Failed to create storage manager: {}", e))
                })?;

            let artifact_config = gl_capture::artifact_storage::ArtifactStorageConfig {
                base_uri: "file:///".to_string(),
                snapshot_extension: "jpg".to_string(),
                include_timestamp: true,
            };
            let storage_service = gl_capture::artifact_storage::ArtifactStorageService::new(
                storage_manager,
                artifact_config,
            );

            // Store the snapshot and get real storage paths
            let snapshot_bytes_for_storage = Bytes::from(capture_result.data.clone());
            let stored_artifact = storage_service
                .store_snapshot(&params.stream_id, snapshot_bytes_for_storage)
                .await
                .map_err(|e| gl_core::Error::Storage(format!("Failed to store snapshot: {}", e)))?;

            // Extract file path from URI - convert file:// URI to filesystem path
            let uri_path = stored_artifact.uri.path().unwrap_or_default().to_string();
            let storage_uri = stored_artifact.uri.to_string();

            // Debug logging to understand what's happening
            debug!(
                "Storage artifact created - URI: {}, URI path: {}, Stream dir: {}",
                storage_uri,
                uri_path,
                stream_artifacts_dir.display()
            );

            // The file should already be stored by the storage service,
            // so we just need to record the correct path in the database
            let full_file_path = if let Some(stripped) = uri_path.strip_prefix('/') {
                stream_artifacts_dir
                    .join(stripped)
                    .to_string_lossy()
                    .to_string()
            } else {
                stream_artifacts_dir
                    .join(&uri_path)
                    .to_string_lossy()
                    .to_string()
            };

            // Create snapshot record with perceptual hash included atomically
            let snapshot_request = gl_db::CreateSnapshotRequest {
                stream_id: params.stream_id.clone(),
                user_id,
                file_path: full_file_path,
                storage_uri,
                content_type: capture_result.content_type.clone(),
                width: Some(capture_result.width as i64),
                height: Some(capture_result.height as i64),
                file_size: capture_result.data.len() as i64,
                checksum: Some(capture_result.checksum),
                etag: None,
                captured_at: chrono::Utc::now().to_rfc3339(),
                perceptual_hash: Some(current_hash.clone()),
            };

            // Save snapshot to database with hash included - no race condition
            let snapshot_repo = gl_db::SnapshotRepository::new(context.db.pool());
            let created_snapshot = snapshot_repo.create(snapshot_request).await?;

            snapshot_id = Some(created_snapshot.id.clone());
            image_stored = true;

            info!(
                "Stored new snapshot for stream {} (hash changed): {}",
                params.stream_id, created_snapshot.id
            );
        } else {
            // Hash hasn't changed significantly, update the most recent snapshot with matching hash
            let rows_affected = sqlx::query(
                "UPDATE snapshots
                 SET updated_at = ?
                 WHERE id = (
                     SELECT id FROM snapshots
                     WHERE stream_id = ? AND perceptual_hash = ?
                     ORDER BY captured_at DESC
                     LIMIT 1
                 )",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&params.stream_id)
            .bind(&previous_hash_str)
            .execute(context.db.pool())
            .await
            .map_err(|e| gl_core::Error::Database(format!("Failed to update timestamp: {}", e)))?
            .rows_affected();

            if rows_affected == 0 {
                warn!(
                    "No snapshot found to update for stream {} with hash {}. This might indicate a race condition.",
                    params.stream_id, current_hash
                );
            }

            debug!(
                "Skipped storage for stream {} (hash unchanged): similarity above threshold {}",
                params.stream_id, threshold
            );
        }

        let result = serde_json::json!({
            "stream_id": params.stream_id,
            "snapshot_id": snapshot_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "status": if hash_changed { "captured" } else { "skipped_duplicate" },
            "hash_changed": hash_changed,
            "image_stored": image_stored,
            "previous_hash": previous_hash_str,
            "current_hash": current_hash,
            "similarity_threshold": threshold
        });

        debug!(
            "Smart snapshot job completed for stream: {} (hash_changed={})",
            params.stream_id, hash_changed
        );
        Ok(result)
    }

    fn job_type(&self) -> &'static str {
        "smart_snapshot"
    }

    fn validate_parameters(&self, parameters: &serde_json::Value) -> Result<()> {
        let _params: SmartSnapshotParams = serde_json::from_value(parameters.clone())
            .map_err(|e| gl_core::Error::Validation(format!("Invalid parameters: {}", e)))?;
        Ok(())
    }
}

/// Parameters for smart snapshot jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct SmartSnapshotParams {
    pub stream_id: String,
    pub quality: Option<String>, // "high", "medium", "low"
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub enable_motion_analysis: Option<bool>,
    pub hash_threshold: Option<f64>, // Sensitivity for hash comparison
}

/// Motion detection job that analyzes recent snapshots for changes
pub struct MotionDetectionJob;

impl Default for MotionDetectionJob {
    fn default() -> Self {
        Self::new()
    }
}

impl MotionDetectionJob {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl JobHandler for MotionDetectionJob {
    async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
        info!("Executing motion detection job: {}", context.job_id);

        // Check for cancellation
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled("Job cancelled".into()));
        }

        let params: MotionDetectionParams = serde_json::from_value(context.parameters)
            .map_err(|e| gl_core::Error::Validation(format!("Invalid motion parameters: {}", e)))?;

        info!("Analyzing motion for stream: {}", params.stream_id);

        // TODO: Implement motion detection:
        // 1. Get recent snapshots from database (based on hash changes)
        // 2. Compare perceptual hashes to detect significant changes
        // 3. If motion detected:
        //    - Create motion event record
        //    - Trigger alert if configured
        //    - Run AI classification if enabled
        // 4. Update motion detection statistics

        let result = serde_json::json!({
            "stream_id": params.stream_id,
            "analysis_timestamp": chrono::Utc::now().to_rfc3339(),
            "motion_detected": false,  // Would be calculated
            "confidence": 0.0,
            "frames_analyzed": 5,
            "events_generated": 0
        });

        debug!(
            "Motion detection completed for stream: {}",
            params.stream_id
        );
        Ok(result)
    }

    fn job_type(&self) -> &'static str {
        "motion_detection"
    }

    fn validate_parameters(&self, parameters: &serde_json::Value) -> Result<()> {
        let _params: MotionDetectionParams = serde_json::from_value(parameters.clone())
            .map_err(|e| gl_core::Error::Validation(format!("Invalid parameters: {}", e)))?;
        Ok(())
    }
}

/// Parameters for motion detection jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct MotionDetectionParams {
    pub stream_id: String,
    pub sensitivity: Option<f64>,    // 0.0 to 1.0
    pub window_seconds: Option<u32>, // Time window to analyze
    pub enable_ai_classification: Option<bool>,
}

/// System maintenance job for cleanup tasks
pub struct MaintenanceJob;

impl Default for MaintenanceJob {
    fn default() -> Self {
        Self::new()
    }
}

impl MaintenanceJob {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl JobHandler for MaintenanceJob {
    async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
        info!("Executing maintenance job: {}", context.job_id);

        // Check for cancellation
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled("Job cancelled".into()));
        }

        let params: MaintenanceParams =
            serde_json::from_value(context.parameters).map_err(|e| {
                gl_core::Error::Validation(format!("Invalid maintenance parameters: {}", e))
            })?;

        let mut tasks_completed = Vec::new();

        // Cleanup old job results
        if params.cleanup_old_jobs.unwrap_or(true) {
            if context.cancellation_token.is_cancelled() {
                return Err(gl_core::Error::Cancelled(
                    "Job cancelled during cleanup".into(),
                ));
            }
            info!("Cleaning up old job execution records");
            // TODO: Implement cleanup logic
            tasks_completed.push("job_cleanup");
        }

        // Cleanup old snapshots (keep based on retention policy)
        if params.cleanup_old_snapshots.unwrap_or(true) {
            if context.cancellation_token.is_cancelled() {
                return Err(gl_core::Error::Cancelled(
                    "Job cancelled during cleanup".into(),
                ));
            }
            info!("Cleaning up old snapshot files based on retention policy");
            // TODO: Implement snapshot cleanup
            // Should respect hash-based deduplication - only delete files not referenced
            tasks_completed.push("snapshot_cleanup");
        }

        // Database maintenance
        if params.database_maintenance.unwrap_or(true) {
            if context.cancellation_token.is_cancelled() {
                return Err(gl_core::Error::Cancelled(
                    "Job cancelled during cleanup".into(),
                ));
            }
            info!("Running database maintenance tasks");
            // TODO: VACUUM, analyze, etc.
            tasks_completed.push("database_maintenance");
        }

        let result = serde_json::json!({
            "maintenance_timestamp": chrono::Utc::now().to_rfc3339(),
            "tasks_completed": tasks_completed,
            "duration_ms": 0  // Would be calculated
        });

        debug!("Maintenance job completed");
        Ok(result)
    }

    fn job_type(&self) -> &'static str {
        "maintenance"
    }
}

/// Parameters for maintenance jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct MaintenanceParams {
    pub cleanup_old_jobs: Option<bool>,
    pub cleanup_old_snapshots: Option<bool>,
    pub database_maintenance: Option<bool>,
    pub retention_days: Option<u32>,
}

/// AI analysis job for processing captured content
pub struct AiAnalysisJob;

impl Default for AiAnalysisJob {
    fn default() -> Self {
        Self::new()
    }
}

impl AiAnalysisJob {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl JobHandler for AiAnalysisJob {
    async fn execute(&self, context: JobContext) -> Result<serde_json::Value> {
        info!("Executing AI analysis job: {}", context.job_id);

        // Check for cancellation
        if context.cancellation_token.is_cancelled() {
            return Err(gl_core::Error::Cancelled("Job cancelled".into()));
        }

        let params: AiAnalysisParams = serde_json::from_value(context.parameters)
            .map_err(|e| gl_core::Error::Validation(format!("Invalid AI parameters: {}", e)))?;

        info!("Running AI analysis on snapshot: {}", params.snapshot_id);

        // TODO: Implement AI analysis:
        // 1. Load snapshot from storage
        // 2. Run through AI describe_frame endpoint
        // 3. If motion event provided, run through classify_event endpoint
        // 4. Store analysis results in database
        // 5. Trigger alerts if configured conditions met

        let result = serde_json::json!({
            "snapshot_id": params.snapshot_id,
            "analysis_timestamp": chrono::Utc::now().to_rfc3339(),
            "objects_detected": ["person", "vehicle"],  // Would come from AI
            "description": "A person walking past a parked vehicle",  // AI generated
            "confidence": 0.92,
            "classification": "normal_activity"  // If event analysis was requested
        });

        debug!("AI analysis completed for snapshot: {}", params.snapshot_id);
        Ok(result)
    }

    fn job_type(&self) -> &'static str {
        "ai_analysis"
    }
}

/// Parameters for AI analysis jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct AiAnalysisParams {
    pub snapshot_id: String,
    pub event_id: Option<String>,    // If analyzing a motion event
    pub analysis_types: Vec<String>, // ["describe", "classify", "detect_objects"]
    pub priority: Option<String>,    // "low", "normal", "high"
}

/// Create all standard job handlers
pub fn create_standard_handlers() -> HashMap<String, Arc<dyn JobHandler>> {
    let mut handlers: HashMap<String, Arc<dyn JobHandler>> = HashMap::new();

    handlers.insert(
        "smart_snapshot".to_string(),
        Arc::new(SmartSnapshotJob::new()),
    );
    handlers.insert(
        "motion_detection".to_string(),
        Arc::new(MotionDetectionJob::new()),
    );
    handlers.insert("maintenance".to_string(), Arc::new(MaintenanceJob::new()));
    handlers.insert("ai_analysis".to_string(), Arc::new(AiAnalysisJob::new()));

    handlers
}

/// Calculate perceptual hash from pre-loaded image (optimized version)
/// Returns a hash string that can be compared to detect similar images
fn calculate_perceptual_hash_from_image(img: &img_hash::image::DynamicImage) -> Result<String> {
    // Configure hasher for perceptual hash (pHash algorithm)
    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::Gradient) // Gradient-based perceptual hash
        .hash_size(8, 8) // 8x8 hash for good balance of speed/accuracy
        .to_hasher();

    // Calculate hash directly
    let hash = hasher.hash_image(img);

    // Convert to base64 string for storage
    Ok(hash.to_base64())
}

/// Calculate similarity score between two perceptual hashes (0.0 to 1.0)
/// 1.0 means identical, 0.0 means completely different
fn calculate_similarity_score(hash1: &str, hash2: &str) -> Result<f64> {
    let hash1: ImageHash<[u8; 8]> = ImageHash::from_base64(hash1)
        .map_err(|e| gl_core::Error::Validation(format!("Invalid hash1: {:?}", e)))?;
    let hash2: ImageHash<[u8; 8]> = ImageHash::from_base64(hash2)
        .map_err(|e| gl_core::Error::Validation(format!("Invalid hash2: {:?}", e)))?;

    // Calculate Hamming distance between hashes
    let distance = hash1.dist(&hash2);

    // Convert distance to similarity (0 = identical, max_distance = completely different)
    let max_distance = 64f64; // For 8x8 hash
    let similarity = 1.0 - (distance as f64 / max_distance);

    Ok(similarity)
}

/// Compare two perceptual hashes and return whether they are similar enough
/// Returns true if images are similar enough (above threshold)
fn compare_perceptual_hashes(hash1: &str, hash2: &str, threshold: f64) -> Result<bool> {
    let similarity = calculate_similarity_score(hash1, hash2)?;
    Ok(similarity >= threshold)
}
