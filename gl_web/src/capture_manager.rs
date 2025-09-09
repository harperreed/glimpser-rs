//! ABOUTME: Central service for managing capture processes and their lifecycle
//! ABOUTME: Coordinates stream execution, status tracking, and resource management

use bytes::Bytes;
use gl_analysis::{AnalysisConfig, AnalysisService, ProcessorContext, ProcessorInput};
use gl_capture::{
    artifact_storage::{ArtifactStorageConfig, ArtifactStorageService},
    CaptureHandle, CaptureSource, FfmpegConfig, FfmpegSource, FileSource, HardwareAccel,
    OutputFormat, RtspTransport, YtDlpConfig, YtDlpSource,
};
#[cfg(feature = "website")]
use gl_capture::{WebsiteConfig, WebsiteSource};
use gl_config::Config as AppConfig;
use gl_core::{time::now_iso8601, Error, Result};
use gl_db::{CreateSnapshotRequest, SnapshotRepository, Stream, StreamRepository};
use gl_notify::NotificationManager;
use gl_storage::StorageManager;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot, RwLock};
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Status of a running capture
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    Starting,
    Active,
    Stopping,
    Inactive,
    Error(String),
}

impl CaptureStatus {
    pub fn as_str(&self) -> &str {
        match self {
            CaptureStatus::Starting => "starting",
            CaptureStatus::Active => "active",
            CaptureStatus::Stopping => "stopping",
            CaptureStatus::Inactive => "inactive",
            CaptureStatus::Error(_) => "error",
        }
    }
}

/// Information about a running capture
#[derive(Debug, Clone)]
pub struct CaptureInfo {
    pub stream_id: String,
    pub status: CaptureStatus,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_frame_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Handle for a running capture task with broadcast capabilities
struct CaptureTask {
    handle: JoinHandle<()>,
    info: CaptureInfo,
    capture_handle: Option<Arc<CaptureHandle>>,
    /// Broadcast channel for real-time frame distribution to MJPEG streams
    frame_sender: broadcast::Sender<Bytes>,
    /// Latest snapshot data for immediate API responses
    latest_snapshot: Arc<RwLock<Option<Bytes>>>,
}

impl std::fmt::Debug for CaptureTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureTask")
            .field("info", &self.info)
            .field("handle", &"JoinHandle<()>")
            .field(
                "capture_handle",
                &self.capture_handle.as_ref().map(|_| "CaptureHandle"),
            )
            .field("subscribers", &self.frame_sender.receiver_count())
            .field("has_latest_snapshot", &"Arc<RwLock<Option<Bytes>>>")
            .finish()
    }
}

/// Central manager for capture processes
pub struct CaptureManager {
    db_pool: sqlx::SqlitePool,
    running_captures: Arc<RwLock<HashMap<String, CaptureTask>>>,
    analysis_service: Option<Arc<tokio::sync::Mutex<AnalysisService>>>,
    storage_config: gl_config::StorageConfig,
}

impl CaptureManager {
    /// Create a new CaptureManager
    pub fn new(db_pool: sqlx::SqlitePool) -> Self {
        let manager = Self {
            db_pool: db_pool.clone(),
            running_captures: Arc::new(RwLock::new(HashMap::new())),
            analysis_service: None,
            storage_config: gl_config::StorageConfig::default(),
        };

        // Reset any stale "active" statuses from previous server runs
        tokio::spawn(async move {
            let stream_repo = StreamRepository::new(&db_pool);
            if let Err(e) = stream_repo.reset_stale_active_statuses().await {
                warn!("Failed to reset stale stream statuses: {}", e);
            } else {
                debug!("Reset stale stream statuses on startup");
            }
        });

        manager
    }

    /// Create a new CaptureManager with custom storage configuration (legacy compatibility)
    pub fn with_storage_config(
        db_pool: sqlx::SqlitePool,
        storage_config: gl_config::StorageConfig,
    ) -> Self {
        Self {
            db_pool: db_pool.clone(),
            running_captures: Arc::new(RwLock::new(HashMap::new())),
            analysis_service: None,
            storage_config,
        }
    }

    /// Create a new CaptureManager with analysis services enabled
    pub fn with_analysis_config(
        db_pool: sqlx::SqlitePool,
        storage_config: gl_config::StorageConfig,
        app_config: &AppConfig,
    ) -> Result<Self> {
        let mut manager = Self {
            db_pool: db_pool.clone(),
            running_captures: Arc::new(RwLock::new(HashMap::new())),
            analysis_service: None,
            storage_config,
        };

        // Initialize analysis service if AI is enabled
        if app_config.features.enable_ai {
            info!("Initializing analysis service with AI enabled");

            // Create analysis configuration with online AI enabled
            let mut ai_config = app_config.ai.to_ai_config();

            // When AI features are enabled, ensure online mode is active
            ai_config.use_online = true;

            // Validate that we have an API key for online mode
            if ai_config.api_key.is_none() || ai_config.api_key.as_ref().unwrap().is_empty() {
                warn!("AI features enabled but no API key provided, falling back to offline mode");
                ai_config.use_online = false;
            }

            let analysis_config = AnalysisConfig {
                ai: Some(ai_config),
                ..Default::default()
            };

            // Create notification manager (stub for now, can be enhanced later)
            let notification_manager = NotificationManager::new();

            // Create analysis service with persistence
            let analysis_service = AnalysisService::with_persistence(
                analysis_config,
                gl_db::Db::from_pool(db_pool.clone()),
                notification_manager,
            )?;

            manager.analysis_service = Some(Arc::new(tokio::sync::Mutex::new(analysis_service)));
            info!("Analysis service initialized successfully");
        } else {
            info!("AI features disabled, analysis service not initialized");
        }

        // Reset any stale "active" statuses from previous server runs
        let reset_db_pool = db_pool.clone();
        tokio::spawn(async move {
            let stream_repo = StreamRepository::new(&reset_db_pool);
            if let Err(e) = stream_repo.reset_stale_active_statuses().await {
                warn!("Failed to reset stale stream statuses: {}", e);
            }
        });

        Ok(manager)
    }

