//! ABOUTME: Process runner for external commands with timeouts and logging
//! ABOUTME: Manages execution of ffmpeg, yt-dlp, and other external tools

use gl_core::{Error, Result};
use metrics::{counter, histogram};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};
use tracing::{debug, error, info, instrument, warn};

/// Maximum bytes to capture from stdout/stderr
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024; // 1MB

/// Command specification for process execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Path to the program to execute
    pub program: PathBuf,
    /// Command line arguments
    pub args: Vec<String>,
    /// Environment variables to set
    pub env: Vec<(String, String)>,
    /// Working directory for the command
    pub cwd: Option<PathBuf>,
    /// Timeout for command execution
    pub timeout: Duration,
    /// Additional time to wait after timeout before force kill
    pub kill_after: Duration,
    /// Maximum bytes to capture from stdout
    pub stdout_limit: Option<usize>,
    /// Maximum bytes to capture from stderr
    pub stderr_limit: Option<usize>,
}

impl CommandSpec {
    /// Create a new command spec with default timeout settings
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            timeout: Duration::from_secs(300), // 5 minutes default
            kill_after: Duration::from_secs(30), // 30 seconds grace period
            stdout_limit: Some(DEFAULT_OUTPUT_LIMIT),
            stderr_limit: Some(DEFAULT_OUTPUT_LIMIT),
        }
    }
    
    /// Add command line arguments
    pub fn args<I, S>(mut self, args: I) -> Self 
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.args.extend(args.into_iter().map(|s| s.as_ref().to_string()));
        self
    }
    
    /// Set environment variables from a HashMap
    pub fn env_map(mut self, env: HashMap<String, String>) -> Self {
        self.env = env.into_iter().collect();
        self
    }
    
    /// Add a single environment variable
    pub fn env_var<K, V>(mut self, key: K, value: V) -> Self
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        self.env.push((key.as_ref().to_string(), value.as_ref().to_string()));
        self
    }
    
    /// Set working directory
    pub fn cwd<P: Into<PathBuf>>(mut self, cwd: P) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
    
    /// Set timeout duration
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    
    /// Set kill grace period after timeout
    pub fn kill_after(mut self, kill_after: Duration) -> Self {
        self.kill_after = kill_after;
        self
    }
}

/// Result of command execution
#[derive(Debug)]
pub struct CommandResult {
    /// Exit status of the command
    pub status: ExitStatus,
    /// Captured stdout (bounded)
    pub stdout: String,
    /// Captured stderr (bounded)
    pub stderr: String,
    /// Total execution duration
    pub duration: Duration,
    /// Whether the command was killed due to timeout
    pub timed_out: bool,
    /// Whether output was truncated due to size limits
    pub stdout_truncated: bool,
    /// Whether stderr was truncated due to size limits
    pub stderr_truncated: bool,
}

impl CommandResult {
    /// Check if the command succeeded (exit code 0)
    pub fn success(&self) -> bool {
        self.status.success()
    }
    
    /// Get the exit code if available
    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }
}

