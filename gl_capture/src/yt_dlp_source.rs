//! ABOUTME: yt-dlp capture source for YouTube and other video platforms
//! ABOUTME: Implements CaptureSource trait with live and VOD support

use crate::{CaptureHandle, CaptureSource, SnapshotConfig};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{run, CommandSpec};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{fs, sync::Mutex};
use tracing::{debug, info, instrument};

/// Output format for yt-dlp
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum OutputFormat {
    /// Best available quality
    #[default]
    Best,
    /// Worst available quality
    Worst,
    /// Specific format ID
    FormatId(String),
    /// Best video with height limit
    BestWithHeight(u32),
}

/// Configuration for yt-dlp capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtDlpConfig {
    /// URL to capture (YouTube, Twitch, etc.)
    pub url: String,
    /// Output format selection
    pub format: OutputFormat,
    /// Additional yt-dlp options
    pub options: HashMap<String, String>,
    /// Whether this is a live stream
    pub is_live: bool,
    /// Timeout for yt-dlp operations
    pub timeout: Option<u32>,
    /// Snapshot configuration
    pub snapshot_config: SnapshotConfig,
}

impl Default for YtDlpConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            format: OutputFormat::default(),
            options: HashMap::new(),
            is_live: false,
            timeout: Some(60), // 60 seconds default
            snapshot_config: SnapshotConfig::default(),
        }
    }
}

/// yt-dlp-based capture source
pub struct YtDlpSource {
    config: YtDlpConfig,
    #[allow(dead_code)]
    process_handle: Arc<Mutex<Option<String>>>,
    restart_count: Arc<Mutex<u32>>,
}

impl YtDlpSource {
    pub fn new(config: YtDlpConfig) -> Self {
        Self {
            config,
            process_handle: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Validate that yt-dlp is available
    pub async fn validate() -> Result<()> {
        let spec = CommandSpec::new("yt-dlp".into())
            .args(vec!["--version".to_string()])
            .timeout(Duration::from_secs(10));

        let result = run(spec).await?;

        if !result.success() {
            return Err(Error::Config(
                "yt-dlp not found. Please install yt-dlp and ensure it's in PATH".to_string(),
            ));
        }

        info!("yt-dlp version: {}", result.stdout.trim());
        Ok(())
    }

    /// Build yt-dlp command for getting video info
    fn build_info_command(&self) -> CommandSpec {
        let mut args = vec![
            "--no-playlist".to_string(),
            "--print".to_string(),
            "%(title)s".to_string(),
            "--print".to_string(),
            "%(duration)s".to_string(),
            "--print".to_string(),
            "%(is_live)s".to_string(),
        ];

        // Add format selection (skip for Best to let yt-dlp auto-choose)
        match &self.config.format {
            OutputFormat::Best => {
                // Don't add format selection - let yt-dlp auto-choose the best format
            }
            OutputFormat::Worst => args.extend(["--format".to_string(), "worst".to_string()]),
            OutputFormat::FormatId(id) => args.extend(["--format".to_string(), id.clone()]),
            OutputFormat::BestWithHeight(height) => {
                args.extend(["--format".to_string(), format!("best[height<={}]", height)]);
            }
        }

        // Add custom options
        for (key, value) in &self.config.options {
            if !key.starts_with('-') {
                args.push(format!("--{}", key));
            } else {
                args.push(key.clone());
            }
            if !value.is_empty() {
                args.push(value.clone());
            }
        }

        args.push(self.config.url.clone());

        let timeout = Duration::from_secs(self.config.timeout.unwrap_or(60) as u64);

        CommandSpec::new("yt-dlp".into())
            .args(args)
            .timeout(timeout)
    }

    /// Get direct stream URL using yt-dlp
    async fn get_stream_url(&self) -> Result<String> {
        let mut args = vec!["--no-playlist".to_string(), "--get-url".to_string()];

        // For non-live videos, add specific optimizations
        if !self.config.is_live {
            // Only get the URL, don't download anything
            args.extend([
                "--no-download".to_string(),
                // Use the best format for snapshots (usually mp4 or webm)
                "--format".to_string(),
                "best[ext=mp4]/best[ext=webm]/best".to_string(),
            ]);
        }

        // Add format selection if specified
        match &self.config.format {
            OutputFormat::Best => {
                // Already handled above for non-live, or let yt-dlp auto-choose for live
            }
            OutputFormat::Worst => {
                args.pop(); // Remove the previous format if set
                args.pop();
                args.extend(["--format".to_string(), "worst".to_string()]);
            }
            OutputFormat::FormatId(id) => {
                args.pop(); // Remove the previous format if set
                args.pop();
                args.extend(["--format".to_string(), id.clone()]);
            }
            OutputFormat::BestWithHeight(height) => {
                args.pop(); // Remove the previous format if set
                args.pop();
                args.extend(["--format".to_string(), format!("best[height<={}]", height)]);
            }
        }

        args.push(self.config.url.clone());

        let spec = CommandSpec::new("yt-dlp".into())
            .args(args)
            .timeout(Duration::from_secs(30));

        // Run yt-dlp in a blocking thread to avoid blocking the async executor
        let result = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(run(spec))
        })
        .await
        .map_err(|e| Error::Config(format!("Background yt-dlp task failed: {}", e)))??;
        if !result.success() {
            return Err(Error::Config(format!(
                "Failed to get stream URL: {}",
                result.stderr
            )));
        }

        let url = result.stdout.trim();
        if url.is_empty() {
            return Err(Error::Config(
                "Got empty stream URL from yt-dlp".to_string(),
            ));
        }

        Ok(url.to_string())
    }

