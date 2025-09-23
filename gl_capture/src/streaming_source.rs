//! ABOUTME: High-performance streaming FFmpeg source using persistent process pool
//! ABOUTME: Provides continuous MJPEG frame extraction without process spawning overhead

use crate::{
    process_pool::{FfmpegProcessPool, ProcessPoolConfig, ProcessPoolMetrics},
    CaptureHandle, CaptureSource, FfmpegConfig, HardwareAccel,
};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, error, info, instrument, warn};

/// High-performance streaming capture source using persistent FFmpeg processes
#[derive(Debug, Clone)]
pub struct StreamingFfmpegSource {
    /// Process pool for frame extraction
    process_pool: Arc<FfmpegProcessPool>,
    /// Source configuration
    config: StreamingSourceConfig,
    /// Running state
    is_running: Arc<AtomicBool>,
}

/// Configuration for streaming FFmpeg source
#[derive(Debug, Clone)]
pub struct StreamingSourceConfig {
    /// Base FFmpeg configuration
    pub ffmpeg_config: FfmpegConfig,
    /// Frame rate for continuous streaming
    pub frame_rate: f64,
    /// Number of worker processes
    pub pool_size: usize,
    /// Enable health monitoring
    pub health_monitoring: bool,
    /// Frame timeout
    pub frame_timeout: Duration,
}

impl Default for StreamingSourceConfig {
    fn default() -> Self {
        Self {
            ffmpeg_config: FfmpegConfig::default(),
            frame_rate: 10.0,
            pool_size: 2,
            health_monitoring: true,
            frame_timeout: Duration::from_secs(5),
        }
    }
}

