//! ABOUTME: Central service for managing capture processes and their lifecycle
//! ABOUTME: Coordinates template execution, status tracking, and resource management

use bytes::Bytes;
use gl_capture::{
    artifact_storage::{ArtifactStorageConfig, ArtifactStorageService},
    CaptureHandle, CaptureSource, FfmpegConfig, FfmpegSource, FileSource, HardwareAccel,
    OutputFormat, YtDlpConfig, YtDlpSource,
};
#[cfg(feature = "website")]
use gl_capture::{WebsiteConfig, WebsiteSource};
use gl_core::{time::now_iso8601, Error, Result};
use gl_db::{CreateSnapshotRequest, SnapshotRepository, Stream, StreamRepository};
use gl_storage::StorageManager;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
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
    pub template_id: String,
    pub status: CaptureStatus,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_frame_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Handle for a running capture task
struct CaptureTask {
    handle: JoinHandle<()>,
    info: CaptureInfo,
    capture_handle: Option<Arc<CaptureHandle>>,
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
            .finish()
    }
}

/// Central manager for capture processes
pub struct CaptureManager {
    db_pool: sqlx::SqlitePool,
    running_captures: Arc<RwLock<HashMap<String, CaptureTask>>>,
    storage_service: ArtifactStorageService<StorageManager>,
}

impl CaptureManager {
    /// Create a new CaptureManager
    pub fn new(db_pool: sqlx::SqlitePool) -> Self {
        // Use default storage configuration (backward compatibility)
        let storage_config = gl_config::StorageConfig::default();
        Self::with_storage_config(db_pool, storage_config)
    }

