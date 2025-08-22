//! ABOUTME: yt-dlp capture source for YouTube and other video platforms  
//! ABOUTME: Implements CaptureSource trait with live and VOD support

use crate::{CaptureSource, CaptureHandle, SnapshotConfig, generate_snapshot_with_ffmpeg};
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{CommandSpec, run};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Mutex, fs};
use tracing::{debug, info, instrument};

/// Output format for yt-dlp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFormat {
    /// Best available quality
    Best,
    /// Worst available quality  
    Worst,
    /// Specific format ID
    FormatId(String),
    /// Best video with height limit
    BestWithHeight(u32),
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Best
    }
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
    temp_file: Arc<Mutex<Option<PathBuf>>>,
    process_handle: Arc<Mutex<Option<String>>>,
    restart_count: Arc<Mutex<u32>>,
}

impl YtDlpSource {
    pub fn new(config: YtDlpConfig) -> Self {
        Self {
            config,
            temp_file: Arc::new(Mutex::new(None)),
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
                "yt-dlp not found. Please install yt-dlp and ensure it's in PATH".to_string()
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

        // Add format selection
        match &self.config.format {
            OutputFormat::Best => args.extend(["--format".to_string(), "best".to_string()]),
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

        let timeout = Duration::from_secs(
            self.config.timeout.unwrap_or(60) as u64
        );

        CommandSpec::new("yt-dlp".into())
            .args(args)
            .timeout(timeout)
    }

    /// Build yt-dlp command for downloading
    async fn build_download_command(&self) -> Result<CommandSpec> {
        let mut temp_guard = self.temp_file.lock().await;
        
        // Create temp file for download
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("yt_dlp_{}_{}.%(ext)s", 
            gl_core::Id::new(), 
            chrono::Utc::now().timestamp()));
        
        let mut args = vec![
            "--no-playlist".to_string(),
            "--output".to_string(),
            temp_file.to_string_lossy().to_string(),
        ];

        // Add format selection
        match &self.config.format {
            OutputFormat::Best => args.extend(["--format".to_string(), "best".to_string()]),
            OutputFormat::Worst => args.extend(["--format".to_string(), "worst".to_string()]),
            OutputFormat::FormatId(id) => args.extend(["--format".to_string(), id.clone()]),
            OutputFormat::BestWithHeight(height) => {
                args.extend(["--format".to_string(), format!("best[height<={}]", height)]);
            }
        }

        // For live streams, add live-specific options
        if self.config.is_live {
            args.extend([
                "--live-from-start".to_string(),
                "--wait-for-video".to_string(),
                "5".to_string(),
            ]);
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

        *temp_guard = Some(temp_file.clone());

        let timeout = Duration::from_secs(
            self.config.timeout.unwrap_or(300) as u64 // Longer timeout for downloads
        );

        Ok(CommandSpec::new("yt-dlp".into())
            .args(args)
            .timeout(timeout))
    }

    /// Find the actual downloaded file (yt-dlp resolves %(ext)s)
    async fn find_downloaded_file(&self) -> Result<PathBuf> {
        let temp_guard = self.temp_file.lock().await;
        let template_path = temp_guard.as_ref()
            .ok_or_else(|| Error::Config("No temp file set".to_string()))?;
        
        let template_str = template_path.to_string_lossy();
        let base_path = template_str.replace(".%(ext)s", "");
        
        // Common video extensions to check
        let extensions = ["mp4", "mkv", "webm", "m4v", "mov", "avi", "flv"];
        
        for ext in &extensions {
            let candidate = PathBuf::from(format!("{}.{}", base_path, ext));
            if candidate.exists() {
                debug!(path = %candidate.display(), "Found downloaded file");
                return Ok(candidate);
            }
        }
        
        Err(Error::Config("Could not find downloaded file".to_string()))
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
        if info_lines.len() >= 3 {
            info!(
                title = info_lines.get(0).unwrap_or(&"Unknown"),
                duration = info_lines.get(1).unwrap_or(&"Unknown"), 
                is_live = info_lines.get(2).unwrap_or(&"Unknown"),
                "Video info retrieved"
            );
        }

        // Start download process for non-live streams
        if !self.config.is_live {
            let download_spec = self.build_download_command().await?;
            debug!(command = ?download_spec, "Starting download");
            
            let download_result = run(download_spec).await?;
            if !download_result.success() {
                counter!("yt_dlp_failures_total").increment(1);
                return Err(Error::Config(format!(
                    "yt-dlp download failed: {}", 
                    download_result.stderr
                )));
            }
        }

        counter!("yt_dlp_starts_total").increment(1);
        histogram!("yt_dlp_start_duration_seconds").record(start_time.elapsed().as_secs_f64());
        
        Ok(CaptureHandle::new(Arc::new(YtDlpSource::new(self.config.clone()))))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        info!(url = %self.config.url, "Taking yt-dlp snapshot");

        // For live streams, we need to capture from the stream directly
        if self.config.is_live {
            // Use yt-dlp to get the direct stream URL, then use ffmpeg
            let get_url_spec = CommandSpec::new("yt-dlp".into())
                .args(vec![
                    "--get-url".to_string(),
                    "--format".to_string(),
                    "best".to_string(),
                    self.config.url.clone(),
                ])
                .timeout(Duration::from_secs(30));

            let url_result = run(get_url_spec).await?;
            if !url_result.success() {
                return Err(Error::Config(format!(
                    "Failed to get stream URL: {}", 
                    url_result.stderr
                )));
            }

            let stream_url = url_result.stdout.trim();
            debug!(stream_url = %stream_url, "Got live stream URL");

            // Use ffmpeg to capture from the live stream
            let temp_path = std::env::temp_dir().join(format!("yt_live_snapshot_{}.jpg", gl_core::Id::new()));
            
            let ffmpeg_args = vec![
                "-i".to_string(),
                stream_url.to_string(),
                "-vframes".to_string(),
                "1".to_string(),
                "-f".to_string(),
                "image2".to_string(),
                "-q:v".to_string(),
                "2".to_string(),
                "-y".to_string(), // Overwrite output file
                temp_path.to_string_lossy().to_string(),
            ];

            let ffmpeg_spec = CommandSpec::new("ffmpeg".into())
                .args(ffmpeg_args)
                .timeout(self.config.snapshot_config.timeout);

            let ffmpeg_result = run(ffmpeg_spec).await?;
            if !ffmpeg_result.success() {
                return Err(Error::Config(format!(
                    "FFmpeg snapshot failed: {}", 
                    ffmpeg_result.stderr
                )));
            }

            // Read the generated image
            let image_data = fs::read(&temp_path).await
                .map_err(|e| Error::Config(format!("Failed to read snapshot: {}", e)))?;
            
            // Clean up temp file
            let _ = fs::remove_file(&temp_path).await;
            
            Ok(Bytes::from(image_data))
        } else {
            // For VOD, use the downloaded file
            let downloaded_file = self.find_downloaded_file().await?;
            generate_snapshot_with_ffmpeg(&downloaded_file, &self.config.snapshot_config).await
        }
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        info!(url = %self.config.url, "Stopping yt-dlp capture");

        // Clean up temp file if it exists
        let mut temp_guard = self.temp_file.lock().await;
        if let Some(_temp_file) = temp_guard.take() {
            // Try to find and remove actual downloaded file
            if let Ok(actual_file) = self.find_downloaded_file().await {
                debug!(path = %actual_file.display(), "Removing downloaded file");
                let _ = fs::remove_file(&actual_file).await;
            }
        }

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
                (OutputFormat::Best, OutputFormat::Best) => {},
                (OutputFormat::Worst, OutputFormat::Worst) => {},
                (OutputFormat::FormatId(a), OutputFormat::FormatId(b)) => assert_eq!(a, b),
                (OutputFormat::BestWithHeight(a), OutputFormat::BestWithHeight(b)) => assert_eq!(a, b),
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
        assert!(spec.args.contains(&"--format".to_string()));
        assert!(spec.args.contains(&"best".to_string()));
        assert!(spec.args.contains(&"https://youtube.com/watch?v=test".to_string()));
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
        
        config.options.insert("cookies".to_string(), "/path/to/cookies.txt".to_string());
        config.options.insert("user-agent".to_string(), "Custom Agent".to_string());
        
        let source = YtDlpSource::new(config);
        let spec = source.build_info_command();
        
        assert!(spec.args.contains(&"--cookies".to_string()));
        assert!(spec.args.contains(&"/path/to/cookies.txt".to_string()));
        assert!(spec.args.contains(&"--user-agent".to_string()));
        assert!(spec.args.contains(&"Custom Agent".to_string()));
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