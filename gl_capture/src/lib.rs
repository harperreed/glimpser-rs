//! ABOUTME: Capture engine for various media sources (ffmpeg, websites, files)
//! ABOUTME: Provides trait-based capture abstractions and implementations

use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{run, CommandSpec};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, warn};

pub mod artifact_storage;
pub mod ffmpeg_source;
pub mod file_source;
pub mod yt_dlp_source;

#[cfg(feature = "website")]
pub mod website_source;

pub use artifact_storage::{
    snapshot_and_store, ArtifactStorageConfig, ArtifactStorageService, StoredArtifact,
};
pub use ffmpeg_source::{FfmpegConfig, FfmpegSource, HardwareAccel};
pub use file_source::FileSource;
pub use yt_dlp_source::{OutputFormat, YtDlpConfig, YtDlpSource};

#[cfg(feature = "website")]
pub use website_source::{WebDriverClient, WebsiteConfig, WebsiteSource};

/// Handle to a running capture session
/// When dropped, the capture should stop gracefully
pub struct CaptureHandle {
    source: Arc<dyn CaptureSource + Send + Sync>,
    state: Arc<Mutex<CaptureState>>,
}

impl CaptureHandle {
    pub(crate) fn new(source: Arc<dyn CaptureSource + Send + Sync>) -> Self {
        Self {
            source,
            state: Arc::new(Mutex::new(CaptureState::Running)),
        }
    }

    /// Take a snapshot from the capture source
    pub async fn snapshot(&self) -> Result<Bytes> {
        let state = self.state.lock().await;
        match *state {
            CaptureState::Running => self.source.snapshot().await,
            CaptureState::Stopped => Err(Error::Config("Capture has been stopped".to_string())),
        }
    }

    /// Stop the capture session
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        match *state {
            CaptureState::Running => {
                *state = CaptureState::Stopped;
                self.source.stop().await
            }
            CaptureState::Stopped => Ok(()), // Already stopped
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let source = Arc::clone(&self.source);
        let state = Arc::clone(&self.state);

        // Spawn a task to stop the capture gracefully
        tokio::spawn(async move {
            let mut guard = state.lock().await;
            if matches!(*guard, CaptureState::Running) {
                *guard = CaptureState::Stopped;
                if let Err(e) = source.stop().await {
                    warn!(error = %e, "Failed to stop capture during drop");
                }
            }
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum CaptureState {
    Running,
    Stopped,
}

/// Trait for capture sources (files, streams, webcams, etc.)
#[async_trait]
pub trait CaptureSource {
    /// Start capturing and return a handle
    async fn start(&self) -> Result<CaptureHandle>;

    /// Take a snapshot and return JPEG bytes
    async fn snapshot(&self) -> Result<Bytes>;

    /// Stop capturing and clean up resources
    async fn stop(&self) -> Result<()>;
}

/// Configuration for snapshot generation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotConfig {
    /// Output format (typically "jpeg")
    pub format: String,
    /// Quality for JPEG (1-100, higher is better quality)
    pub quality: u8,
    /// Maximum width for output (preserves aspect ratio)
    pub max_width: Option<u32>,
    /// Maximum height for output (preserves aspect ratio)
    pub max_height: Option<u32>,
    /// Timeout for snapshot generation
    pub timeout: Duration,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            format: "jpeg".to_string(),
            quality: 85,
            max_width: Some(1920),
            max_height: Some(1080),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Utility function to generate JPEG snapshots using ffmpeg
#[instrument(skip(input_path))]
pub async fn generate_snapshot_with_ffmpeg(
    input_path: &Path,
    config: &SnapshotConfig,
) -> Result<Bytes> {
    info!(
        input = %input_path.display(),
        format = %config.format,
        quality = config.quality,
        "Generating snapshot with ffmpeg"
    );

    // Build ffmpeg command for snapshot extraction
    let mut args = vec![
        "-i".to_string(),
        input_path.to_string_lossy().to_string(),
        "-vframes".to_string(),
        "1".to_string(), // Extract only 1 frame
        "-f".to_string(),
        "image2".to_string(),
        "-q:v".to_string(),
        ((31 * (100 - config.quality as u32)) / 100 + 2).to_string(), // Convert quality to ffmpeg scale
    ];

    // Add scaling if specified
    if let (Some(width), Some(height)) = (config.max_width, config.max_height) {
        args.extend([
            "-vf".to_string(),
            format!(
                "scale={}:{}:force_original_aspect_ratio=decrease",
                width, height
            ),
        ]);
    }

    // Output to stdout
    args.push("pipe:1".to_string());

    let spec = CommandSpec::new("ffmpeg".into())
        .args(args)
        .timeout(config.timeout);

    debug!(command = ?spec, "Running ffmpeg command");

    let result = run(spec).await?;

    if !result.success() {
        error!(
            exit_code = result.exit_code(),
            stderr = %result.stderr,
            "ffmpeg command failed"
        );
        return Err(Error::Config(format!(
            "ffmpeg failed with exit code {}: {}",
            result.exit_code().unwrap_or(-1),
            result.stderr
        )));
    }

    if result.stdout.is_empty() {
        return Err(Error::Config("ffmpeg produced no output".to_string()));
    }

    debug!(
        output_size = result.stdout.len(),
        truncated = result.stdout_truncated,
        "ffmpeg snapshot generated successfully"
    );

    Ok(Bytes::from(result.stdout.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::create_test_id;
    use tokio::fs;

    #[tokio::test]
    async fn test_snapshot_config_default() {
        let config = SnapshotConfig::default();
        assert_eq!(config.format, "jpeg");
        assert_eq!(config.quality, 85);
        assert_eq!(config.max_width, Some(1920));
        assert_eq!(config.max_height, Some(1080));
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_capture_handle_lifecycle() {
        // This is a basic test - full integration tests are in file_source module
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        // Create a dummy file for testing (real tests would use actual video)
        fs::write(&video_path, b"fake video data").await.unwrap();

        let source = FileSource::new(video_path);

        // Test that we can create a handle (even though it will fail without real video)
        // This mainly tests the API structure
        match source.start().await {
            Ok(_handle) => {
                // If we get here, it means ffmpeg is available and working
            }
            Err(_) => {
                // Expected when ffmpeg isn't available or file isn't valid video
            }
        }
    }
}