/// Run a command according to the specification
#[instrument(skip(spec), fields(program = %spec.program.display(), args = ?spec.args))]
pub async fn run(spec: CommandSpec) -> Result<CommandResult> {
    let start = Instant::now();
    
    info!(
        program = %spec.program.display(),
        args = ?spec.args,
        timeout_secs = spec.timeout.as_secs(),
        "Starting command execution"
    );
    
    // Build the command
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    
    // Set environment variables
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    
    // Set working directory
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    
    // Spawn the process
    let mut child = cmd.spawn()
        .map_err(|e| Error::Config(format!("Failed to spawn command {}: {}", spec.program.display(), e)))?;
    
    // Get stdout and stderr handles
    let stdout = child.stdout.take()
        .ok_or_else(|| Error::Config("Failed to capture stdout".to_string()))?;
    let stderr = child.stderr.take()
        .ok_or_else(|| Error::Config("Failed to capture stderr".to_string()))?;
    
    // Create output capture tasks
    let stdout_task = capture_output(stdout, spec.stdout_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT), "stdout");
    let stderr_task = capture_output_with_logging(stderr, spec.stderr_limit.unwrap_or(DEFAULT_OUTPUT_LIMIT), "stderr");
    
    // Wait for either completion or timeout
    let execution_result = timeout(spec.timeout, async {
        // Wait for process completion and output capture concurrently
        let (status_result, (stdout_output, stdout_truncated), (stderr_output, stderr_truncated)) = tokio::join!(
            child.wait(),
            stdout_task,
            stderr_task
        );
        
        let status = status_result
            .map_err(|e| Error::Config(format!("Failed to wait for command: {}", e)))?;
            
        Ok::<_, Error>((status, stdout_output, stdout_truncated, stderr_output, stderr_truncated))
    }).await;
    
    let (status, stdout_output, stdout_truncated, stderr_output, stderr_truncated, timed_out) = match execution_result {
        Ok(Ok((status, stdout_output, stdout_truncated, stderr_output, stderr_truncated))) => {
            debug!("Command completed normally");
            (status, stdout_output, stdout_truncated, stderr_output, stderr_truncated, false)
        }
        Ok(Err(e)) => {
            error!(error = %e, "Command execution failed");
            return Err(e);
        }
        Err(_) => {
            warn!(timeout_secs = spec.timeout.as_secs(), "Command timed out, initiating graceful shutdown");
            
            // Send SIGTERM for graceful shutdown
            if let Err(e) = child.kill().await {
                warn!(error = %e, "Failed to send kill signal to process");
            }
            
            // Wait for kill_after duration for graceful shutdown
            let kill_result = timeout(spec.kill_after, child.wait()).await;
            
            match kill_result {
                Ok(Ok(status)) => {
                    info!("Command terminated gracefully after timeout");
                    // Since we timed out, we can't get the full output, return what we can
                    (status, String::new(), false, String::new(), false, true)
                }
                _ => {
                    error!("Command did not terminate gracefully, process may still be running");
                    // Create a mock exit status for timeout case
                    // We'll use a command that we know will fail to get a proper ExitStatus
                    let timeout_status = std::process::Command::new("false")
                        .status()
                        .unwrap_or_else(|_| std::process::Command::new("sh").args(["-c", "exit 1"]).status().unwrap());
                    (timeout_status, String::new(), false, String::new(), false, true)
                }
            }
        }
    };
    
    let duration = start.elapsed();
    
    let result = CommandResult {
        status,
        stdout: stdout_output,
        stderr: stderr_output,
        duration,
        timed_out,
        stdout_truncated,
        stderr_truncated,
    };
    
    // Record metrics
    let program_name = spec.program.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    
    histogram!("command_duration_seconds", 
        "program" => program_name.to_string())
        .record(duration.as_secs_f64());
    
    if result.timed_out {
        counter!("command_timeout_total", 
            "program" => program_name.to_string())
            .increment(1);
    }
    
    if result.success() {
        counter!("command_success_total", 
            "program" => program_name.to_string())
            .increment(1);
    } else {
        counter!("command_failure_total", 
            "program" => program_name.to_string(),
            "exit_code" => result.exit_code().unwrap_or(-1).to_string())
            .increment(1);
    }
    
    if result.success() && !result.timed_out {
        info!(
            duration_ms = duration.as_millis(),
            exit_code = result.exit_code(),
            "Command completed successfully"
        );
    } else {
        warn!(
            duration_ms = duration.as_millis(),
            exit_code = result.exit_code(),
            timed_out = result.timed_out,
            "Command failed or timed out"
        );
    }
    
    Ok(result)
}

/// Capture output from a stream with size limits
async fn capture_output(
    stream: tokio::process::ChildStdout,
    limit: usize,
    stream_name: &str,
) -> (String, bool) {
    let mut reader = BufReader::new(stream);
    let mut output = String::new();
    let mut buffer = String::new();
    let mut truncated = false;
    
    while output.len() < limit {
        buffer.clear();
        
        match reader.read_line(&mut buffer).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let remaining = limit - output.len();
                if buffer.len() > remaining {
                    output.push_str(&buffer[..remaining]);
                    truncated = true;
                    break;
                } else {
                    output.push_str(&buffer);
                }
            }
            Err(e) => {
                debug!(stream = stream_name, error = %e, "Error reading from stream");
                break;
            }
        }
    }
    
    if truncated {
        debug!(stream = stream_name, captured_bytes = output.len(), limit, "Output truncated due to size limit");
    }
    
    (output, truncated)
}