    /// Create a new CaptureManager with custom storage configuration
    pub fn with_storage_config(
        db_pool: sqlx::SqlitePool,
        storage_config: gl_config::StorageConfig,
    ) -> Self {
        // Create storage configuration for filesystem storage
        let artifacts_dir = PathBuf::from(&storage_config.artifacts_dir);

        // Create the artifacts directory if it doesn't exist
        if !artifacts_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&artifacts_dir) {
                warn!("Failed to create artifacts directory: {}", e);
            }
        }

        let gl_storage_config = gl_storage::StorageConfig {
            base_dir: Some(artifacts_dir),
            ..Default::default()
        };

        let storage_manager =
            StorageManager::new(gl_storage_config).expect("Failed to create storage manager");

        let artifact_config = ArtifactStorageConfig {
            base_uri: format!("file://{}", storage_config.artifacts_dir),
            snapshot_extension: "jpg".to_string(),
            include_timestamp: true,
        };

        let storage_service = ArtifactStorageService::new(storage_manager, artifact_config);

        Self {
            db_pool,
            running_captures: Arc::new(RwLock::new(HashMap::new())),
            storage_service,
        }
    }

    /// Start a capture from a template
    pub async fn start_template(&self, template_id: &str) -> Result<()> {
        info!(template_id = %template_id, "Starting template capture");

        // Get template from database first
        let template_repo = StreamRepository::new(&self.db_pool);
        let template = template_repo
            .find_by_id(template_id)
            .await?
            .ok_or_else(|| Error::NotFound(format!("Template {} not found", template_id)))?;

        // Use single write lock to atomically check and insert
        let mut captures = self.running_captures.write().await;

        // Check if already running under write lock
        if captures.contains_key(template_id) {
            warn!(template_id = %template_id, "Template capture already running");
            return Err(Error::Config(
                "Template capture already running".to_string(),
            ));
        }

        // Update status to starting with execution timestamp
        template_repo
            .update_execution_status(
                template_id,
                "starting",
                Some(&chrono::Utc::now().to_rfc3339()),
            )
            .await?;

        // Create capture info
        let capture_info = CaptureInfo {
            template_id: template_id.to_string(),
            status: CaptureStatus::Starting,
            started_at: Some(chrono::Utc::now()),
            last_frame_at: None,
        };

        // Start the capture task
        let db_pool = self.db_pool.clone();
        let template_clone = template.clone();
        let running_captures = self.running_captures.clone();
        let template_id_clone = template_id.to_string();

        let handle = tokio::spawn(async move {
            let result =
                Self::run_capture_task(db_pool.clone(), template_clone, template_id_clone.clone())
                    .await;

            // Always remove from running captures when task completes
            {
                let mut captures = running_captures.write().await;
                captures.remove(&template_id_clone);
            }

            // Update database status based on result
            let template_repo = StreamRepository::new(&db_pool);
            match result {
                Ok(_) => {
                    if let Err(e) = template_repo
                        .update_execution_status(&template_id_clone, "inactive", None)
                        .await
                    {
                        error!(template_id = %template_id_clone, error = %e, "Failed to update completion status");
                    }
                }
                Err(e) => {
                    error!(template_id = %template_id_clone, error = %e, "Capture task failed");
                    let error_msg = format!("Capture failed: {}", e);
                    if let Err(update_err) = template_repo
                        .update_execution_status_with_error(&template_id_clone, "error", &error_msg)
                        .await
                    {
                        error!(template_id = %template_id_clone, error = %update_err, "Failed to update error status");
                    }
                }
            }
        });

        // Store the task under the existing write lock
        let task = CaptureTask {
            handle,
            info: capture_info,
            capture_handle: None, // TODO: Store actual capture handle for snapshot access
        };

        captures.insert(template_id.to_string(), task);
        drop(captures); // Explicitly release lock

        debug!(template_id = %template_id, "Template capture started");
        Ok(())
    }

    /// Stop a running capture
    pub async fn stop_template(&self, template_id: &str) -> Result<()> {
        info!(template_id = %template_id, "Stopping template capture");

        let task = {
            let mut captures = self.running_captures.write().await;
            captures.remove(template_id)
        };

        if let Some(task) = task {
            // Update status to stopping (no timestamp update needed)
            let template_repo = StreamRepository::new(&self.db_pool);
            template_repo
                .update_execution_status(template_id, "stopping", None)
                .await?;

            // Abort the task and wait briefly for it to finish
            task.handle.abort();

            // Give the task a moment to cleanup
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Update status to inactive
            template_repo
                .update_execution_status(template_id, "inactive", None)
                .await?;

            debug!(template_id = %template_id, "Template capture stopped");
            Ok(())
        } else {
            warn!(template_id = %template_id, "Template capture not running");
            Err(Error::NotFound("Template capture not running".to_string()))
        }
    }

    /// Get status of a capture
    pub async fn get_capture_status(&self, template_id: &str) -> Option<CaptureInfo> {
        let captures = self.running_captures.read().await;
        captures.get(template_id).map(|task| task.info.clone())
    }

    /// Get all running captures
    pub async fn get_all_captures(&self) -> Vec<CaptureInfo> {
        let captures = self.running_captures.read().await;
        captures.values().map(|task| task.info.clone()).collect()
    }

    /// Check if a template is currently running
    pub async fn is_template_running(&self, template_id: &str) -> bool {
        let captures = self.running_captures.read().await;
        captures.contains_key(template_id)
    }

    /// Take a snapshot from a running template
    /// For now, this creates a temporary capture to get a snapshot
    /// TODO: In production, store capture handles in CaptureTask for direct access
    pub async fn take_template_snapshot(&self, template_id: &str) -> Result<bytes::Bytes> {
        // Check if template is running
        if !self.is_template_running(template_id).await {
            return Err(Error::NotFound(
                "Template is not currently running".to_string(),
            ));
        }

        // Get template from database to recreate capture source
        let template_repo = StreamRepository::new(&self.db_pool);
        let template = template_repo
            .find_by_id(template_id)
            .await?
            .ok_or_else(|| Error::NotFound("Template not found".to_string()))?;

        // Parse template config
        let config: Value = serde_json::from_str(&template.config)
            .map_err(|e| Error::Config(format!("Invalid template config JSON: {}", e)))?;

        let kind = config
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Template config missing 'kind' field".to_string()))?;

        // Create a temporary capture source to get snapshot
        match kind {
            "file" => Self::take_file_snapshot(&config).await,
            "ffmpeg" => Self::take_ffmpeg_snapshot(&config).await,
            "website" => Self::take_website_snapshot(&config).await,
            "yt" => Self::take_yt_snapshot(&config).await,
            _ => Err(Error::Config(format!(
                "Unsupported template kind: {}",
                kind
            ))),
        }
    }

    /// Internal method to run a capture task
    async fn run_capture_task(
        db_pool: sqlx::SqlitePool,
        template: Stream,
        template_id: String,
    ) -> Result<()> {
        info!(template_id = %template_id, "Running capture task");

        // Parse template config
        let config: Value = serde_json::from_str(&template.config)
            .map_err(|e| Error::Config(format!("Invalid template config JSON: {}", e)))?;

        let kind = config
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Template config missing 'kind' field".to_string()))?;

        // Update status to active with timestamp
        let template_repo = StreamRepository::new(&db_pool);
        template_repo
            .update_execution_status(
                &template_id,
                "active",
                Some(&chrono::Utc::now().to_rfc3339()),
            )
            .await?;

        // Create and start the capture source based on template type
        let capture_manager = CaptureManager::new(db_pool);
        match kind {
            "file" => {
                capture_manager
                    .run_file_capture(&config, &template_id, &template.user_id)
                    .await
            }
            "ffmpeg" => {
                capture_manager
                    .run_ffmpeg_capture(&config, &template_id, &template.user_id)
                    .await
            }
            "website" => {
                capture_manager
                    .run_website_capture(&config, &template_id, &template.user_id)
                    .await
            }
            "yt" => {
                capture_manager
                    .run_yt_capture(&config, &template_id, &template.user_id)
                    .await
            }
            _ => Err(Error::Config(format!(
                "Unsupported template kind: {}",
                kind
            ))),
        }
    }

    async fn run_file_capture(
        &self,
        config: &Value,
        template_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let file_path = config
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("File template config missing 'file_path' field".to_string())
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
                            if let Err(e) = self.store_snapshot(template_id, user_id, &snapshot_data).await {
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

    async fn run_ffmpeg_capture(
        &self,
        config: &Value,
        template_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let source_url = config
            .get("source_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("FFmpeg template config missing 'source_url' field".to_string())
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
                            if let Err(e) = self.store_snapshot(template_id, user_id, &snapshot_data).await {
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

    #[cfg(feature = "website")]
    async fn run_website_capture(
        &self,
        config: &Value,
        template_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
            Error::Config("Website template config missing 'url' field".to_string())
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
                            if let Err(e) = self.store_snapshot(template_id, user_id, &snapshot_data).await {
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

    #[cfg(not(feature = "website"))]
    async fn run_website_capture(
        &self,
        _config: &Value,
        _template_id: &str,
        _user_id: &str,
    ) -> Result<()> {
        Err(Error::Config(
            "Website capture not enabled - compile with 'website' feature".to_string(),
        ))
    }

    async fn run_yt_capture(&self, config: &Value, template_id: &str, user_id: &str) -> Result<()> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("yt template config missing 'url' field".to_string()))?;

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
                            debug!("YouTube capture snapshot taken, {} bytes", snapshot_data.len());
                            if let Err(e) = self.store_snapshot(template_id, user_id, &snapshot_data).await {
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

        drop(handle);
        Ok(())
    }

    /// Take a snapshot from a file source
    async fn take_file_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let file_path = config
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Config("File template config missing 'file_path' field".to_string())
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
                Error::Config("FFmpeg template config missing 'source_url' field".to_string())
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

    /// Take a snapshot from a website source
    #[cfg(feature = "website")]
    async fn take_website_snapshot(config: &Value) -> Result<bytes::Bytes> {
        let url = config.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
            Error::Config("Website template config missing 'url' field".to_string())
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
            .ok_or_else(|| Error::Config("yt template config missing 'url' field".to_string()))?;

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

    /// Store snapshot using ArtifactStorageService and update database
    async fn store_snapshot(
        &self,
        template_id: &str,
        user_id: &str,
        snapshot_data: &[u8],
    ) -> Result<()> {
        let snapshot_bytes = Bytes::from(snapshot_data.to_vec());

        // Store the snapshot file using ArtifactStorageService
        let stored_artifact = self
            .storage_service
            .store_snapshot(template_id, snapshot_bytes)
            .await?;

        // Extract file path from storage URI for database
        let file_path = stored_artifact.uri.path().unwrap_or_default().to_string();

        // Create database record with file path reference
        let snapshot_repo = SnapshotRepository::new(&self.db_pool);
        let request = CreateSnapshotRequest {
            template_id: template_id.to_string(),
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
                    "Stored snapshot {} for template {} at {}",
                    snapshot.id, template_id, stored_artifact.uri
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    "Failed to store snapshot for template {}: {}",
                    template_id, e
                );

                // Try to clean up the stored file on database error
                if let Err(cleanup_error) = self
                    .storage_service
                    .delete_artifact(&stored_artifact.uri)
                    .await
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
                for (template_id, task) in captures.drain() {
                    debug!(template_id = %template_id, "Aborting capture task during manager drop");
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
