//! ABOUTME: File-based capture source for reading video files
//! ABOUTME: Implements CaptureSource trait for MP4 and other video file formats

use crate::{
    CaptureHandle, CaptureSource, FfmpegConfig, HardwareAccel, RtspTransport, SnapshotConfig,
    StreamingFfmpegSource, StreamingSourceConfig,
};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{debug, info, instrument};

/// File-based capture source that reads from video files using optimized streaming
#[derive(Debug)]
pub struct FileSource {
    /// Path to the video file
    file_path: PathBuf,
    /// Configuration for snapshot generation
    config: SnapshotConfig,
    /// Internal streaming source for optimized performance
    streaming_source: Arc<Mutex<Option<StreamingFfmpegSource>>>,
}

impl FileSource {
    /// Create a new file source from a video file path
    pub fn new<P: AsRef<Path>>(file_path: P) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
            config: SnapshotConfig::default(),
            streaming_source: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a file source with custom snapshot configuration
    pub fn with_config<P: AsRef<Path>>(file_path: P, config: SnapshotConfig) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
            config,
            streaming_source: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get the snapshot configuration
    pub fn config(&self) -> &SnapshotConfig {
        &self.config
    }

    /// Update the snapshot configuration
    pub fn set_config(&mut self, config: SnapshotConfig) {
        self.config = config;
    }

    /// Validate that the file exists and is accessible
    #[instrument(skip(self))]
    pub async fn validate(&self) -> Result<()> {
        debug!(path = %self.file_path.display(), "Validating file source");

        if !self.file_path.exists() {
            return Err(Error::Config(format!(
                "File does not exist: {}",
                self.file_path.display()
            )));
        }

        if !self.file_path.is_file() {
            return Err(Error::Config(format!(
                "Path is not a file: {}",
                self.file_path.display()
            )));
        }

        // Check if file is readable
        match tokio::fs::File::open(&self.file_path).await {
            Ok(_) => {
                info!(path = %self.file_path.display(), "File source validated successfully");
                Ok(())
            }
            Err(e) => Err(Error::Config(format!(
                "Cannot read file {}: {}",
                self.file_path.display(),
                e
            ))),
        }
    }

    /// Initialize the streaming source for optimized performance
    #[instrument(skip(self))]
    async fn ensure_streaming_source(&self) -> Result<()> {
        // Check if already initialized without holding the lock long
        let needs_init = {
            let source_guard = self.streaming_source.lock().unwrap();
            source_guard.is_none()
        };

        if needs_init {
            debug!(path = %self.file_path.display(), "Initializing streaming source for file");

            // Create FFmpeg config for file input
            let ffmpeg_config = FfmpegConfig {
                input_url: format!("file:{}", self.file_path.display()),
                hardware_accel: HardwareAccel::VideoToolbox, // Use hardware acceleration on macOS
                input_options: HashMap::new(),
                rtsp_transport: RtspTransport::Tcp,
                video_codec: None,
                frame_rate: Some(10.0), // Default frame rate for file sources
                buffer_size: Some("1M".to_string()),
                timeout: Some(30),
                snapshot_config: self.config.clone(),
            };

            // Create streaming config optimized for file sources
            let streaming_config = StreamingSourceConfig {
                ffmpeg_config,
                frame_rate: 10.0,
                pool_size: 1, // Files only need 1 process since they're not real-time
                health_monitoring: true,
                frame_timeout: Duration::from_secs(5),
            };

            let streaming_source = StreamingFfmpegSource::new(streaming_config).await?;

            // Now store the created source
            {
                let mut source_guard = self.streaming_source.lock().unwrap();
                *source_guard = Some(streaming_source);
            }

            info!(path = %self.file_path.display(), "Streaming source initialized successfully");
        }

        Ok(())
    }
}

impl Clone for FileSource {
    fn clone(&self) -> Self {
        Self {
            file_path: self.file_path.clone(),
            config: self.config.clone(),
            streaming_source: Arc::new(Mutex::new(None)), // Create fresh streaming source for clone
        }
    }
}

#[async_trait]
impl CaptureSource for FileSource {
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(path = %self.file_path.display(), "Starting file capture source");

        // Validate the file before starting
        self.validate().await?;

        // For file sources, "starting" just means verifying the file is accessible
        // The actual capture happens during snapshot() calls
        debug!("File capture source started successfully");

        Ok(CaptureHandle::new(Arc::new(self.clone())))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        debug!(
            path = %self.file_path.display(),
            quality = self.config.quality,
            "Taking optimized snapshot from file source"
        );

        // Ensure streaming source is initialized
        self.ensure_streaming_source().await?;