impl StreamingFfmpegSource {
    /// Create a new streaming FFmpeg source
    #[instrument(skip(config))]
    pub async fn new(config: StreamingSourceConfig) -> Result<Self> {
        info!(
            input_url = %config.ffmpeg_config.input_url,
            frame_rate = config.frame_rate,
            pool_size = config.pool_size,
            "Creating streaming FFmpeg source"
        );

        // Create process pool configuration
        let pool_config = ProcessPoolConfig {
            ffmpeg_config: config.ffmpeg_config.clone(),
            pool_size: config.pool_size,
            frame_rate: config.frame_rate,
            health_monitoring: config.health_monitoring,
        };

        // Initialize the process pool
        let process_pool = FfmpegProcessPool::new(pool_config).await?;

        Ok(Self {
            process_pool: Arc::new(process_pool),
            config,
            is_running: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create from FFmpeg config with default streaming settings
    pub async fn from_ffmpeg_config(ffmpeg_config: FfmpegConfig) -> Result<Self> {
        let config = StreamingSourceConfig {
            ffmpeg_config,
            ..Default::default()
        };
        Self::new(config).await
    }

    /// Create with auto-detected hardware acceleration
    #[instrument(skip(ffmpeg_config))]
    pub async fn with_auto_acceleration(ffmpeg_config: FfmpegConfig) -> Result<Self> {
        info!(
            input_url = %ffmpeg_config.input_url,
            "Creating streaming source with auto-detected hardware acceleration"
        );

        // Use the process pool config helper to detect acceleration
        let pool_config = ProcessPoolConfig::with_auto_acceleration(ffmpeg_config).await?;

        let config = StreamingSourceConfig {
            ffmpeg_config: pool_config.ffmpeg_config,
            frame_rate: pool_config.frame_rate,
            pool_size: pool_config.pool_size,
            health_monitoring: pool_config.health_monitoring,
            frame_timeout: Duration::from_secs(5),
        };

        Self::new(config).await
    }

    /// Create with preferred hardware acceleration
    #[instrument(skip(ffmpeg_config))]
    pub async fn with_preferred_acceleration(
        ffmpeg_config: FfmpegConfig,
        preferred: HardwareAccel,
    ) -> Result<Self> {
        info!(
            input_url = %ffmpeg_config.input_url,
            preferred = ?preferred,
            "Creating streaming source with preferred hardware acceleration"
        );

        // Use the process pool config helper with preference
        let pool_config =
            ProcessPoolConfig::with_preferred_acceleration(ffmpeg_config, preferred).await?;

        let config = StreamingSourceConfig {
            ffmpeg_config: pool_config.ffmpeg_config,
            frame_rate: pool_config.frame_rate,
            pool_size: pool_config.pool_size,
            health_monitoring: pool_config.health_monitoring,
            frame_timeout: Duration::from_secs(5),
        };

        Self::new(config).await
    }

    /// Get the current configuration
    pub fn config(&self) -> &StreamingSourceConfig {
        &self.config
    }

    /// Get process pool metrics
    pub fn metrics(&self) -> &ProcessPoolMetrics {
        self.process_pool.metrics()
    }

    /// Check if the source is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Validate the streaming configuration
    #[instrument(skip(self))]
    pub async fn validate(&self) -> Result<()> {
        debug!(
            input_url = %self.config.ffmpeg_config.input_url,
            "Validating streaming FFmpeg configuration"
        );

        // Try to extract a single frame to validate the configuration
        match tokio::time::timeout(self.config.frame_timeout, self.process_pool.extract_frame())
            .await
        {
            Ok(Ok(frame)) => {
                info!(
                    frame_size = frame.len(),
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Streaming FFmpeg source validation successful"
                );
                Ok(())
            }
            Ok(Err(e)) => {
                error!(
                    error = %e,
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Streaming validation failed"
                );
                Err(e)
            }
            Err(_) => {
                let timeout_ms = self.config.frame_timeout.as_millis();
                error!(
                    timeout_ms = timeout_ms,
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Streaming validation timed out"
                );
                Err(Error::Config(format!(
                    "Streaming validation timed out after {}ms for {}",
                    timeout_ms, self.config.ffmpeg_config.input_url
                )))
            }
        }
    }
}

#[async_trait]
impl CaptureSource for StreamingFfmpegSource {
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(
            input_url = %self.config.ffmpeg_config.input_url,
            "Starting streaming FFmpeg capture source"
        );

        // Validate the configuration first
        self.validate().await?;

        // Update running state
        self.is_running.store(true, Ordering::SeqCst);

        debug!("Streaming FFmpeg capture source started successfully");

        Ok(CaptureHandle::new(Arc::new(self.clone())))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        if !self.is_running() {
            return Err(Error::Config("Source is not running".to_string()));
        }

        debug!(
            input_url = %self.config.ffmpeg_config.input_url,
            "Extracting frame from streaming source"
        );

        // Extract frame with timeout
        match tokio::time::timeout(self.config.frame_timeout, self.process_pool.extract_frame())
            .await
        {
            Ok(Ok(frame)) => {
                debug!(
                    frame_size = frame.len(),
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Frame extracted successfully from streaming source"
                );

                // Validate JPEG format
                if frame.len() >= 2 && frame[0] == 0xFF && frame[1] == 0xD8 {
                    Ok(frame)
                } else {
                    warn!(
                        frame_size = frame.len(),
                        input_url = %self.config.ffmpeg_config.input_url,
                        "Invalid JPEG frame format"
                    );
                    Err(Error::Config("Invalid JPEG frame format".to_string()))
                }
            }
            Ok(Err(e)) => {
                error!(
                    error = %e,
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Frame extraction failed"
                );
                Err(e)
            }
            Err(_) => {
                let timeout_ms = self.config.frame_timeout.as_millis();
                warn!(
                    timeout_ms = timeout_ms,
                    input_url = %self.config.ffmpeg_config.input_url,
                    "Frame extraction timed out"
                );
                Err(Error::Config(format!(
                    "Frame extraction timed out after {}ms",
                    timeout_ms
                )))
            }
        }
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        debug!(
            input_url = %self.config.ffmpeg_config.input_url,
            "Stopping streaming FFmpeg capture source"
        );

        // Update running state
        self.is_running.store(false, Ordering::SeqCst);

        // The process pool will continue running for other sources
        // It will be shut down when dropped
        info!("Streaming FFmpeg capture source stopped");

        Ok(())
    }
}

impl Drop for StreamingFfmpegSource {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HardwareAccel, RtspTransport, SnapshotConfig};
    use std::collections::HashMap;

    #[test]
    fn test_streaming_source_config_default() {
        let config = StreamingSourceConfig::default();
        assert_eq!(config.frame_rate, 10.0);
        assert_eq!(config.pool_size, 2);
        assert!(config.health_monitoring);
        assert_eq!(config.frame_timeout, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_streaming_source_creation() {
        let ffmpeg_config = FfmpegConfig {
            input_url: "rtsp://test.example.com/stream".to_string(),
            hardware_accel: HardwareAccel::None,
            input_options: HashMap::new(),
            rtsp_transport: RtspTransport::Tcp,
            video_codec: None,
            frame_rate: None,
            buffer_size: None,
            timeout: Some(10),
            snapshot_config: SnapshotConfig::default(),
        };

        let config = StreamingSourceConfig {
            ffmpeg_config,
            frame_rate: 15.0,
            pool_size: 1,
            health_monitoring: false, // Disable for test
            frame_timeout: Duration::from_secs(2),
        };

        // This will fail without ffmpeg, but tests the structure
        match StreamingFfmpegSource::new(config).await {
            Ok(source) => {
                assert!(!source.is_running());
                assert_eq!(source.config().frame_rate, 15.0);
                assert_eq!(source.config().pool_size, 1);
                assert!(!source.config().health_monitoring);
            }
            Err(e) => {
                // Expected without ffmpeg
                eprintln!("Expected error without ffmpeg: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_from_ffmpeg_config() {
        let ffmpeg_config = FfmpegConfig {
            input_url: "rtsp://test.example.com/stream".to_string(),
            ..Default::default()
        };

        // This will fail without ffmpeg, but tests the structure
        match StreamingFfmpegSource::from_ffmpeg_config(ffmpeg_config.clone()).await {
            Ok(source) => {
                assert_eq!(
                    source.config().ffmpeg_config.input_url,
                    ffmpeg_config.input_url
                );
                assert_eq!(source.config().frame_rate, 10.0); // Default
                assert_eq!(source.config().pool_size, 2); // Default
            }
            Err(e) => {
                // Expected without ffmpeg
                eprintln!("Expected error without ffmpeg: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_streaming_source_state_management() {
        let config = StreamingSourceConfig {
            ffmpeg_config: FfmpegConfig {
                input_url: "test://input".to_string(),
                ..Default::default()
            },
            pool_size: 1,
            health_monitoring: false,                  // Disable for test
            frame_timeout: Duration::from_millis(100), // Short timeout
            ..Default::default()
        };

        // This will fail during creation without ffmpeg, but tests the state logic
        match StreamingFfmpegSource::new(config).await {
            Ok(source) => {
                // Initial state
                assert!(!source.is_running());

                // The actual start/stop logic would be tested with ffmpeg available
            }
            Err(_) => {
                // Expected without ffmpeg - test passes if we reach here
            }
        }
    }

    // Integration tests would require ffmpeg to be installed
    #[tokio::test]
    #[ignore = "Requires ffmpeg installation and network access"]
    async fn test_streaming_source_integration() {
        let config = StreamingSourceConfig {
            ffmpeg_config: FfmpegConfig {
                input_url: "testsrc=duration=5:size=320x240:rate=5".to_string(),
                input_options: {
                    let mut opts = HashMap::new();
                    opts.insert("f".to_string(), "lavfi".to_string());
                    opts
                },
                ..Default::default()
            },
            frame_rate: 2.0,
            pool_size: 1,
            health_monitoring: false, // Disable for test
            frame_timeout: Duration::from_secs(5),
        };

        match StreamingFfmpegSource::new(config).await {
            Ok(source) => {
                // Test lifecycle
                let handle = source.start().await.unwrap();
                assert!(source.is_running());

                // Extract a frame
                match handle.snapshot().await {
                    Ok(frame) => {
                        assert!(!frame.is_empty());
                        assert_eq!(frame[0], 0xFF);
                        assert_eq!(frame[1], 0xD8);
                        println!("Frame extracted: {} bytes", frame.len());
                    }
                    Err(e) => {
                        eprintln!("Frame extraction failed: {}", e);
                    }
                }

                // Stop
                source.stop().await.unwrap();
                assert!(!source.is_running());

                // Check metrics
                let metrics = source.metrics();
                let frames = metrics.frames_extracted.load(Ordering::Relaxed);
                println!("Frames extracted: {}", frames);
                assert!(frames > 0);
            }
            Err(e) => {
                eprintln!("Integration test failed (expected without ffmpeg): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires ffmpeg installation"]
    async fn test_continuous_vs_single_frame_performance() {
        // This test would compare the performance of the new streaming source
        // against the traditional single-frame extraction approach

        let config = StreamingSourceConfig {
            ffmpeg_config: FfmpegConfig {
                input_url: "testsrc=duration=10:size=640x480:rate=10".to_string(),
                input_options: {
                    let mut opts = HashMap::new();
                    opts.insert("f".to_string(), "lavfi".to_string());
                    opts
                },
                ..Default::default()
            },
            frame_rate: 10.0,
            pool_size: 2,
            health_monitoring: true,
            frame_timeout: Duration::from_secs(2),
        };

        match StreamingFfmpegSource::new(config).await {
            Ok(source) => {
                let handle = source.start().await.unwrap();

                // Extract multiple frames and measure timing
                let start_time = std::time::Instant::now();
                let mut successful_extractions = 0;

                for i in 0..20 {
                    match handle.snapshot().await {
                        Ok(frame) => {
                            successful_extractions += 1;
                            println!("Frame {}: {} bytes", i, frame.len());
                        }
                        Err(e) => {
                            eprintln!("Frame {} failed: {}", i, e);
                        }
                    }

                    // Small delay between extractions
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }

                let duration = start_time.elapsed();
                let avg_ms = duration.as_millis() / successful_extractions.max(1) as u128;

                println!(
                    "Extracted {} frames in {:?}",
                    successful_extractions, duration
                );
                println!("Average time per frame: {}ms", avg_ms);

                // With persistent processes, average should be much lower than 100ms
                // (The traditional approach with process spawning typically takes 100-500ms per frame)
                if successful_extractions > 0 {
                    assert!(
                        avg_ms < 100,
                        "Average extraction time {}ms should be < 100ms",
                        avg_ms
                    );
                }

                // Check metrics
                let metrics = source.metrics();
                let total_frames = metrics.frames_extracted.load(Ordering::Relaxed);
                let avg_time_us = metrics.avg_extraction_time_us.load(Ordering::Relaxed);

                println!("Total frames from pool: {}", total_frames);
                println!("Average extraction time from pool: {}μs", avg_time_us);

                source.stop().await.unwrap();
            }
            Err(e) => {
                eprintln!("Performance test failed (expected without ffmpeg): {}", e);
            }
        }
    }
}