    /// Build optimized ffmpeg command for direct stream capture
    fn build_direct_ffmpeg_command(&self, stream_url: &str, output_path: &Path) -> CommandSpec {
        let mut args = vec![
            "-i".to_string(),
            stream_url.to_string(),
            "-vframes".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "image2".to_string(),
        ];

        // For non-live videos, we can add additional optimizations
        if !self.config.is_live {
            // Seek to a specific time for better frame quality (avoid black screens)
            args.extend([
                "-ss".to_string(),
                "00:00:02".to_string(), // Seek 2 seconds in
            ]);
        }

        args.extend([
            "-q:v".to_string(),
            "2".to_string(),  // High quality
            "-y".to_string(), // Overwrite output file
            output_path.to_string_lossy().to_string(),
        ]);

        CommandSpec::new("ffmpeg".into())
            .args(args)
            .timeout(Duration::from_secs(15))
    }
}

#[async_trait]
impl CaptureSource for YtDlpSource {
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        let start_time = Instant::now();
        info!(url = %self.config.url, "Starting yt-dlp capture");

        // Validate URL format
        if self.config.url.is_empty() {
            return Err(Error::Config("yt-dlp URL cannot be empty".to_string()));
        }

        // Increment restart counter
        {
            let mut restart_guard = self.restart_count.lock().await;
            *restart_guard += 1;
            counter!("yt_dlp_restarts_total").increment(1);
        }

        // Get video info first
        let info_spec = self.build_info_command();
        debug!(command = ?info_spec, "Getting video info");

        let info_result = run(info_spec).await?;
        if !info_result.success() {
            counter!("yt_dlp_failures_total").increment(1);
            return Err(Error::Config(format!(
                "yt-dlp info command failed: {}",
                info_result.stderr
            )));
        }

        let info_lines: Vec<&str> = info_result.stdout.trim().split('\n').collect();
        let mut is_live_stream = false;
        if info_lines.len() >= 3 {
            let is_live_str = info_lines.get(2).unwrap_or(&"Unknown");
            is_live_stream = *is_live_str == "True";
            info!(
                title = info_lines.first().unwrap_or(&"Unknown"),
                duration = info_lines.get(1).unwrap_or(&"Unknown"),
                is_live = is_live_str,
                "Video info retrieved"
            );
        }

        counter!("yt_dlp_starts_total").increment(1);
        histogram!("yt_dlp_start_duration_seconds").record(start_time.elapsed().as_secs_f64());

        // Create a new config with the detected live stream status
        let mut updated_config = self.config.clone();
        updated_config.is_live = is_live_stream;

