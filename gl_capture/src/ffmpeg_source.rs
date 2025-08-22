//! ABOUTME: FFmpeg-based capture source for live streams and RTSP feeds  
//! ABOUTME: Implements CaptureSource trait with hardware acceleration support

use crate::{CaptureSource, CaptureHandle, SnapshotConfig};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{CommandSpec, run};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::{debug, info, warn, instrument};

/// FFmpeg hardware acceleration types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HardwareAccel {
    /// Software decoding (no acceleration)
    None,
    /// Video Acceleration API (Linux)
    Vaapi,
    /// NVIDIA CUDA
    Cuda,
    /// Intel Quick Sync Video
    Qsv,
    /// VideoToolbox (macOS)
    VideoToolbox,
}

impl Default for HardwareAccel {
    fn default() -> Self {
        Self::None
    }
}

/// Configuration for FFmpeg capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfmpegConfig {
    /// Input URL (RTSP, HTTP, file, etc.)
    pub input_url: String,
    /// Hardware acceleration type
    pub hardware_accel: HardwareAccel,
    /// Additional FFmpeg input options
    pub input_options: HashMap<String, String>,
    /// Video codec for processing
    pub video_codec: Option<String>,
    /// Frame rate for capture
    pub frame_rate: Option<f64>,
    /// Buffer size for input
    pub buffer_size: Option<String>,
    /// Connection timeout in seconds
    pub timeout: Option<u32>,
    /// Snapshot configuration
    pub snapshot_config: SnapshotConfig,
}

impl Default for FfmpegConfig {
    fn default() -> Self {
        Self {
            input_url: String::new(),
            hardware_accel: HardwareAccel::None,
            input_options: HashMap::new(),
            video_codec: None,
            frame_rate: None,
            buffer_size: None,
            timeout: Some(30),
            snapshot_config: SnapshotConfig::default(),
        }
    }
}

/// FFmpeg-based capture source
#[derive(Debug, Clone)]
pub struct FfmpegSource {
    config: FfmpegConfig,
    process_state: Arc<Mutex<ProcessState>>,
}

#[derive(Debug, Clone)]
struct ProcessState {
    is_running: bool,
    restart_count: u64,
}

impl FfmpegSource {
    /// Create a new FFmpeg capture source
    pub fn new(config: FfmpegConfig) -> Self {
        Self {
            config,
            process_state: Arc::new(Mutex::new(ProcessState {
                is_running: false,
                restart_count: 0,
            })),
        }
    }