    /// Start a capture from a stream
    pub async fn start_stream(&self, stream_id: &str) -> Result<()> {
        info!(stream_id = %stream_id, "Starting stream capture");

        // Get stream from database first
        let stream_repo = StreamRepository::new(&self.db_pool);
        let stream = stream_repo
            .find_by_id(stream_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Stream {} not found", stream_id)))?;

        // Use single write lock to atomically check and insert
        let mut captures = self.running_captures.write().await;

        // Check if already running under write lock
        if captures.contains_key(stream_id) {
            warn!(stream_id = %stream_id, "Stream capture already running");
            return Err(Error::Config("Stream capture already running".to_string()));
        }

        // Update status to starting with execution timestamp
        stream_repo
            .update_execution_status(
                stream_id,
                "starting",
                Some(&chrono::Utc::now().to_rfc3339()),
            )
            .await?;

        // Create capture info
        let capture_info = CaptureInfo {
            stream_id: stream_id.to_string(),
            status: CaptureStatus::Starting,
            started_at: Some(chrono::Utc::now()),
            last_frame_at: None,
        };

        // Create broadcast channel for real-time frame distribution (capacity of 10 frames)
        let (frame_sender, _) = broadcast::channel(10);
        let latest_snapshot = Arc::new(RwLock::new(None));

        // Start the capture task
        let db_pool = self.db_pool.clone();
        let stream_clone = stream.clone();
        let running_captures = self.running_captures.clone();
        let stream_id_clone = stream_id.to_string();
        let frame_sender_clone = frame_sender.clone();
        let latest_snapshot_clone = latest_snapshot.clone();
        let analysis_service_clone = self.analysis_service.clone();
        // Note: We pass storage_service by reference to avoid clone issues
        // The spawned task will create its own copy of necessary components

        // Create channel to receive the capture handle for efficient snapshotting
        let (capture_handle_sender, capture_handle_receiver) = oneshot::channel();

        let storage_config_clone = self.storage_config.clone();
        let handle = tokio::spawn(async move {
            // Create fresh storage service instance for the async task
            let artifacts_dir = PathBuf::from(&storage_config_clone.artifacts_dir);
            let gl_storage_config = gl_storage::StorageConfig {
                base_dir: Some(artifacts_dir),
                ..Default::default()
            };
            let storage_manager = StorageManager::new(gl_storage_config)
                .expect("Failed to create storage manager for async task");
            let artifact_config = ArtifactStorageConfig {
                base_uri: format!("file://{}", storage_config_clone.artifacts_dir),
                snapshot_extension: "jpg".to_string(),
                include_timestamp: true,
            };
            let storage_service = ArtifactStorageService::new(storage_manager, artifact_config);

            let result = Self::run_persistent_capture_task(
                db_pool.clone(),
                storage_service,
                stream_clone,
                stream_id_clone.clone(),
                frame_sender_clone,
                latest_snapshot_clone,
                analysis_service_clone,
                Some(capture_handle_sender),
            )
            .await;

            // Always remove from running captures when task completes
            {
                let mut captures = running_captures.write().await;
                captures.remove(&stream_id_clone);
            }

            // Update database status based on result
            let stream_repo = StreamRepository::new(&db_pool);
            match result {
                Ok(_) => {
                    if let Err(e) = stream_repo
                        .update_execution_status(&stream_id_clone, "inactive", None)
                        .await
                    {
                        error!(stream_id = %stream_id_clone, error = %e, "Failed to update completion status");
                    }
                }
                Err(e) => {
                    error!(stream_id = %stream_id_clone, error = %e, "Capture task failed");
                    let error_msg = format!("Capture failed: {}", e);
                    if let Err(update_err) = stream_repo
                        .update_execution_status_with_error(&stream_id_clone, "error", &error_msg)
                        .await
                    {
                        error!(stream_id = %stream_id_clone, error = %update_err, "Failed to update error status");
                    }
                }
            }
        });

        // Store the task under the existing write lock
        let task = CaptureTask {
            handle,
            info: capture_info,
            capture_handle: None, // Will be updated when capture handle is received
            frame_sender,
            latest_snapshot,
        };

        captures.insert(stream_id.to_string(), task);
        drop(captures); // Explicitly release lock

        // Spawn a task to update the capture handle when it becomes available
        let running_captures_clone = self.running_captures.clone();
        let stream_id_for_handle = stream_id.to_string();
        tokio::spawn(async move {
            if let Ok(capture_handle) = capture_handle_receiver.await {
                let mut captures = running_captures_clone.write().await;
                if let Some(task) = captures.get_mut(&stream_id_for_handle) {
                    task.capture_handle = Some(capture_handle);
                    debug!(stream_id = %stream_id_for_handle, "Updated capture handle for efficient snapshotting");
                }
            }
        });

        debug!(stream_id = %stream_id, "Stream capture started");
        Ok(())
    }

    /// Stop a running capture
    pub async fn stop_stream(&self, stream_id: &str) -> Result<()> {
        info!(stream_id = %stream_id, "Stopping stream capture");

        let task = {
            let mut captures = self.running_captures.write().await;
            captures.remove(stream_id)
        };

        if let Some(task) = task {
            // Update status to stopping (no timestamp update needed)
            let stream_repo = StreamRepository::new(&self.db_pool);
            stream_repo
                .update_execution_status(stream_id, "stopping", None)
                .await?;

            // Abort the task and wait briefly for it to finish
            task.handle.abort();

            // Give the task a moment to cleanup
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Update status to inactive
            stream_repo
                .update_execution_status(stream_id, "inactive", None)
                .await?;

            debug!(stream_id = %stream_id, "Stream capture stopped");
            Ok(())
        } else {
            warn!(stream_id = %stream_id, "Stream capture not running");
            Err(Error::NotFound("Stream capture not running".to_string()))
        }
    }

    /// Get status of a capture
    pub async fn get_capture_status(&self, stream_id: &str) -> Option<CaptureInfo> {
        let captures = self.running_captures.read().await;
        captures.get(stream_id).map(|task| task.info.clone())
    }

    /// Get all running captures
    pub async fn get_all_captures(&self) -> Vec<CaptureInfo> {
        let captures = self.running_captures.read().await;
        captures.values().map(|task| task.info.clone()).collect()
    }

    /// Check if a stream is currently running
    pub async fn is_stream_running(&self, stream_id: &str) -> bool {
        let captures = self.running_captures.read().await;
        captures.contains_key(stream_id)
    }

    /// Get the latest snapshot from a running stream (fast database/memory lookup)
    pub async fn get_latest_snapshot(&self, stream_id: &str) -> Result<bytes::Bytes> {
        // First try to get from running capture task for real-time data
        if let Some(latest_snapshot) = {
            let captures = self.running_captures.read().await;
            captures
                .get(stream_id)
                .map(|task| task.latest_snapshot.clone())
        } {
            let snapshot_guard = latest_snapshot.read().await;
            if let Some(ref snapshot) = *snapshot_guard {
                return Ok(snapshot.clone());
            }
        }

        // Try to take a fresh snapshot from the running capture handle (efficient)
        if let Some(capture_handle) = {
            let captures = self.running_captures.read().await;
            captures
                .get(stream_id)
                .and_then(|task| task.capture_handle.clone())
        } {
            debug!(stream_id = %stream_id, "Taking fresh snapshot from running capture handle");
            match capture_handle.snapshot().await {
                Ok(snapshot_data) => {
                    // Cache the snapshot for future requests
                    if let Some(latest_snapshot) = {
                        let captures = self.running_captures.read().await;
                        captures
                            .get(stream_id)
                            .map(|task| task.latest_snapshot.clone())
                    } {
                        let mut snapshot_guard = latest_snapshot.write().await;
                        *snapshot_guard = Some(snapshot_data.clone());
                    }
                    return Ok(snapshot_data);
                }
                Err(e) => {
                    warn!(stream_id = %stream_id, error = %e, "Failed to take snapshot from capture handle, falling back");
                }
            }
        }

        // Fall back to database for the latest stored snapshot
        let repo = SnapshotRepository::new(&self.db_pool);
        if let Some(latest_snapshot) = repo.get_latest_by_template(stream_id).await? {
            // Read the file from storage
            let file_path = PathBuf::from(&latest_snapshot.file_path);
            match tokio::fs::read(&file_path).await {
                Ok(data) => Ok(Bytes::from(data)),
                Err(e) => {
                    warn!(
                        stream_id = %stream_id,
                        error = %e,
                        file_path = %latest_snapshot.file_path,
                        "Failed to read snapshot file, falling back to on-demand capture"
                    );
                    self.take_stream_snapshot_fallback(stream_id).await
                }
            }
        } else {
            // No snapshot available, try on-demand capture as fallback
            self.take_stream_snapshot_fallback(stream_id).await
        }
    }

    /// Subscribe to real-time frame broadcast from a running stream
    pub async fn subscribe_to_stream(&self, stream_id: &str) -> Option<broadcast::Receiver<Bytes>> {
        let captures = self.running_captures.read().await;
        captures
            .get(stream_id)
            .map(|task| task.frame_sender.subscribe())
    }

    /// Take a snapshot from a running stream (fallback method - on-demand capture)
    /// This is the original behavior, used when no running capture exists
    pub async fn take_stream_snapshot_fallback(&self, stream_id: &str) -> Result<bytes::Bytes> {
        // Check if stream is running
        if !self.is_stream_running(stream_id).await {
            return Err(Error::NotFound(
                "Stream is not currently running".to_string(),
            ));
        }

        // Get stream from database to recreate capture source
        let stream_repo = StreamRepository::new(&self.db_pool);
        let stream = stream_repo
            .find_by_id(stream_id)
            .await?
            .ok_or_else(|| Error::NotFound("Stream not found".to_string()))?;

        // Parse stream config
        let config: Value = serde_json::from_str(&stream.config)
            .map_err(|e| Error::Config(format!("Invalid stream config JSON: {}", e)))?;

        let kind = config
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Stream config missing 'kind' field".to_string()))?;

        // Create a temporary capture source to get snapshot
        match kind {
            "file" => Self::take_file_snapshot(&config).await,
            "rtsp" => Self::take_rtsp_snapshot(&config).await,
            "ffmpeg" => Self::take_ffmpeg_snapshot(&config).await,
            "website" => Self::take_website_snapshot(&config).await,
            "yt" => Self::take_yt_snapshot(&config).await,
            _ => Err(Error::Config(format!("Unsupported stream kind: {}", kind))),
        }
    }

    /// Internal method to run a persistent capture task with broadcast capabilities
    #[allow(clippy::too_many_arguments)]
    async fn run_persistent_capture_task(
        db_pool: sqlx::SqlitePool,
        storage_service: ArtifactStorageService<StorageManager>,
        stream: Stream,
        stream_id: String,
        frame_sender: broadcast::Sender<Bytes>,
        latest_snapshot: Arc<RwLock<Option<Bytes>>>,
        analysis_service: Option<Arc<tokio::sync::Mutex<AnalysisService>>>,
        capture_handle_sender: Option<oneshot::Sender<Arc<CaptureHandle>>>,
    ) -> Result<()> {
        info!(stream_id = %stream_id, "Running persistent capture task with broadcast");

        // Parse stream config
        let config: Value = serde_json::from_str(&stream.config)
            .map_err(|e| Error::Config(format!("Invalid stream config JSON: {}", e)))?;

        let kind = config
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Stream config missing 'kind' field".to_string()))?;

        // Update status to active with timestamp
        let stream_repo = StreamRepository::new(&db_pool);
        stream_repo
            .update_execution_status(&stream_id, "active", Some(&chrono::Utc::now().to_rfc3339()))
            .await?;

        // Create and start the capture source based on stream type
        let capture_handle = match kind {
            "file" => Self::create_file_capture(&config).await?,
            "rtsp" => Self::create_rtsp_capture(&config).await?,
            "ffmpeg" => Self::create_ffmpeg_capture(&config).await?,
            "website" => Self::create_website_capture(&config).await?,
            "yt" => Self::create_yt_capture(&config).await?,
            _ => return Err(Error::Config(format!("Unsupported stream kind: {}", kind))),
        };

        // Send the capture handle back to the main thread for efficient snapshotting
        if let Some(sender) = capture_handle_sender {
            let _ = sender.send(capture_handle.clone());
        }

        // Get snapshot interval from config (default: 5 seconds)
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        // Setup snapshot timer
        let mut snapshot_timer = interval(Duration::from_secs(snapshot_interval));

        // Get duration from config (default: 1 hour, 0 = infinite)
        let duration = config
            .get("duration")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600); // Default 1 hour

        // Setup end time for finite duration captures
        let end_time = if duration > 0 {
            Some(tokio::time::Instant::now() + Duration::from_secs(duration))
        } else {
            None
        };

        // Persistent capture loop with graceful shutdown
        loop {
            tokio::select! {
                _ = snapshot_timer.tick() => {
                    // Take snapshot
                    match capture_handle.snapshot().await {
                        Ok(snapshot_data) => {
                            debug!(
                                stream_id = %stream_id,
                                frame_size = snapshot_data.len(),
                                subscribers = frame_sender.receiver_count(),
                                "Captured frame for persistent task"
                            );

                            // Store latest snapshot in memory for fast API access
                            {
                                let mut snapshot_guard = latest_snapshot.write().await;
                                *snapshot_guard = Some(snapshot_data.clone());
                            }

                            // Broadcast to MJPEG streams (ignore errors if no subscribers)
                            let _ = frame_sender.send(snapshot_data.clone());

                            // Store to database and filesystem (directly, not spawned for simplicity)
                            if let Err(e) = Self::store_snapshot_async(
                                &storage_service,
                                &db_pool,
                                &stream_id,
                                &stream.user_id,
                                &snapshot_data,
                            ).await {
                                warn!(
                                    stream_id = %stream_id,
                                    error = %e,
                                    "Failed to store snapshot"
                                );
                            }

                            // Process through analysis service if available
                            if let Some(analysis_service) = &analysis_service {
                                let analysis_clone = analysis_service.clone();
                                let stream_id_clone = stream_id.clone();
                                let snapshot_clone = snapshot_data.clone();

                                // Spawn analysis task to avoid blocking capture loop
                                tokio::spawn(async move {
                                    let mut service_guard = analysis_clone.lock().await;

                                    let processor_input = ProcessorInput {
                                        template_id: stream_id_clone.clone(),
                                        frame_data: Some(snapshot_clone),
                                        frame_format: Some("jpeg".to_string()), // Most captures are JPEG
                                        text_content: None,
                                        context: ProcessorContext::new(stream_id_clone.clone()),
                                        timestamp: chrono::Utc::now(),
                                    };

                                    match service_guard.analyze(processor_input).await {
                                        Ok(events) => {
                                            if !events.is_empty() {
                                                debug!(
                                                    stream_id = %stream_id_clone,
                                                    event_count = events.len(),
                                                    "Analysis completed with events"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                stream_id = %stream_id_clone,
                                                error = %e,
                                                "Analysis failed for snapshot"
                                            );
                                        }
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            warn!(
                                stream_id = %stream_id,
                                error = %e,
                                "Failed to capture frame in persistent task"
                            );
                        }
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!(stream_id = %stream_id, "Persistent capture interrupted by signal");
                    break;
                }
                _ = async {
                    if let Some(end_time) = end_time {
                        tokio::time::sleep_until(end_time).await;
                    } else {
                        // For infinite duration, never complete this branch
                        std::future::pending::<()>().await;
                    }
                } => {
                    info!(stream_id = %stream_id, duration = duration, "Persistent capture completed after duration limit");
                    break;
                }
            }

            // Check if we should exit due to duration limit
            if let Some(end_time) = end_time {
                if tokio::time::Instant::now() >= end_time {
                    break;
                }
            }
        }

        drop(capture_handle);
        Ok(())
    }

    /// Internal method to run a capture task (legacy method for compatibility)
    #[allow(dead_code)]
    async fn run_capture_task(
        db_pool: sqlx::SqlitePool,
        stream: Stream,
        stream_id: String,
    ) -> Result<()> {
        info!(stream_id = %stream_id, "Running capture task");

        // Parse stream config
        let config: Value = serde_json::from_str(&stream.config)
            .map_err(|e| Error::Config(format!("Invalid stream config JSON: {}", e)))?;

        let kind = config
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Stream config missing 'kind' field".to_string()))?;

        // Update status to active with timestamp
        let stream_repo = StreamRepository::new(&db_pool);
        stream_repo
            .update_execution_status(&stream_id, "active", Some(&chrono::Utc::now().to_rfc3339()))
            .await?;

        // Create and start the capture source based on stream type
        let capture_manager = CaptureManager::new(db_pool);
        match kind {
            "file" => {
                capture_manager
                    .run_file_capture(&config, &stream_id, &stream.user_id)
                    .await
            }
            "ffmpeg" => {
                capture_manager
                    .run_ffmpeg_capture(&config, &stream_id, &stream.user_id)
                    .await
            }
            "website" => {
                capture_manager
                    .run_website_capture(&config, &stream_id, &stream.user_id)
                    .await
            }
            "yt" => {
                capture_manager
                    .run_yt_capture(&config, &stream_id, &stream.user_id)
                    .await
            }
            "rtsp" => {
                capture_manager
                    .run_rtsp_capture(&config, &stream_id, &stream.user_id)
                    .await
            }
            _ => Err(Error::Config(format!("Unsupported stream kind: {}", kind))),
        }
    }

    #[allow(dead_code)]
    async fn run_file_capture(&self, config: &Value, stream_id: &str, user_id: &str) -> Result<()> {
        let file_path = config
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("File stream config missing 'file_path' field".to_string())
            })?;

        // Validate file path for security (prevent path traversal)
        let source_path = PathBuf::from(file_path);
        if source_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Config("Path traversal not allowed".to_string()));
        }

        let file_source = FileSource::new(&source_path);
        let handle = file_source.start().await?;

        // Setup snapshot capture loop
        let duration = config
            .get("duration")
            .and_then(|v| v.as_u64())
            .unwrap_or(300); // Default 5 minutes
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5); // Default 5 seconds between snapshots

        let timeout = std::cmp::min(duration, u32::MAX as u64) as u32;
        let mut snapshot_timer =
            tokio::time::interval(std::time::Duration::from_secs(snapshot_interval));
        let end_time = std::time::Instant::now() + std::time::Duration::from_secs(timeout as u64);

        // Run capture with snapshot loop
        loop {
            tokio::select! {
                _ = snapshot_timer.tick() => {
                    // Take snapshot
                    match handle.snapshot().await {
                        Ok(snapshot_data) => {
                            debug!("File capture snapshot taken, {} bytes", snapshot_data.len());
                            if let Err(e) = self.store_snapshot(stream_id, user_id, &snapshot_data).await {
                                warn!("Failed to store file capture snapshot: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to take file capture snapshot: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(end_time)) => {
                    debug!("File capture completed after {} seconds", timeout);
                    break;
                }
                _ = tokio::signal::ctrl_c() => {
                    debug!("File capture interrupted by signal");
                    break;
                }
            }

            if std::time::Instant::now() >= end_time {
                break;
            }
        }

        drop(handle);
        Ok(())
    }

    #[allow(dead_code)]
    async fn run_ffmpeg_capture(
        &self,
        config: &Value,
        stream_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let source_url = config
            .get("source_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("FFmpeg stream config missing 'source_url' field".to_string())
            })?;

        let mut ffmpeg_config = FfmpegConfig {
            input_url: source_url.to_string(),
            ..Default::default()
        };

        // Parse optional configuration
        if let Some(hw_accel) = config.get("hardware_accel").and_then(|v| v.as_str()) {
            ffmpeg_config.hardware_accel = match hw_accel.to_lowercase().as_str() {
                "vaapi" => HardwareAccel::Vaapi,
                "cuda" => HardwareAccel::Cuda,
                "qsv" => HardwareAccel::Qsv,
                "videotoolbox" => HardwareAccel::VideoToolbox,
                _ => HardwareAccel::None,
            };
        }

        // Safe timeout handling - prevent overflow
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;

        // Setup snapshot capture loop
        let duration = config
            .get("duration")
            .and_then(|v| v.as_u64())
            .unwrap_or(1800); // Default 30 minutes
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5); // Default 5 seconds between snapshots

        let safe_duration = std::cmp::min(duration, u32::MAX as u64) as u32;
        let mut snapshot_timer =
            tokio::time::interval(std::time::Duration::from_secs(snapshot_interval));
        let end_time =
            std::time::Instant::now() + std::time::Duration::from_secs(safe_duration as u64);

        // Run capture with snapshot loop
        loop {
            tokio::select! {
                _ = snapshot_timer.tick() => {
                    // Take snapshot
                    match handle.snapshot().await {
                        Ok(snapshot_data) => {
                            debug!("FFmpeg capture snapshot taken, {} bytes", snapshot_data.len());
                            if let Err(e) = self.store_snapshot(stream_id, user_id, &snapshot_data).await {
                                warn!("Failed to store FFmpeg capture snapshot: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to take FFmpeg capture snapshot: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(end_time)) => {
                    debug!("FFmpeg capture completed after {} seconds", safe_duration);
                    break;
                }
                _ = tokio::signal::ctrl_c() => {
                    debug!("FFmpeg capture interrupted by signal");
                    break;
                }
            }

            if std::time::Instant::now() >= end_time {
                break;
            }
        }

        drop(handle);
        Ok(())
    }

    #[allow(dead_code)]
    async fn run_rtsp_capture(&self, config: &Value, stream_id: &str, user_id: &str) -> Result<()> {
        let rtsp_url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("RTSP stream config missing 'url' field".to_string()))?;

        // Use FFmpeg to handle RTSP with optimized settings
        let mut ffmpeg_config = FfmpegConfig {
            input_url: rtsp_url.to_string(),
            rtsp_transport: RtspTransport::Tcp, // Default to TCP for reliability
            ..Default::default()
        };

        // Parse optional RTSP-specific configuration
        if let Some(transport) = config.get("transport").and_then(|v| v.as_str()) {
            ffmpeg_config.rtsp_transport = match transport.to_lowercase().as_str() {
                "tcp" => RtspTransport::Tcp,
                "udp" => RtspTransport::Udp,
                _ => RtspTransport::Auto,
            };
        }

        // Parse optional timeout (RTSP streams often need longer timeouts)
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        } else {
            // Default timeout for RTSP
            ffmpeg_config.timeout = Some(10);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;

        // Setup snapshot capture loop
        let duration = config.get("duration").and_then(|v| v.as_u64()).unwrap_or(0); // Default to continuous for RTSP
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5); // Default 5 seconds between snapshots

        let safe_duration = std::cmp::min(duration, u32::MAX as u64) as u32;
        let mut snapshot_timer =
            tokio::time::interval(std::time::Duration::from_secs(snapshot_interval));
        let end_time =
            std::time::Instant::now() + std::time::Duration::from_secs(safe_duration as u64);

        loop {
            snapshot_timer.tick().await;

            match handle.snapshot().await {
                Ok(jpeg_bytes) => {
                    if let Err(e) = self.store_snapshot(stream_id, user_id, &jpeg_bytes).await {
                        warn!("Failed to store RTSP snapshot: {}", e);
                    }
                }
                Err(e) => {
                    warn!("Failed to capture RTSP snapshot: {}", e);
                }
            }

            // For RTSP, continue indefinitely if duration is 0
            if safe_duration > 0 && std::time::Instant::now() >= end_time {
                break;
            }
        }

        drop(handle);
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "website")]
    async fn run_website_capture(
        &self,
        config: &Value,
        stream_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
            Error::Config("Website stream config missing 'url' field".to_string())
        })?;

        let mut website_config = WebsiteConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields with safe casting
        if let Some(headless) = config.get("headless").and_then(|v| v.as_bool()) {
            website_config.headless = headless;
        }

        if let Some(width) = config.get("width").and_then(|v| v.as_u64()) {
            website_config.width = std::cmp::min(width, u32::MAX as u64) as u32;
        }

        if let Some(height) = config.get("height").and_then(|v| v.as_u64()) {
            website_config.height = std::cmp::min(height, u32::MAX as u64) as u32;
        }

        // Extract element selector configuration
        if let Some(element_selector) = config.get("element_selector").and_then(|v| v.as_str()) {
            website_config.element_selector = Some(element_selector.to_string());
        }

        if let Some(selector_type) = config.get("selector_type").and_then(|v| v.as_str()) {
            website_config.selector_type = selector_type.to_string();
        }

        // Extract other optional fields
        if let Some(stealth) = config.get("stealth").and_then(|v| v.as_bool()) {
            website_config.stealth = stealth;
        }

        if let Some(basic_auth_username) =
            config.get("basic_auth_username").and_then(|v| v.as_str())
        {
            website_config.basic_auth_username = Some(basic_auth_username.to_string());
        }

        if let Some(basic_auth_password) =
            config.get("basic_auth_password").and_then(|v| v.as_str())
        {
            website_config.basic_auth_password = Some(basic_auth_password.to_string());
        }

        if let Some(timeout_secs) = config.get("timeout").and_then(|v| v.as_u64()) {
            website_config.timeout = std::time::Duration::from_secs(timeout_secs);
        }

        let client =
            gl_capture::website_source::HeadlessChromeClient::new_boxed().map_err(|e| {
                Error::Config(format!("Failed to create embedded Chrome client: {}", e))
            })?;
        let website_source = WebsiteSource::new(website_config, client);
        let handle = website_source.start().await?;

        // Setup snapshot capture loop
        let duration = config
            .get("duration")
            .and_then(|v| v.as_u64())
            .unwrap_or(600); // Default 10 minutes
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(10); // Default 10 seconds between snapshots for websites

        let safe_duration = std::cmp::min(duration, u32::MAX as u64) as u32;
        let mut snapshot_timer =
            tokio::time::interval(std::time::Duration::from_secs(snapshot_interval));
        let end_time =
            std::time::Instant::now() + std::time::Duration::from_secs(safe_duration as u64);

        // Run capture with snapshot loop
        loop {
            tokio::select! {
                _ = snapshot_timer.tick() => {
                    // Take snapshot
                    match handle.snapshot().await {
                        Ok(snapshot_data) => {
                            debug!("Website capture snapshot taken, {} bytes", snapshot_data.len());
                            if let Err(e) = self.store_snapshot(stream_id, user_id, &snapshot_data).await {
                                warn!("Failed to store website capture snapshot: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to take website capture snapshot: {}", e);
                        }
                    }
                }
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(end_time)) => {
                    debug!("Website capture completed after {} seconds", safe_duration);
                    break;
                }
                _ = tokio::signal::ctrl_c() => {
                    debug!("Website capture interrupted by signal");
                    break;
                }
            }

            if std::time::Instant::now() >= end_time {
                break;
            }
        }

        drop(handle);
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(not(feature = "website"))]
    async fn run_website_capture(
        &self,
        _config: &Value,
        _stream_id: &str,
        _user_id: &str,
    ) -> Result<()> {
        Err(Error::Config(
            "Website capture not enabled - compile with 'website' feature".to_string(),
        ))
    }

    #[allow(dead_code)]
    async fn run_yt_capture(&self, config: &Value, stream_id: &str, user_id: &str) -> Result<()> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("yt stream config missing 'url' field".to_string()))?;

        let mut ytdlp_config = YtDlpConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields
        if let Some(format) = config.get("format").and_then(|v| v.as_str()) {
            ytdlp_config.format = match format {
                "best" => OutputFormat::Best,
                "worst" => OutputFormat::Worst,
                _ => OutputFormat::Best,
            };
        }

        // Safe timeout handling
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ytdlp_config.timeout = Some(safe_timeout);
        }