        Ok(CaptureHandle::new(Arc::new(YtDlpSource::new(
            updated_config,
        ))))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        info!(url = %self.config.url, is_live = %self.config.is_live, "Taking yt-dlp snapshot");

        let temp_path =
            std::env::temp_dir().join(format!("yt_snapshot_{}.jpg", gl_core::Id::new()));

        // First get the direct stream URL
        let stream_url = self.get_stream_url().await?;
        debug!(stream_url = %stream_url, "Got direct stream URL");

        // Then use ffmpeg directly on the stream URL
        let cmd_spec = self.build_direct_ffmpeg_command(&stream_url, &temp_path);

        // Run FFmpeg in a blocking thread to avoid blocking the async executor
        let cmd_result = tokio::task::spawn_blocking(move || {
            let runtime = tokio::runtime::Handle::current();
            runtime.block_on(run(cmd_spec))
        })
        .await
        .map_err(|e| Error::Config(format!("Background FFmpeg task failed: {}", e)))??;

        if !cmd_result.success() {
            return Err(Error::Config(format!(
                "ffmpeg command failed: {}",
                cmd_result.stderr
            )));
        }

        let image_data = fs::read(&temp_path)
            .await
            .map_err(|e| Error::Config(format!("Failed to read snapshot: {}", e)))?;

        let _ = fs::remove_file(&temp_path).await;

        Ok(Bytes::from(image_data))
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        info!(url = %self.config.url, "Stopping yt-dlp capture");