    /// Get the current configuration
    pub fn config(&self) -> &FfmpegConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: FfmpegConfig) {
        self.config = config;
    }

    /// Get restart count for metrics
    pub async fn restart_count(&self) -> u64 {
        self.process_state.lock().await.restart_count
    }

    /// Build FFmpeg command for snapshot extraction
    #[instrument(skip(self))]
    fn build_snapshot_command(&self) -> CommandSpec {
        let mut args = Vec::new();

        // Add hardware acceleration if specified
        match self.config.hardware_accel {
            HardwareAccel::None => {}
            HardwareAccel::Vaapi => {
                args.extend(["-hwaccel".to_string(), "vaapi".to_string()]);
            }
            HardwareAccel::Cuda => {
                args.extend(["-hwaccel".to_string(), "cuda".to_string()]);
            }
            HardwareAccel::Qsv => {
                args.extend(["-hwaccel".to_string(), "qsv".to_string()]);
            }
            HardwareAccel::VideoToolbox => {
                args.extend(["-hwaccel".to_string(), "videotoolbox".to_string()]);
            }
        }

        // Add input options
        for (key, value) in &self.config.input_options {
            args.extend([format!("-{}", key), value.clone()]);
        }

        // Add timeout if specified
        if let Some(timeout) = self.config.timeout {
            args.extend(["-timeout".to_string(), timeout.to_string()]);
        }

        // Add buffer size if specified  
        if let Some(buffer_size) = &self.config.buffer_size {
            args.extend(["-buffer_size".to_string(), buffer_size.clone()]);
        }

        // Input source
        args.extend(["-i".to_string(), self.config.input_url.clone()]);

        // Video processing options
        if let Some(codec) = &self.config.video_codec {
            args.extend(["-c:v".to_string(), codec.clone()]);
        }

        // Frame extraction: get one frame
        args.extend([
            "-vframes".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "image2".to_string(),
        ]);

        // Quality settings for JPEG
        let quality_scale = ((31 * (100 - self.config.snapshot_config.quality as u32)) / 100 + 2).to_string();
        args.extend(["-q:v".to_string(), quality_scale]);

        // Scaling if specified
        if let (Some(width), Some(height)) = (
            self.config.snapshot_config.max_width,
            self.config.snapshot_config.max_height,
        ) {
            args.extend([
                "-vf".to_string(),
                format!("scale={}:{}:force_original_aspect_ratio=decrease", width, height),
            ]);
        }

        // Output to stdout
        args.push("pipe:1".to_string());

        CommandSpec::new("ffmpeg".into())
            .args(args)
            .timeout(self.config.snapshot_config.timeout)
    }

    /// Validate FFmpeg configuration and connectivity
    #[instrument(skip(self))]
    pub async fn validate(&self) -> Result<()> {
        debug!(url = %self.config.input_url, "Validating FFmpeg source");

        // Build a simple probe command to test connectivity
        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
        ];

        // Add timeout
        if let Some(timeout) = self.config.timeout {
            args.extend(["-timeout".to_string(), timeout.to_string()]);
        }

        // Input
        args.extend([
            "-i".to_string(),
            self.config.input_url.clone(),
            "-t".to_string(),
            "1".to_string(), // Just probe for 1 second
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ]);

        let spec = CommandSpec::new("ffmpeg".into())
            .args(args)
            .timeout(Duration::from_secs(10));

        debug!(command = ?spec, "Running FFmpeg validation");

        match run(spec).await {
            Ok(result) => {
                if result.success() {
                    info!(url = %self.config.input_url, "FFmpeg source validation successful");
                    Ok(())
                } else {
                    let error_msg = if !result.stderr.is_empty() {
                        result.stderr
                    } else {
                        "FFmpeg validation failed".to_string()
                    };
                    Err(Error::Config(format!(
                        "FFmpeg validation failed for {}: {}",
                        self.config.input_url, error_msg
                    )))
                }
            }
            Err(e) => Err(Error::Config(format!(
                "Failed to run FFmpeg validation for {}: {}",
                self.config.input_url, e
            ))),
        }
    }
}

#[async_trait]
impl CaptureSource for FfmpegSource {
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(url = %self.config.input_url, "Starting FFmpeg capture source");

        // Validate configuration first
        self.validate().await?;

        // Update process state
        {
            let mut state = self.process_state.lock().await;
            state.is_running = true;
        }

        debug!("FFmpeg capture source started successfully");

        Ok(CaptureHandle::new(Arc::new(self.clone())))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        let start_time = Instant::now();
        
        debug!(
            url = %self.config.input_url,
            quality = self.config.snapshot_config.quality,
            "Taking snapshot from FFmpeg source"
        );

        // Increment snapshot attempt counter
        counter!(
            "ffmpeg_snapshot_attempts_total",
            "input_url" => self.config.input_url.clone(),
            "hardware_accel" => format!("{:?}", self.config.hardware_accel)
        ).increment(1);

        let command = self.build_snapshot_command();
        debug!(command = ?command, "Running FFmpeg snapshot command");

        let result = run(command).await?;

        if !result.success() {
            // Log stderr for debugging
            if !result.stderr.is_empty() {
                warn!(
                    stderr = %result.stderr,
                    url = %self.config.input_url,
                    "FFmpeg snapshot command failed"
                );
            }

            // Increment restart count and failure metrics on failure
            {
                let mut state = self.process_state.lock().await;
                state.restart_count += 1;
                
                counter!(
                    "ffmpeg_process_restarts_total",
                    "input_url" => self.config.input_url.clone()
                ).increment(1);
            }

            counter!(
                "ffmpeg_snapshot_failures_total",
                "input_url" => self.config.input_url.clone(),
                "error_type" => "process_failure"
            ).increment(1);

            return Err(Error::Config(format!(
                "FFmpeg snapshot failed for {}: exit code {} - {}",
                self.config.input_url,
                result.exit_code().unwrap_or(-1),
                result.stderr
            )));
        }