        let ytdlp_source = YtDlpSource::new(ytdlp_config);
        let handle = ytdlp_source.start().await?;

        // Setup snapshot capture loop
        let duration = config
            .get("duration")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600); // Default 1 hour
        let snapshot_interval = config
            .get("snapshot_interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(30); // Default 30 seconds between snapshots for YouTube

        let mut snapshot_timer =
            tokio::time::interval(std::time::Duration::from_secs(snapshot_interval));

        // Run capture with snapshot loop
        if duration == 0 {
            // Infinite duration
            loop {
                tokio::select! {
                    _ = snapshot_timer.tick() => {
                        // Take snapshot
                        match handle.snapshot().await {
                            Ok(snapshot_data) => {
                                debug!("YouTube capture snapshot taken, {} bytes", snapshot_data.len());
                                if let Err(e) = self.store_snapshot(stream_id, user_id, &snapshot_data).await {
                                    warn!("Failed to store YouTube capture snapshot: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to take YouTube capture snapshot: {}", e);
                            }
                        }
                    }
                    _ = tokio::signal::ctrl_c() => {
                        debug!("YouTube capture interrupted by signal");
                        break;
                    }
                }
            }
        } else {
            // Fixed duration
            let safe_duration = std::cmp::min(duration, u32::MAX as u64) as u32;
            let end_time =
                std::time::Instant::now() + std::time::Duration::from_secs(safe_duration as u64);

            loop {
                tokio::select! {
                    _ = snapshot_timer.tick() => {
                        // Take snapshot
                        match handle.snapshot().await {
                            Ok(snapshot_data) => {
                                debug!("YouTube capture snapshot taken, {} bytes", snapshot_data.len());
                                if let Err(e) = self.store_snapshot(stream_id, user_id, &snapshot_data).await {
                                    warn!("Failed to store YouTube capture snapshot: {}", e);
                                }
                            }
                            Err(e) => {
                                warn!("Failed to take YouTube capture snapshot: {}", e);
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(end_time)) => {
                        debug!("YouTube capture completed after {} seconds", safe_duration);
                        break;
                    }
                    _ = tokio::signal::ctrl_c() => {
                        debug!("YouTube capture interrupted by signal");
                        break;
                    }
                }

                if std::time::Instant::now() >= end_time {
                    break;
                }
            }
        }

        drop(handle);
        Ok(())
    }

    /// Take a snapshot from a file source
    async fn take_file_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let file_path = config
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("File stream config missing 'file_path' field".to_string())
            })?;

        let source_path = PathBuf::from(file_path);
        if source_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Config("Path traversal not allowed".to_string()));
        }

        let file_source = FileSource::new(&source_path);
        let handle = file_source.start().await?;
        let snapshot = handle.snapshot().await?;
        drop(handle);
        Ok(snapshot)
    }

    /// Take a snapshot from an FFmpeg source
    async fn take_ffmpeg_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let source_url = config
            .get("source_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("FFmpeg stream config missing 'source_url' field".to_string())
            })?;

        let mut ffmpeg_config = FfmpegConfig {
            input_url: source_url.to_string(),
            ..Default::default()
        };

        // Parse optional configuration
        if let Some(hw_accel) = config.get("hardware_accel").and_then(|v| v.as_str()) {
            ffmpeg_config.hardware_accel = match hw_accel.to_lowercase().as_str() {
                "vaapi" => HardwareAccel::Vaapi,
                "cuda" => HardwareAccel::Cuda,
                "qsv" => HardwareAccel::Qsv,
                "videotoolbox" => HardwareAccel::VideoToolbox,
                _ => HardwareAccel::None,
            };
        }

        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;
        let snapshot = handle.snapshot().await?;
        drop(handle);
        Ok(snapshot)
    }

    /// Take a snapshot from an RTSP source
    async fn take_rtsp_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let rtsp_url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("RTSP stream config missing 'url' field".to_string()))?;

        // Use FFmpeg to handle RTSP with optimized settings
        let mut ffmpeg_config = FfmpegConfig {
            input_url: rtsp_url.to_string(),
            rtsp_transport: RtspTransport::Tcp, // Default to TCP for reliability
            ..Default::default()
        };

        // Parse optional RTSP-specific configuration
        if let Some(transport) = config.get("transport").and_then(|v| v.as_str()) {
            ffmpeg_config.rtsp_transport = match transport.to_lowercase().as_str() {
                "tcp" => RtspTransport::Tcp,
                "udp" => RtspTransport::Udp,
                _ => RtspTransport::Auto,
            };
        }

        // Parse optional timeout (RTSP streams often need longer timeouts)
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        } else {
            // Default timeout for RTSP
            ffmpeg_config.timeout = Some(10);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;
        let snapshot = handle.snapshot().await?;
        drop(handle);
        Ok(snapshot)
    }

    /// Take a snapshot from a website source
    #[cfg(feature = "website")]
    async fn take_website_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
            Error::Config("Website stream config missing 'url' field".to_string())
        })?;

        let mut website_config = WebsiteConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields
        if let Some(headless) = config.get("headless").and_then(|v| v.as_bool()) {
            website_config.headless = headless;
        }

        if let Some(width) = config.get("width").and_then(|v| v.as_u64()) {
            website_config.width = std::cmp::min(width, u32::MAX as u64) as u32;
        }

        if let Some(height) = config.get("height").and_then(|v| v.as_u64()) {
            website_config.height = std::cmp::min(height, u32::MAX as u64) as u32;
        }

        // Extract element selector configuration
        if let Some(element_selector) = config.get("element_selector").and_then(|v| v.as_str()) {
            website_config.element_selector = Some(element_selector.to_string());
        }

        if let Some(selector_type) = config.get("selector_type").and_then(|v| v.as_str()) {
            website_config.selector_type = selector_type.to_string();
        }

        // Extract other optional fields
        if let Some(stealth) = config.get("stealth").and_then(|v| v.as_bool()) {
            website_config.stealth = stealth;
        }

        if let Some(basic_auth_username) =
            config.get("basic_auth_username").and_then(|v| v.as_str())
        {
            website_config.basic_auth_username = Some(basic_auth_username.to_string());
        }

        if let Some(basic_auth_password) =
            config.get("basic_auth_password").and_then(|v| v.as_str())
        {
            website_config.basic_auth_password = Some(basic_auth_password.to_string());
        }

        if let Some(timeout_secs) = config.get("timeout").and_then(|v| v.as_u64()) {
            website_config.timeout = std::time::Duration::from_secs(timeout_secs);
        }

        let client =
            gl_capture::website_source::HeadlessChromeClient::new_boxed().map_err(|e| {
                Error::Config(format!("Failed to create embedded Chrome client: {}", e))
            })?;
        let website_source = WebsiteSource::new(website_config, client);
        let handle = website_source.start().await?;
        let snapshot = handle.snapshot().await?;
        drop(handle);
        Ok(snapshot)
    }

    /// Take a snapshot from a website source (stub when website feature disabled)
    #[cfg(not(feature = "website"))]
    async fn take_website_snapshot(_config: &Value) -> Result<bytes::Bytes> {
        Err(Error::Config(
            "Website capture not enabled - compile with 'website' feature".to_string(),
        ))
    }

    /// Take a snapshot from a YouTube/yt-dlp source
    async fn take_yt_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("yt stream config missing 'url' field".to_string()))?;

        let mut ytdlp_config = YtDlpConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields
        if let Some(format) = config.get("format").and_then(|v| v.as_str()) {
            ytdlp_config.format = match format {
                "best" => OutputFormat::Best,
                "worst" => OutputFormat::Worst,
                _ => OutputFormat::Best,
            };
        }

        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ytdlp_config.timeout = Some(safe_timeout);
        }

        let ytdlp_source = YtDlpSource::new(ytdlp_config);
        let handle = ytdlp_source.start().await?;
        let snapshot = handle.snapshot().await?;
        drop(handle);
        Ok(snapshot)
    }

    /// Create file capture source
    async fn create_file_capture(config: &Value) -> Result<Arc<CaptureHandle>> {
        let file_path = config
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("File stream config missing 'file_path' field".to_string())
            })?;

        // Validate file path for security (prevent path traversal)
        let source_path = PathBuf::from(file_path);
        if source_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Config("Path traversal not allowed".to_string()));
        }

        let file_source = FileSource::new(&source_path);
        let handle = file_source.start().await?;
        Ok(Arc::new(handle))
    }

    /// Create FFmpeg capture source
    async fn create_ffmpeg_capture(config: &Value) -> Result<Arc<CaptureHandle>> {
        let source_url = config
            .get("source_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("FFmpeg stream config missing 'source_url' field".to_string())
            })?;

        let mut ffmpeg_config = FfmpegConfig {
            input_url: source_url.to_string(),
            ..Default::default()
        };

        // Parse optional configuration
        if let Some(hw_accel) = config.get("hardware_accel").and_then(|v| v.as_str()) {
            ffmpeg_config.hardware_accel = match hw_accel.to_lowercase().as_str() {
                "vaapi" => HardwareAccel::Vaapi,
                "cuda" => HardwareAccel::Cuda,
                "qsv" => HardwareAccel::Qsv,
                "videotoolbox" => HardwareAccel::VideoToolbox,
                _ => HardwareAccel::None,
            };
        }

        // Safe timeout handling - prevent overflow
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;
        Ok(Arc::new(handle))
    }

    /// Create RTSP capture source
    async fn create_rtsp_capture(config: &Value) -> Result<Arc<CaptureHandle>> {
        let rtsp_url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("RTSP stream config missing 'url' field".to_string()))?;

        // Use FFmpeg to handle RTSP with optimized settings
        let mut ffmpeg_config = FfmpegConfig {
            input_url: rtsp_url.to_string(),
            rtsp_transport: RtspTransport::Tcp, // Default to TCP for reliability
            ..Default::default()
        };

        // Parse optional RTSP-specific configuration
        if let Some(transport) = config.get("transport").and_then(|v| v.as_str()) {
            ffmpeg_config.rtsp_transport = match transport.to_lowercase().as_str() {
                "tcp" => RtspTransport::Tcp,
                "udp" => RtspTransport::Udp,
                _ => RtspTransport::Auto,
            };
        }

        // Parse optional timeout (RTSP streams often need longer timeouts)
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ffmpeg_config.timeout = Some(safe_timeout);
        } else {
            // Default timeout for RTSP
            ffmpeg_config.timeout = Some(10);
        }

        let ffmpeg_source = FfmpegSource::new(ffmpeg_config);
        let handle = ffmpeg_source.start().await?;
        Ok(Arc::new(handle))
    }

    /// Create website capture source
    #[cfg(feature = "website")]
    async fn create_website_capture(config: &Value) -> Result<Arc<CaptureHandle>> {
        let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
            Error::Config("Website stream config missing 'url' field".to_string())
        })?;

        let mut website_config = WebsiteConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields with safe casting
        if let Some(headless) = config.get("headless").and_then(|v| v.as_bool()) {
            website_config.headless = headless;
        }

        if let Some(width) = config.get("width").and_then(|v| v.as_u64()) {
            website_config.width = std::cmp::min(width, u32::MAX as u64) as u32;
        }

        if let Some(height) = config.get("height").and_then(|v| v.as_u64()) {
            website_config.height = std::cmp::min(height, u32::MAX as u64) as u32;
        }

        // Extract element selector configuration
        if let Some(element_selector) = config.get("element_selector").and_then(|v| v.as_str()) {
            website_config.element_selector = Some(element_selector.to_string());
        }

        if let Some(selector_type) = config.get("selector_type").and_then(|v| v.as_str()) {
            website_config.selector_type = selector_type.to_string();
        }

        // Extract other optional fields
        if let Some(stealth) = config.get("stealth").and_then(|v| v.as_bool()) {
            website_config.stealth = stealth;
        }

        if let Some(basic_auth_username) =
            config.get("basic_auth_username").and_then(|v| v.as_str())
        {
            website_config.basic_auth_username = Some(basic_auth_username.to_string());
        }

        if let Some(basic_auth_password) =
            config.get("basic_auth_password").and_then(|v| v.as_str())
        {
            website_config.basic_auth_password = Some(basic_auth_password.to_string());
        }

        if let Some(timeout_secs) = config.get("timeout").and_then(|v| v.as_u64()) {
            website_config.timeout = std::time::Duration::from_secs(timeout_secs);
        }

        let client =
            gl_capture::website_source::HeadlessChromeClient::new_boxed().map_err(|e| {
                Error::Config(format!("Failed to create embedded Chrome client: {}", e))
            })?;
        let website_source = WebsiteSource::new(website_config, client);
        let handle = website_source.start().await?;
        Ok(Arc::new(handle))
    }

    /// Create website capture source (stub when website feature disabled)
    #[cfg(not(feature = "website"))]
    async fn create_website_capture(_config: &Value) -> Result<Arc<CaptureHandle>> {
        Err(Error::Config(
            "Website capture not enabled - compile with 'website' feature".to_string(),
        ))
    }

    /// Create YouTube capture source
    async fn create_yt_capture(config: &Value) -> Result<Arc<CaptureHandle>> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("yt stream config missing 'url' field".to_string()))?;

        let mut ytdlp_config = YtDlpConfig {
            url: url.to_string(),
            ..Default::default()
        };

        // Parse optional fields
        if let Some(format) = config.get("format").and_then(|v| v.as_str()) {
            ytdlp_config.format = match format {
                "best" => OutputFormat::Best,
                "worst" => OutputFormat::Worst,
                _ => OutputFormat::Best,
            };
        }

        // Safe timeout handling
        if let Some(timeout_val) = config.get("timeout").and_then(|v| v.as_u64()) {
            let safe_timeout = std::cmp::min(timeout_val, u32::MAX as u64) as u32;
            ytdlp_config.timeout = Some(safe_timeout);
        }

        let ytdlp_source = YtDlpSource::new(ytdlp_config);
        let handle = ytdlp_source.start().await?;
        Ok(Arc::new(handle))
    }

    /// Store snapshot asynchronously (static method for spawned tasks)
    async fn store_snapshot_async(
        storage_service: &ArtifactStorageService<StorageManager>,
        db_pool: &sqlx::SqlitePool,
        stream_id: &str,
        user_id: &str,
        snapshot_data: &[u8],
    ) -> Result<()> {
        let snapshot_bytes = Bytes::from(snapshot_data.to_vec());

        // Store the snapshot file using ArtifactStorageService
        let stored_artifact = storage_service
            .store_snapshot(stream_id, snapshot_bytes)
            .await?;

        // Extract file path from storage URI for database
        let file_path = stored_artifact.uri.path().unwrap_or_default().to_string();

        // Create database record with file path reference
        let snapshot_repo = SnapshotRepository::new(db_pool);
        let request = CreateSnapshotRequest {
            stream_id: stream_id.to_string(),
            user_id: user_id.to_string(),
            file_path,
            storage_uri: stored_artifact.uri.to_string(),
            content_type: stored_artifact.content_type,
            width: None,
            height: None,
            file_size: stored_artifact.size as i64,
            checksum: stored_artifact.checksum,
            etag: stored_artifact.etag,
            captured_at: now_iso8601(),
        };

        match snapshot_repo.create(request).await {
            Ok(snapshot) => {
                debug!(
                    "Stored snapshot {} for stream {} at {}",
                    snapshot.id, stream_id, stored_artifact.uri
                );

                // Clean up old snapshots to prevent disk space bloat
                if let Err(e) = snapshot_repo.cleanup_old_snapshots(stream_id, 20).await {
                    warn!(
                        stream_id = %stream_id,
                        error = %e,
                        "Failed to cleanup old snapshots"
                    );
                }

                Ok(())
            }
            Err(e) => {
                error!("Failed to store snapshot for stream {}: {}", stream_id, e);

                // Try to clean up the stored file on database error
                if let Err(cleanup_error) =
                    storage_service.delete_artifact(&stored_artifact.uri).await
                {
                    warn!(
                        "Failed to clean up stored artifact after database error: {}",
                        cleanup_error
                    );
                }

                Err(e)
            }
        }
    }

    /// Store snapshot using ArtifactStorageService and update database
    #[allow(dead_code)]
    async fn store_snapshot(
        &self,
        stream_id: &str,
        user_id: &str,
        snapshot_data: &[u8],
    ) -> Result<()> {
        // Create storage service for this operation
        let artifacts_dir = PathBuf::from(&self.storage_config.artifacts_dir);
        let gl_storage_config = gl_storage::StorageConfig {
            base_dir: Some(artifacts_dir),
            ..Default::default()
        };
        let storage_manager =
            StorageManager::new(gl_storage_config).expect("Failed to create storage manager");
        let artifact_config = ArtifactStorageConfig {
            base_uri: format!("file://{}", self.storage_config.artifacts_dir),
            snapshot_extension: "jpg".to_string(),
            include_timestamp: true,
        };
        let storage_service = ArtifactStorageService::new(storage_manager, artifact_config);

        let snapshot_bytes = Bytes::from(snapshot_data.to_vec());

        // Store the snapshot file using ArtifactStorageService
        let stored_artifact = storage_service
            .store_snapshot(stream_id, snapshot_bytes)
            .await?;

        // Extract file path from storage URI for database
        let file_path = stored_artifact.uri.path().unwrap_or_default().to_string();

        // Create database record with file path reference
        let snapshot_repo = SnapshotRepository::new(&self.db_pool);
        let request = CreateSnapshotRequest {
            stream_id: stream_id.to_string(),
            user_id: user_id.to_string(),
            file_path,
            storage_uri: stored_artifact.uri.to_string(),
            content_type: stored_artifact.content_type,
            width: None,
            height: None,
            file_size: stored_artifact.size as i64,
            checksum: stored_artifact.checksum,
            etag: stored_artifact.etag,
            captured_at: now_iso8601(),
        };

        match snapshot_repo.create(request).await {
            Ok(snapshot) => {
                debug!(
                    "Stored snapshot {} for stream {} at {}",
                    snapshot.id, stream_id, stored_artifact.uri
                );
                Ok(())
            }
            Err(e) => {
                error!("Failed to store snapshot for stream {}: {}", stream_id, e);

                // Try to clean up the stored file on database error
                if let Err(cleanup_error) =
                    storage_service.delete_artifact(&stored_artifact.uri).await
                {
                    warn!(
                        "Failed to clean up stored artifact after database error: {}",
                        cleanup_error
                    );
                }

                Err(e)
            }
        }
    }
}

impl Drop for CaptureManager {
    fn drop(&mut self) {
        // Cancel all running tasks when the manager is dropped
        // Use only synchronous operations since Drop must be sync
        match self.running_captures.try_write() {
            Ok(mut captures) => {
                for (stream_id, task) in captures.drain() {
                    debug!(stream_id = %stream_id, "Aborting capture task during manager drop");
                    task.handle.abort();
                }
                debug!("All capture tasks aborted during CaptureManager drop");
            }
            Err(_) => {
                // If we can't get the lock, we can't safely clean up
                // This could happen during shutdown races, but the tasks will be
                // cleaned up when the runtime shuts down anyway
                warn!("Failed to acquire write lock during CaptureManager drop - tasks will be cleaned up by runtime shutdown");
            }
        }
    }
}
