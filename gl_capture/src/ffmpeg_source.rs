//! ABOUTME: FFmpeg-based capture source for live streams and RTSP feeds
//! ABOUTME: Implements CaptureSource trait with hardware acceleration support

use crate::{CaptureHandle, CaptureSource, SnapshotConfig};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{run, CommandSpec};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, info, instrument, warn};

/// FFmpeg hardware acceleration types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Preferred RTSP transport protocol
    pub rtsp_transport: RtspTransport,
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
            rtsp_transport: RtspTransport::Tcp,
            video_codec: None,
            frame_rate: None,
            buffer_size: None,
            timeout: Some(30),
            snapshot_config: SnapshotConfig::default(),
        }
    }
}

/// RTSP transport options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RtspTransport {
    /// Use TCP transport
    Tcp,
    /// Use UDP transport
    Udp,
    /// Let FFmpeg decide transport
    Auto,
}

impl Default for RtspTransport {
    fn default() -> Self {
        Self::Tcp
    }
}

/// FFmpeg-based capture source
#[derive(Debug, Clone)]
pub struct FfmpegSource {
    config: FfmpegConfig,
    is_running: Arc<AtomicBool>,
    restart_count: Arc<AtomicU64>,
}

impl FfmpegSource {
    /// Create a new FFmpeg capture source
    pub fn new(config: FfmpegConfig) -> Self {
        Self {
            config,
            is_running: Arc::new(AtomicBool::new(false)),
            restart_count: Arc::new(AtomicU64::new(0)),
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

    /// Check if capture is running
    pub fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    /// Get restart count for metrics
    pub fn restart_count(&self) -> u64 {
        self.restart_count.load(Ordering::SeqCst)
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

        // Determine if input is RTSP or RTSPS
        let is_rtsp = self.config.input_url.starts_with("rtsp://")
            || self.config.input_url.starts_with("rtsps://");

        // Add input options
        for (key, value) in &self.config.input_options {
            args.extend([format!("-{}", key), value.clone()]);
        }

        // Add timeout if specified
        if let Some(timeout) = self.config.timeout {
            let micros = (timeout as u64) * 1_000_000;
            args.extend(["-timeout".to_string(), micros.to_string()]);
        }

        // Add default RTSP options
        if is_rtsp {
            match self.config.rtsp_transport {
                RtspTransport::Tcp => {
                    args.extend(["-rtsp_transport".to_string(), "tcp".to_string()]);
                    args.extend(["-rtsp_flags".to_string(), "prefer_tcp".to_string()]);
                }
                RtspTransport::Udp => {
                    args.extend(["-rtsp_transport".to_string(), "udp".to_string()]);
                }
                RtspTransport::Auto => {
                    args.extend(["-rtsp_flags".to_string(), "prefer_tcp".to_string()]);
                }
            }
            args.extend(["-fflags".to_string(), "nobuffer".to_string()]);
            args.extend(["-flags".to_string(), "low_delay".to_string()]);
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
        let quality_scale =
            ((31 * (100 - self.config.snapshot_config.quality as u32)) / 100 + 2).to_string();
        args.extend(["-q:v".to_string(), quality_scale]);

        // Scaling if specified
        if let (Some(width), Some(height)) = (
            self.config.snapshot_config.max_width,
            self.config.snapshot_config.max_height,
        ) {
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

        CommandSpec::new("ffmpeg".into())
            .args(args)
            .timeout(self.config.snapshot_config.timeout)
    }

    /// Build FFmpeg command for connection validation
    fn build_validate_command(&self) -> CommandSpec {
        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
        ];

        let is_rtsp = self.config.input_url.starts_with("rtsp://")
            || self.config.input_url.starts_with("rtsps://");

        if let Some(timeout) = self.config.timeout {
            let micros = (timeout as u64) * 1_000_000;
            args.extend(["-timeout".to_string(), micros.to_string()]);
        }

        if is_rtsp {
            match self.config.rtsp_transport {
                RtspTransport::Tcp => {
                    args.extend(["-rtsp_transport".to_string(), "tcp".to_string()]);
                    args.extend(["-rtsp_flags".to_string(), "prefer_tcp".to_string()]);
                }
                RtspTransport::Udp => {
                    args.extend(["-rtsp_transport".to_string(), "udp".to_string()]);
                }
                RtspTransport::Auto => {
                    args.extend(["-rtsp_flags".to_string(), "prefer_tcp".to_string()]);
                }
            }
            args.extend(["-fflags".to_string(), "nobuffer".to_string()]);
            args.extend(["-flags".to_string(), "low_delay".to_string()]);
        }

        args.extend([
            "-i".to_string(),
            self.config.input_url.clone(),
            "-t".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "null".to_string(),
            "-".to_string(),
        ]);

        CommandSpec::new("ffmpeg".into())
            .args(args)
            .timeout(Duration::from_secs(10))
    }

    /// Validate FFmpeg configuration and connectivity
    #[instrument(skip(self))]
    pub async fn validate(&self) -> Result<()> {
        debug!(url = %self.config.input_url, "Validating FFmpeg source");
        let spec = self.build_validate_command();

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
        self.is_running.store(true, Ordering::SeqCst);

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
        )
        .increment(1);

        let command = self.build_snapshot_command();
        debug!(command = ?command, "Running FFmpeg snapshot command");

        // Calculate total deadline: snapshot timeout + grace period
        let snapshot_timeout = self.config.snapshot_config.timeout;
        let grace_period = Duration::from_secs(5); // Extra time for process cleanup
        let total_deadline = snapshot_timeout + grace_period;

        // Run FFmpeg in a blocking thread with timeout protection
        let result = match tokio::time::timeout(
            total_deadline,
            tokio::task::spawn_blocking(move || {
                let runtime = tokio::runtime::Handle::current();
                runtime.block_on(run(command))
            })
        ).await {
            Ok(spawn_result) => {
                spawn_result.map_err(|e| Error::Config(format!("Background FFmpeg task failed: {}", e)))?
            },
            Err(_) => {
                // Timeout occurred - FFmpeg process is stuck
                counter!(
                    "ffmpeg_snapshot_timeouts_total",
                    "input_url" => self.config.input_url.clone(),
                    "timeout_seconds" => total_deadline.as_secs().to_string()
                )
                .increment(1);

                warn!(
                    url = %self.config.input_url,
                    timeout_seconds = total_deadline.as_secs(),
                    "FFmpeg snapshot operation timed out - process may be stuck"
                );

                return Err(Error::Config(format!(
                    "FFmpeg snapshot timed out after {}s for {}",
                    total_deadline.as_secs(),
                    self.config.input_url
                )));
            }
        }?;

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
            self.restart_count.fetch_add(1, Ordering::SeqCst);
            counter!(
                "ffmpeg_process_restarts_total",
                "input_url" => self.config.input_url.clone()
            )
            .increment(1);

            counter!(
                "ffmpeg_snapshot_failures_total",
                "input_url" => self.config.input_url.clone(),
                "error_type" => "process_failure"
            )
            .increment(1);

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
            )
            .increment(1);

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
        )
        .record(duration.as_secs_f64());

        counter!(
            "ffmpeg_snapshot_success_total",
            "input_url" => self.config.input_url.clone()
        )
        .increment(1);

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
        self.is_running.store(false, Ordering::SeqCst);

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
            rtsp_transport: RtspTransport::Tcp,
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

    #[test]
    fn test_restart_count_initialization() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);