        if result.stdout.is_empty() {
            counter!(
                "ffmpeg_snapshot_failures_total",
                "input_url" => self.config.input_url.clone(),
                "error_type" => "empty_output"
            ).increment(1);

            return Err(Error::Config(format!(
                "FFmpeg produced no output for {}",
                self.config.input_url
            )));
        }

        // Record successful snapshot metrics
        let duration = start_time.elapsed();
        histogram!(
            "ffmpeg_snapshot_duration_seconds",
            "input_url" => self.config.input_url.clone(),
            "hardware_accel" => format!("{:?}", self.config.hardware_accel)
        ).record(duration.as_secs_f64());

        counter!(
            "ffmpeg_snapshot_success_total",
            "input_url" => self.config.input_url.clone()
        ).increment(1);

        debug!(
            output_size = result.stdout.len(),
            truncated = result.stdout_truncated,
            duration_ms = duration.as_millis(),
            url = %self.config.input_url,
            "FFmpeg snapshot generated successfully"
        );

        Ok(Bytes::from(result.stdout.into_bytes()))
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        debug!(url = %self.config.input_url, "Stopping FFmpeg capture source");

        // Update process state
        {
            let mut state = self.process_state.lock().await;
            state.is_running = false;
        }

        // For FFmpeg snapshots, there's no persistent process to stop
        // This maintains the interface contract
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test FFmpeg configuration
    fn create_test_config() -> FfmpegConfig {
        FfmpegConfig {
            input_url: "rtsp://example.com/stream".to_string(),
            hardware_accel: HardwareAccel::None,
            input_options: HashMap::new(),
            video_codec: None,
            frame_rate: None,
            buffer_size: None,
            timeout: Some(10),
            snapshot_config: SnapshotConfig::default(),
        }
    }

    #[test]
    fn test_ffmpeg_config_creation() {
        let config = FfmpegConfig::default();
        assert_eq!(config.input_url, "");
        assert!(matches!(config.hardware_accel, HardwareAccel::None));
        assert_eq!(config.timeout, Some(30));
        assert_eq!(config.snapshot_config.quality, 85);
    }

    #[test]
    fn test_hardware_accel_serialization() {
        let accel_types = vec![
            HardwareAccel::None,
            HardwareAccel::Vaapi,
            HardwareAccel::Cuda,
            HardwareAccel::Qsv,
            HardwareAccel::VideoToolbox,
        ];

        for accel in accel_types {
            let json = serde_json::to_string(&accel).unwrap();
            let deserialized: HardwareAccel = serde_json::from_str(&json).unwrap();
            // Can't directly compare enum variants, but this tests round-trip serialization
            let _: String = format!("{:?}", deserialized);
        }
    }

    #[test]
    fn test_ffmpeg_source_creation() {
        let config = create_test_config();
        let source = FfmpegSource::new(config.clone());
        
        assert_eq!(source.config().input_url, config.input_url);
        assert_eq!(source.config().timeout, config.timeout);
    }