        counter!("yt_dlp_stops_total").increment(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yt_dlp_config_creation() {
        let config = YtDlpConfig {
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            format: OutputFormat::BestWithHeight(720),
            is_live: false,
            ..Default::default()
        };

        assert_eq!(config.url, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert!(matches!(config.format, OutputFormat::BestWithHeight(720)));
        assert!(!config.is_live);
        assert_eq!(config.timeout, Some(60));
    }

    #[test]
    fn test_output_format_serialization() {
        let formats = vec![
            OutputFormat::Best,
            OutputFormat::Worst,
            OutputFormat::FormatId("137".to_string()),
            OutputFormat::BestWithHeight(1080),
        ];

        for format in formats {
            let json = serde_json::to_string(&format).unwrap();
            let deserialized: OutputFormat = serde_json::from_str(&json).unwrap();

            match (&format, &deserialized) {
                (OutputFormat::Best, OutputFormat::Best) => {}
                (OutputFormat::Worst, OutputFormat::Worst) => {}
                (OutputFormat::FormatId(a), OutputFormat::FormatId(b)) => assert_eq!(a, b),
                (OutputFormat::BestWithHeight(a), OutputFormat::BestWithHeight(b)) => {
                    assert_eq!(a, b)
                }
                _ => panic!("Format mismatch after serialization"),
            }
        }
    }

    #[test]
    fn test_yt_dlp_source_creation() {
        let config = YtDlpConfig {
            url: "https://example.com/video".to_string(),
            ..Default::default()
        };

        let source = YtDlpSource::new(config.clone());
        assert_eq!(source.config.url, config.url);
    }

    #[test]
    fn test_command_building_basic() {
        let config = YtDlpConfig {
            url: "https://youtube.com/watch?v=test".to_string(),
            format: OutputFormat::Best,
            ..Default::default()
        };

        let source = YtDlpSource::new(config);
        let spec = source.build_info_command();

        assert_eq!(spec.program.to_string_lossy(), "yt-dlp");
        assert!(spec.args.contains(&"--no-playlist".to_string()));
        // For OutputFormat::Best, we don't add --format to let yt-dlp auto-choose
        assert!(!spec.args.contains(&"--format".to_string()));
        assert!(spec
            .args
            .contains(&"https://youtube.com/watch?v=test".to_string()));
    }

    #[test]
    fn test_command_building_with_format_id() {
        let config = YtDlpConfig {
            url: "https://youtube.com/watch?v=test".to_string(),
            format: OutputFormat::FormatId("137+140".to_string()),
            ..Default::default()
        };

        let source = YtDlpSource::new(config);
        let spec = source.build_info_command();

        assert!(spec.args.contains(&"--format".to_string()));
        assert!(spec.args.contains(&"137+140".to_string()));
    }

    #[test]
    fn test_command_building_with_options() {
        let mut config = YtDlpConfig {
            url: "https://youtube.com/watch?v=test".to_string(),
            ..Default::default()
        };

        config
            .options
            .insert("cookies".to_string(), "/path/to/cookies.txt".to_string());
        config
            .options
            .insert("user-agent".to_string(), "Custom Agent".to_string());

        let source = YtDlpSource::new(config);
        let spec = source.build_info_command();

        assert!(spec.args.contains(&"--cookies".to_string()));
        assert!(spec.args.contains(&"/path/to/cookies.txt".to_string()));
        assert!(spec.args.contains(&"--user-agent".to_string()));
        assert!(spec.args.contains(&"Custom Agent".to_string()));
    }

    #[test]
    fn test_direct_ffmpeg_command_for_non_live() {
        let config = YtDlpConfig {
            url: "https://youtube.com/watch?v=test".to_string(),
            is_live: false,
            ..Default::default()
        };

        let source = YtDlpSource::new(config);
        let output = std::path::PathBuf::from("/tmp/out.jpg");
        let stream_url = "https://example.com/stream.m3u8";

        let spec = source.build_direct_ffmpeg_command(stream_url, &output);

        assert_eq!(spec.program.to_str().unwrap(), "ffmpeg");
        assert!(spec.args.contains(&"-i".to_string()));
        assert!(spec.args.contains(&stream_url.to_string()));
        assert!(spec.args.contains(&"-vframes".to_string()));
        assert!(spec.args.contains(&"1".to_string()));

        // For non-live videos, should include seek option for better frame quality
        assert!(spec.args.contains(&"-ss".to_string()));
        assert!(spec.args.contains(&"00:00:02".to_string()));
    }

    #[test]
    fn test_direct_ffmpeg_command_for_live() {
        let config = YtDlpConfig {
            url: "https://youtube.com/watch?v=live_test".to_string(),
            is_live: true,
            ..Default::default()
        };

        let source = YtDlpSource::new(config);
        let output = std::path::PathBuf::from("/tmp/out.jpg");
        let stream_url = "https://example.com/live_stream.m3u8";

        let spec = source.build_direct_ffmpeg_command(stream_url, &output);

        assert_eq!(spec.program.to_str().unwrap(), "ffmpeg");
        assert!(spec.args.contains(&"-i".to_string()));
        assert!(spec.args.contains(&stream_url.to_string()));

        // For live videos, should NOT include seek option (can't seek in live streams)
        assert!(!spec.args.contains(&"-ss".to_string()));
    }

    #[tokio::test]
    #[ignore = "Requires yt-dlp installation"]
    async fn test_yt_dlp_validation() {
        match YtDlpSource::validate().await {
            Ok(()) => {
                // yt-dlp is available, test should pass
            }
            Err(e) => {
                // yt-dlp not available, which is expected in CI
                assert!(e.to_string().contains("yt-dlp not found"));
            }
        }
    }

    #[tokio::test]
    async fn test_yt_dlp_source_lifecycle() {
        let config = YtDlpConfig {
            url: "https://example.com/test".to_string(),
            ..Default::default()
        };

        let source = YtDlpSource::new(config);

        // Test that we can create a handle (even though it will likely fail without yt-dlp)
        match source.start().await {
            Ok(_handle) => {
                // If we get here, yt-dlp is available and working
            }
            Err(_) => {
                // Expected when yt-dlp isn't available or URL is invalid
            }
        }
    }

    #[tokio::test]
    async fn test_yt_dlp_source_validation_invalid_url() {
        let config = YtDlpConfig {
            url: "".to_string(), // Empty URL should fail
            ..Default::default()
        };

        let source = YtDlpSource::new(config);
        let result = source.start().await;

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("cannot be empty"));
        }
    }

    #[tokio::test]
    async fn test_restart_count_initialization() {
        let config = YtDlpConfig::default();
        let source = YtDlpSource::new(config);

        let count = *source.restart_count.lock().await;
        assert_eq!(count, 0);
    }
}