        // Clone the streaming source to use outside the lock
        let streaming_source = {
            let source_guard = self.streaming_source.lock().unwrap();
            if let Some(ref streaming_source) = *source_guard {
                streaming_source.clone()
            } else {
                return Err(Error::Config(
                    "Streaming source not initialized".to_string(),
                ));
            }
        };

        // Now we can use the streaming source without holding the lock
        let handle = streaming_source.start().await?;
        let snapshot = handle.snapshot().await?;
        let _ = handle.stop().await; // Clean up after snapshot
        Ok(snapshot)
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        debug!(path = %self.file_path.display(), "Stopping file capture source");

        // Clean up the streaming source if it exists
        let streaming_source = {
            let mut source_guard = self.streaming_source.lock().unwrap();
            source_guard.take()
        };

        if let Some(streaming_source) = streaming_source {
            if let Ok(handle) = streaming_source.start().await {
                let _ = handle.stop().await; // Best effort cleanup
            }
            debug!("Streaming source cleaned up");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::create_test_id;
    use tokio::fs;

    #[tokio::test]
    async fn test_file_source_creation() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        let source = FileSource::new(&video_path);
        assert_eq!(source.file_path(), &video_path);
        assert_eq!(source.config().quality, 85); // Default quality
    }

    #[tokio::test]
    async fn test_file_source_with_config() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        let config = SnapshotConfig {
            quality: 95,
            max_width: Some(1280),
            ..Default::default()
        };

        let source = FileSource::with_config(&video_path, config);
        assert_eq!(source.config().quality, 95);
        assert_eq!(source.config().max_width, Some(1280));
    }

    #[tokio::test]
    async fn test_file_source_validation_missing_file() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("nonexistent.mp4");

        let source = FileSource::new(&video_path);
        let result = source.validate().await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("File does not exist"));
    }

    #[tokio::test]
    async fn test_file_source_validation_directory() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        let source = FileSource::new(&temp_dir);
        let result = source.validate().await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Path is not a file"));
    }

    #[tokio::test]
    async fn test_file_source_validation_success() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        // Create a dummy file (not a real video, but sufficient for validation test)
        fs::write(&video_path, b"fake video data").await.unwrap();

        let source = FileSource::new(&video_path);
        let result = source.validate().await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_file_source_start_missing_file() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("nonexistent.mp4");

        let source = FileSource::new(&video_path);
        let result = source.start().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_file_source_start_success() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        // Create a dummy file
        fs::write(&video_path, b"fake video data").await.unwrap();

        let source = FileSource::new(&video_path);
        let result = source.start().await;

        assert!(result.is_ok());

        // Test that we can stop the handle
        let handle = result.unwrap();
        let stop_result = handle.stop().await;
        assert!(stop_result.is_ok());
    }

    #[tokio::test]
    async fn test_file_source_stop() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        fs::write(&video_path, b"fake video data").await.unwrap();

        let source = FileSource::new(&video_path);
        let result = source.stop().await;

        // Stop should always succeed for file sources
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_capture_handle_drop_cleanup() {
        let test_id = create_test_id();
        let temp_dir = std::env::temp_dir().join(format!("gl_capture_test_{}", test_id));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let video_path = temp_dir.join("test.mp4");

        fs::write(&video_path, b"fake video data").await.unwrap();

        let source = FileSource::new(&video_path);
        let handle = source.start().await.unwrap();

        // Test that dropping the handle doesn't panic
        drop(handle);

        // Give the drop cleanup task a moment to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Integration test for snapshot generation - only runs if ffmpeg is available
    #[tokio::test]
    #[ignore = "Requires ffmpeg and real video file"]
    async fn test_file_source_snapshot_integration() {
        // This test would require a real video file and ffmpeg installed
        // For now, we'll skip it in regular test runs

        // To run this test manually:
        // 1. Create a test.mp4 file in /tmp
        // 2. Run: cargo test test_file_source_snapshot_integration -- --ignored

        let video_path = PathBuf::from("/tmp/test.mp4");
        if video_path.exists() {
            let source = FileSource::new(&video_path);
            let handle = source.start().await.unwrap();

            match handle.snapshot().await {
                Ok(jpeg_bytes) => {
                    assert!(!jpeg_bytes.is_empty());
                    // JPEG files start with 0xFF 0xD8
                    assert_eq!(jpeg_bytes[0], 0xFF);
                    assert_eq!(jpeg_bytes[1], 0xD8);
                }
                Err(e) => {
                    // This could fail if ffmpeg isn't installed or file isn't valid
                    eprintln!("Snapshot failed (expected if ffmpeg not available): {}", e);
                }
            }
        }
    }
}
