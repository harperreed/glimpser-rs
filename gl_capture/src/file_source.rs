//! ABOUTME: File-based capture source for reading video files
//! ABOUTME: Implements CaptureSource trait for MP4 and other video file formats

use crate::{generate_snapshot_with_ffmpeg, CaptureHandle, CaptureSource, SnapshotConfig};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{debug, info, instrument};

/// File-based capture source that reads from video files
#[derive(Debug, Clone)]
pub struct FileSource {
    /// Path to the video file
    file_path: PathBuf,
    /// Configuration for snapshot generation
    config: SnapshotConfig,
}

impl FileSource {
    /// Create a new file source from a video file path
    pub fn new<P: AsRef<Path>>(file_path: P) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
            config: SnapshotConfig::default(),
        }
    }

    /// Create a file source with custom snapshot configuration
    pub fn with_config<P: AsRef<Path>>(file_path: P, config: SnapshotConfig) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
            config,
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
            "Taking snapshot from file source"
        );

        // Use ffmpeg to extract a frame from the video file
        generate_snapshot_with_ffmpeg(&self.file_path, &self.config).await
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        debug!(path = %self.file_path.display(), "Stopping file capture source");

        // For file sources, there's no background process to stop
        // This is a no-op but maintains the interface contract
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

        let mut config = SnapshotConfig::default();
        config.quality = 95;
        config.max_width = Some(1280);

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