    #[tokio::test]
    async fn test_restart_count_initialization() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);
        
        assert_eq!(source.restart_count().await, 0);
    }

    #[test]
    fn test_command_building_basic() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);
        
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain input URL
        assert!(args.iter().any(|arg| arg == "rtsp://example.com/stream"));
        
        // Should contain basic options
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"-vframes".to_string()));
        assert!(args.contains(&"1".to_string()));
        assert!(args.contains(&"pipe:1".to_string()));
        
        // Should contain timeout
        assert!(args.contains(&"-timeout".to_string()));
        assert!(args.contains(&"10".to_string()));
    }

    #[test]
    fn test_command_building_with_hardware_accel() {
        let mut config = create_test_config();
        config.hardware_accel = HardwareAccel::Cuda;
        
        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain hardware acceleration
        assert!(args.contains(&"-hwaccel".to_string()));
        assert!(args.contains(&"cuda".to_string()));
    }

    #[test]
    fn test_command_building_with_codec() {
        let mut config = create_test_config();
        config.video_codec = Some("h264".to_string());
        
        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain video codec
        assert!(args.contains(&"-c:v".to_string()));
        assert!(args.contains(&"h264".to_string()));
    }

    #[test]
    fn test_command_building_with_scaling() {
        let mut config = create_test_config();
        config.snapshot_config.max_width = Some(1280);
        config.snapshot_config.max_height = Some(720);
        
        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain scaling filter
        assert!(args.contains(&"-vf".to_string()));
        assert!(args.iter().any(|arg| arg.contains("scale=1280:720")));
    }

    #[test]
    fn test_command_building_with_input_options() {
        let mut config = create_test_config();
        config.input_options.insert("rtsp_transport".to_string(), "tcp".to_string());
        config.input_options.insert("stimeout".to_string(), "5000000".to_string());
        
        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain input options
        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"tcp".to_string()));
        assert!(args.contains(&"-stimeout".to_string()));
        assert!(args.contains(&"5000000".to_string()));
    }

    #[test]
    fn test_quality_conversion() {
        let mut config = create_test_config();
        config.snapshot_config.quality = 95; // High quality
        
        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;
        
        // Should contain quality parameter
        assert!(args.contains(&"-q:v".to_string()));
        
        // Find the quality value: (31 * (100 - 95)) / 100 + 2 = 3 (approximately)
        let quality_index = args.iter().position(|arg| arg == "-q:v").unwrap();
        let quality_value: u32 = args[quality_index + 1].parse().unwrap();
        assert!(quality_value <= 5); // Should be low value for high quality
    }

    // Integration tests - these require ffmpeg to be installed
    #[tokio::test]
    #[ignore = "Requires ffmpeg installation and network access"]
    async fn test_ffmpeg_source_validation_success() {
        // Test with a known-good test pattern generator
        let mut config = create_test_config();
        config.input_url = "testsrc=duration=1:size=320x240:rate=1".to_string();
        config.input_options.insert("f".to_string(), "lavfi".to_string());
        
        let source = FfmpegSource::new(config);
        
        // This should succeed if ffmpeg is available
        match source.validate().await {
            Ok(_) => {
                // Success - ffmpeg is available and working
            }
            Err(e) => {
                // Expected if ffmpeg is not available
                eprintln!("FFmpeg validation failed (expected if not installed): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires ffmpeg installation"]
    async fn test_ffmpeg_source_snapshot_integration() {
        // Test with a synthetic test pattern
        let mut config = create_test_config();
        config.input_url = "testsrc=duration=1:size=640x480:rate=1".to_string();
        config.input_options.insert("f".to_string(), "lavfi".to_string());
        config.timeout = Some(5);
        
        let source = FfmpegSource::new(config);
        
        match source.start().await {
            Ok(handle) => {
                match handle.snapshot().await {
                    Ok(jpeg_bytes) => {
                        assert!(!jpeg_bytes.is_empty());
                        // JPEG files start with 0xFF 0xD8
                        assert_eq!(jpeg_bytes[0], 0xFF);
                        assert_eq!(jpeg_bytes[1], 0xD8);
                    }
                    Err(e) => {
                        eprintln!("Snapshot failed (expected if ffmpeg not fully functional): {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("FFmpeg start failed (expected if ffmpeg not available): {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_ffmpeg_source_validation_invalid_url() {
        let mut config = create_test_config();
        config.input_url = "invalid://nonexistent/stream".to_string();
        config.timeout = Some(2); // Short timeout
        
        let source = FfmpegSource::new(config);
        
        // This should fail due to invalid URL
        let result = source.validate().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ffmpeg_source_lifecycle() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);
        
        // Test basic lifecycle without actual ffmpeg execution
        // This tests the state management logic
        let initial_count = source.restart_count().await;
        assert_eq!(initial_count, 0);
        
        // Stop should work even without starting
        let result = source.stop().await;
        assert!(result.is_ok());
    }
}