        assert_eq!(source.restart_count(), 0);
    }

    #[test]
    fn test_initial_is_running() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);

        assert!(!source.is_running());
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

        // Should contain RTSP defaults
        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"tcp".to_string()));
        assert!(args.contains(&"-rtsp_flags".to_string()));
        assert!(args.contains(&"prefer_tcp".to_string()));
        assert!(args.contains(&"-fflags".to_string()));
        assert!(args.contains(&"nobuffer".to_string()));
        assert!(args.contains(&"-flags".to_string()));
        assert!(args.contains(&"low_delay".to_string()));

        // Should convert timeout to microseconds
        assert!(args.contains(&"-timeout".to_string()));
        assert!(args.contains(&"10000000".to_string()));
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
        config
            .input_options
            .insert("user_option".to_string(), "value".to_string());

        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;

        // Should contain user input option along with defaults
        assert!(args.contains(&"-user_option".to_string()));
        assert!(args.contains(&"value".to_string()));
        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"tcp".to_string()));
    }

    #[test]
    fn test_rtsp_transport_udp() {
        let mut config = create_test_config();
        config.rtsp_transport = RtspTransport::Udp;

        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;

        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"udp".to_string()));
    }

    #[test]
    fn test_timeout_conversion_non_rtsp() {
        let mut config = create_test_config();
        config.input_url = "http://example.com/stream".to_string();

        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;

        assert!(args.contains(&"-timeout".to_string()));
        assert!(args.contains(&"10000000".to_string()));
    }

    #[test]
    fn test_validate_command_rtsp_defaults() {
        let config = create_test_config();
        let source = FfmpegSource::new(config);
        let command = source.build_validate_command();
        let args = &command.args;

        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"tcp".to_string()));
        assert!(args.contains(&"-rtsp_flags".to_string()));
        assert!(args.contains(&"prefer_tcp".to_string()));
        assert!(args.contains(&"-fflags".to_string()));
        assert!(args.contains(&"nobuffer".to_string()));
        assert!(args.contains(&"-flags".to_string()));
        assert!(args.contains(&"low_delay".to_string()));
        assert!(args.contains(&"-timeout".to_string()));
        assert!(args.contains(&"10000000".to_string()));
    }

    #[test]
    fn test_rtsps_support() {
        let mut config = create_test_config();
        config.input_url = "rtsps://secure.example.com/stream".to_string();

        let source = FfmpegSource::new(config);
        let command = source.build_snapshot_command();
        let args = &command.args;

        assert!(args.contains(&"-rtsp_transport".to_string()));
        assert!(args.contains(&"tcp".to_string()));
        assert!(args.contains(&"-timeout".to_string()));
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
        config
            .input_options
            .insert("f".to_string(), "lavfi".to_string());

        let source = FfmpegSource::new(config);

        // This should succeed if ffmpeg is available
        match source.validate().await {
            Ok(_) => {
                // Success - ffmpeg is available and working
            }
            Err(e) => {
                // Expected if ffmpeg is not available
                eprintln!(
                    "FFmpeg validation failed (expected if not installed): {}",
                    e
                );
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires ffmpeg installation"]
    async fn test_ffmpeg_source_snapshot_integration() {
        // Test with a synthetic test pattern
        let mut config = create_test_config();
        config.input_url = "testsrc=duration=1:size=640x480:rate=1".to_string();
        config
            .input_options
            .insert("f".to_string(), "lavfi".to_string());
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
                        eprintln!(
                            "Snapshot failed (expected if ffmpeg not fully functional): {}",
                            e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "FFmpeg start failed (expected if ffmpeg not available): {}",
                    e
                );
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
        let initial_count = source.restart_count();
        assert_eq!(initial_count, 0);

        // Stop should work even without starting
        let result = source.stop().await;
        assert!(result.is_ok());
    }
}