/// Capture output with live logging for stderr
async fn capture_output_with_logging(
    stream: tokio::process::ChildStderr,
    limit: usize,
    stream_name: &str,
) -> (String, bool) {
    let mut reader = BufReader::new(stream);
    let mut output = String::new();
    let mut buffer = String::new();
    let mut truncated = false;
    
    while output.len() < limit {
        buffer.clear();
        
        match reader.read_line(&mut buffer).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let line = buffer.trim_end();
                if !line.is_empty() {
                    // Log stderr lines as they arrive
                    debug!(stream = stream_name, line = %line, "Process stderr");
                }
                
                let remaining = limit - output.len();
                if buffer.len() > remaining {
                    output.push_str(&buffer[..remaining]);
                    truncated = true;
                    break;
                } else {
                    output.push_str(&buffer);
                }
            }
            Err(e) => {
                debug!(stream = stream_name, error = %e, "Error reading from stream");
                break;
            }
        }
    }
    
    if truncated {
        debug!(stream = stream_name, captured_bytes = output.len(), limit, "Output truncated due to size limit");
    }
    
    (output, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    
    #[tokio::test]
    async fn test_successful_command() {
        let spec = CommandSpec::new("echo".into())
            .args(["hello", "world"]);
            
        let result = run(spec).await.expect("Command should succeed");
        
        assert!(result.success());
        assert_eq!(result.stdout.trim(), "hello world");
        assert!(!result.timed_out);
        assert!(!result.stdout_truncated);
    }
    
    #[tokio::test]
    async fn test_command_with_env() {
        let spec = CommandSpec::new("sh".into())
            .args(["-c", "echo $TEST_VAR"])
            .env_var("TEST_VAR", "test_value");
            
        let result = run(spec).await.expect("Command should succeed");
        
        assert!(result.success());
        assert_eq!(result.stdout.trim(), "test_value");
    }
    
    #[tokio::test]
    async fn test_command_with_cwd() {
        let spec = CommandSpec::new("pwd".into())
            .cwd("/tmp");
            
        let result = run(spec).await.expect("Command should succeed");
        
        assert!(result.success());
        // On macOS, /tmp resolves to /private/tmp, so just check it ends with tmp
        assert!(result.stdout.trim().ends_with("tmp"));
    }
    
    #[tokio::test]
    async fn test_command_timeout() {
        let spec = CommandSpec::new("sleep".into())
            .args(["2"])
            .timeout(Duration::from_millis(100))
            .kill_after(Duration::from_millis(50));
            
        let result = run(spec).await.expect("Command should complete with timeout");
        
        assert!(result.timed_out);
        assert!(!result.success());
    }
    
    #[tokio::test]
    async fn test_output_truncation() {
        // Generate output larger than limit
        let large_text = "x".repeat(2000);
        let spec = CommandSpec::new("echo".into())
            .args([&large_text]);
        
        let mut spec = spec;
        spec.stdout_limit = Some(100);
        
        let result = run(spec).await.expect("Command should succeed");
        
        assert!(result.success());
        assert!(result.stdout_truncated);
        assert_eq!(result.stdout.len(), 100);
    }
    
    #[tokio::test]
    async fn test_failed_command() {
        let spec = CommandSpec::new("sh".into())
            .args(["-c", "exit 42"]);
            
        let result = run(spec).await.expect("Command should execute");
        
        assert!(!result.success());
        assert_eq!(result.exit_code(), Some(42));
        assert!(!result.timed_out);
    }
    
    #[tokio::test]
    async fn test_nonexistent_command() {
        let spec = CommandSpec::new("this_command_does_not_exist_12345".into());
        
        let result = run(spec).await;
        
        assert!(result.is_err());
    }
}
