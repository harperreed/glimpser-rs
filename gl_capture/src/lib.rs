//! ABOUTME: Capture engine for various media sources (ffmpeg, websites, files)
//! ABOUTME: Provides trait-based capture abstractions and implementations

use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use gl_proc::{run, CommandSpec};
use std::{
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use tracing::{debug, error, info, instrument, warn};

pub mod artifact_storage;
pub mod background_processor;
pub mod ffmpeg_source;
pub mod file_source;
pub mod hardware_accel;
pub mod process_pool;
pub mod streaming_source;
pub mod yt_dlp_source;

#[cfg(feature = "website")]
pub mod website_source;

pub use artifact_storage::{
    snapshot_and_store, ArtifactStorageConfig, ArtifactStorageService, StoredArtifact,
};
pub use background_processor::{
    BackgroundSnapshotProcessor, JobStatus, ProcessorStats, SnapshotJob,
};
pub use ffmpeg_source::{FfmpegConfig, FfmpegSource, HardwareAccel, RtspTransport};
pub use file_source::FileSource;
pub use process_pool::{
    FfmpegProcess, FfmpegProcessPool, ProcessHealth, ProcessPoolConfig, ProcessPoolMetrics,
};
pub use streaming_source::{StreamingFfmpegSource, StreamingSourceConfig};
pub use yt_dlp_source::{OutputFormat, YtDlpConfig, YtDlpSource};

#[cfg(feature = "website")]
pub use website_source::{WebDriverClient, WebsiteConfig, WebsiteSource};

/// Handle to a running capture session
/// When dropped, the capture should stop gracefully
pub struct CaptureHandle {
    source: Arc<dyn CaptureSource + Send + Sync>,
    state: Arc<Mutex<CaptureState>>,
    /// Runtime handle for synchronous cleanup in Drop
    /// This is wrapped in Option so we can take it during drop
    runtime_handle: Option<tokio::runtime::Handle>,
}

impl CaptureHandle {
    pub(crate) fn new(source: Arc<dyn CaptureSource + Send + Sync>) -> Self {
        // Capture the current runtime handle for use in Drop
        let runtime_handle = tokio::runtime::Handle::try_current().ok();

        if runtime_handle.is_none() {
            warn!("CaptureHandle created outside of tokio runtime - cleanup may not work properly");
        }

        Self {
            source,
            state: Arc::new(Mutex::new(CaptureState::Running)),
            runtime_handle,
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

        // CRITICAL FIX: Use blocking cleanup with timeout to ensure processes are stopped
        // This prevents FFmpeg process leaks during runtime shutdown
        if let Some(runtime_handle) = self.runtime_handle.take() {
            // Attempt cleanup with panic protection
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Strategy: For multi-threaded runtimes, spawn a dedicated thread to run block_on.
                // This prevents blocking the runtime's worker threads and avoids potential panics
                // from calling block_on within an async context.

                let cleanup_future = async move {
                    let mut guard = state.lock().await;
                    if matches!(*guard, CaptureState::Running) {
                        *guard = CaptureState::Stopped;
                        if let Err(e) = source.stop().await {
                            warn!(error = %e, "Failed to stop capture during drop");
                        }
                    }
                };

                if runtime_handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread
                {
                    // Current thread runtime - safe to block_on directly
                    // Give cleanup 5 seconds to complete
                    let timeout_result =
                        runtime_handle.block_on(tokio::time::timeout(
                            Duration::from_secs(5),
                            cleanup_future,
                        ));

                    if timeout_result.is_err() {
                        error!("Capture cleanup timed out during drop - process may be leaked");
                    }
                } else {
                    // Multi-threaded runtime - spawn a dedicated thread to avoid blocking workers
                    match std::thread::spawn(move || {
                        let timeout_result = runtime_handle.block_on(tokio::time::timeout(
                            Duration::from_secs(5),
                            cleanup_future,
                        ));

                        if timeout_result.is_err() {
                            error!("Capture cleanup timed out during drop - process may be leaked");
                        }
                    })
                    .join()
                    {
                        Err(e) => {
                            error!("Cleanup thread panicked during drop: {:?}", e);
                        }
                        Ok(()) => {
                            // Cleanup completed successfully
                        }
                    }
                }
            }));

            if let Err(e) = result {
                error!("Panic during capture cleanup: {:?}", e);
            }
        } else {
            // No runtime available - best effort cleanup
            // Log warning since we can't guarantee cleanup
            warn!("No runtime handle available for cleanup in Drop - process may leak");
        }
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

/// Cleanup orphaned FFmpeg processes from previous runs
/// This should be called on application startup to prevent accumulation of zombie processes
///
/// **Safety Note:** This function attempts to identify processes spawned by this application
/// by checking for specific patterns in the command line (pipe:1 output AND mjpeg format).
/// This is a best-effort heuristic and may not catch all cases. To avoid killing unrelated
/// processes, it requires BOTH patterns to be present AND checks that the process doesn't
/// have a parent process (orphaned).
#[instrument]
pub async fn cleanup_orphaned_ffmpeg_processes() -> Result<()> {
    info!("Cleaning up orphaned FFmpeg processes from previous runs");

    // Use pgrep to find all ffmpeg processes
    let pgrep_result = tokio::process::Command::new("pgrep")
        .arg("-f")
        .arg("ffmpeg")
        .output()
        .await;

    match pgrep_result {
        Ok(output) if output.status.success() => {
            let pids = String::from_utf8_lossy(&output.stdout);
            let current_pid = std::process::id();

            let mut killed_count = 0;
            let mut checked_count = 0;

            for pid_str in pids.lines() {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    // Don't kill our own process
                    if pid == current_pid {
                        continue;
                    }

                    checked_count += 1;

                    // Check if this is actually an ffmpeg process we spawned
                    // by checking the command line (Linux-specific /proc filesystem)
                    let cmdline_result =
                        tokio::fs::read_to_string(format!("/proc/{}/cmdline", pid)).await;

                    if let Ok(cmdline) = cmdline_result {
                        // Be more selective: require BOTH pipe:1 AND mjpeg to reduce false positives
                        // This matches our specific usage pattern
                        let has_pipe_output = cmdline.contains("pipe:1");
                        let has_mjpeg = cmdline.contains("mjpeg") || cmdline.contains("image2");

                        // Also check if the process is truly orphaned (no valid parent)
                        let is_orphaned = tokio::fs::read_to_string(format!("/proc/{}/stat", pid))
                            .await
                            .ok()
                            .and_then(|stat| {
                                // Parse the stat file to get PPID (parent process ID)
                                // Format: pid (comm) state ppid ...
                                // Note: comm is in parentheses and may contain spaces, so we need to
                                // find the closing paren and then split from there

                                // Find the last ')' to skip over the comm field
                                if let Some(rparen_pos) = stat.rfind(')') {
                                    // Everything after ')' is: state ppid ...
                                    let after_comm = &stat[rparen_pos + 1..];
                                    let parts: Vec<&str> = after_comm.split_whitespace().collect();

                                    // parts[0] = state, parts[1] = ppid
                                    if parts.len() >= 2 {
                                        parts[1].parse::<u32>().ok()
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .map(|ppid| ppid == 1) // PPID of 1 means orphaned (adopted by init)
                            .unwrap_or(false);

                        // Only kill if it matches our pattern AND is orphaned
                        if has_pipe_output && has_mjpeg && is_orphaned {
                            debug!(
                                pid,
                                "Killing orphaned FFmpeg process (matches our pattern)"
                            );

                            // Try to kill the orphaned process using signals
                            #[cfg(unix)]
                            {
                                // Try SIGTERM first for graceful shutdown
                                // Use tokio::process::Command for consistency
                                let kill_result = tokio::process::Command::new("kill")
                                    .arg("-TERM")
                                    .arg(pid.to_string())
                                    .status()
                                    .await;

                                if kill_result.is_ok() {
                                    killed_count += 1;

                                    // Wait a bit for graceful shutdown
                                    tokio::time::sleep(Duration::from_millis(100)).await;

                                    // Check if still running, use SIGKILL if needed
                                    if tokio::fs::metadata(format!("/proc/{}", pid))
                                        .await
                                        .is_ok()
                                    {
                                        warn!(
                                            pid,
                                            "Process didn't respond to SIGTERM, using SIGKILL"
                                        );
                                        let _ = tokio::process::Command::new("kill")
                                            .arg("-KILL")
                                            .arg(pid.to_string())
                                            .status()
                                            .await;
                                    }
                                }
                            }

                            #[cfg(not(unix))]
                            {
                                warn!(
                                    pid,
                                    "Orphaned process cleanup not supported on this platform"
                                );
                            }
                        } else if has_pipe_output || has_mjpeg {
                            // Log but don't kill processes that partially match
                            debug!(
                                pid,
                                has_pipe_output,
                                has_mjpeg,
                                is_orphaned,
                                "FFmpeg process found but not killed (doesn't match all criteria)"
                            );
                        }
                    }
                }
            }

            if killed_count > 0 {
                info!(
                    killed_count,
                    checked_count,
                    "Cleaned up orphaned FFmpeg processes"
                );
            } else {
                debug!(checked_count, "No orphaned FFmpeg processes found");
            }

            Ok(())
        }
        Ok(_) => {
            // pgrep returned no results (exit code 1) - no ffmpeg processes found
            debug!("No FFmpeg processes found to clean up");
            Ok(())
        }
        Err(e) => {
            // pgrep command failed - this might be on a system without pgrep
            warn!(
                error = %e,
                "Failed to run pgrep for orphaned process cleanup - continuing anyway"
            );
            Ok(())
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

    // Run FFmpeg in a blocking thread to avoid blocking the async executor
    let result = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(run(spec))
    })
    .await
    .map_err(|e| Error::Config(format!("Background FFmpeg task failed: {}", e)))??;

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
