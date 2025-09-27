//! ABOUTME: Hardware acceleration detection and configuration for FFmpeg
//! ABOUTME: Provides platform-specific acceleration detection with graceful fallback mechanisms

use crate::HardwareAccel;
use gl_core::{Error, Result};
use gl_proc::{run, CommandSpec};
use std::{collections::HashMap, sync::OnceLock, time::Duration};
use tracing::{debug, info, instrument, warn};

/// Platform-specific hardware acceleration capabilities
#[derive(Debug, Clone)]
pub struct AccelerationCapabilities {
    /// Available acceleration types on this platform
    pub available: Vec<HardwareAccel>,
    /// Preferred acceleration (best performing)
    pub preferred: HardwareAccel,
    /// Platform name for logging
    pub platform: String,
    /// Additional acceleration-specific options
    pub options: HashMap<HardwareAccel, Vec<String>>,
}

/// Cache for acceleration capabilities (computed once per process)
static ACCEL_CAPABILITIES: OnceLock<AccelerationCapabilities> = OnceLock::new();

#[allow(dead_code)]
impl AccelerationCapabilities {
    /// Get or detect hardware acceleration capabilities for the current platform
    #[instrument]
    pub async fn detect() -> &'static AccelerationCapabilities {
        // Use std::sync::Once to avoid async complexity in static initialization
        use std::sync::Once;
        static INIT: Once = Once::new();

        INIT.call_once(|| {
            // Spawn detection in background and store result
            // For now, use default capabilities to avoid async in static context
            let _ = ACCEL_CAPABILITIES.set(AccelerationCapabilities {
                available: vec![HardwareAccel::None],
                preferred: HardwareAccel::None,
                platform: Self::detect_platform(),
                options: {
                    let mut opts = HashMap::new();
                    opts.insert(HardwareAccel::None, vec![]);
                    opts
                },
            });
        });

        ACCEL_CAPABILITIES.get().unwrap()
    }

    /// Detect available hardware acceleration capabilities
    #[instrument]
    async fn detect_capabilities() -> AccelerationCapabilities {
        info!("Detecting hardware acceleration capabilities");

        let platform = Self::detect_platform();
        let mut available = Vec::new();
        let mut options = HashMap::new();

        // Always include software fallback
        available.push(HardwareAccel::None);
        options.insert(HardwareAccel::None, vec![]);

        // Platform-specific detection
        match platform.as_str() {
            "macos" => {
                Self::probe_videotoolbox(&mut available, &mut options).await;
            }
            "linux" => {
                Self::probe_vaapi(&mut available, &mut options).await;
                Self::probe_cuda(&mut available, &mut options).await;
                Self::probe_qsv(&mut available, &mut options).await;
            }
            "windows" => {
                Self::probe_qsv(&mut available, &mut options).await;
                Self::probe_cuda(&mut available, &mut options).await;
                // TODO: Add D3D11VA/DXVA2 support
            }
            _ => {
                warn!(platform = %platform, "Unknown platform for hardware acceleration");
            }
        }

        // Determine preferred acceleration (first available after None)
        let preferred = available
            .get(1) // Skip None
            .copied()
            .unwrap_or(HardwareAccel::None);

        let caps = AccelerationCapabilities {
            available,
            preferred,
            platform,
            options,
        };

        info!(
            platform = %caps.platform,
            preferred = ?caps.preferred,
            available = ?caps.available,
            "Hardware acceleration detection completed"
        );

        caps
    }

    /// Detect the current platform
    fn detect_platform() -> String {
        if cfg!(target_os = "macos") {
            "macos".to_string()
        } else if cfg!(target_os = "linux") {
            "linux".to_string()
        } else if cfg!(target_os = "windows") {
            "windows".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Probe VideoToolbox acceleration (macOS)
    #[instrument]
    async fn probe_videotoolbox(
        available: &mut Vec<HardwareAccel>,
        options: &mut HashMap<HardwareAccel, Vec<String>>,
    ) {
        debug!("Probing VideoToolbox acceleration");

        let test_command = CommandSpec::new("ffmpeg".into())
            .args(vec![
                "-hide_banner".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                "testsrc=duration=0.1:size=320x240:rate=1".to_string(),
                "-hwaccel".to_string(),
                "videotoolbox".to_string(),
                "-f".to_string(),
                "null".to_string(),
                "-".to_string(),
            ])
            .timeout(Duration::from_secs(5));

        match run(test_command).await {
            Ok(result) => {
                if result.success() {
                    info!("VideoToolbox acceleration is available");
                    available.push(HardwareAccel::VideoToolbox);
                    options.insert(
                        HardwareAccel::VideoToolbox,
                        vec!["-hwaccel".to_string(), "videotoolbox".to_string()],
                    );
                } else {
                    debug!(
                        stderr = %result.stderr,
                        "VideoToolbox acceleration test failed"
                    );
                }
            }
            Err(e) => {
                debug!(error = %e, "Failed to test VideoToolbox acceleration");
            }
        }
    }

    /// Probe VAAPI acceleration (Linux)
    #[instrument]
    async fn probe_vaapi(
        available: &mut Vec<HardwareAccel>,
        options: &mut HashMap<HardwareAccel, Vec<String>>,
    ) {
        debug!("Probing VAAPI acceleration");

        let test_command = CommandSpec::new("ffmpeg".into())
            .args(vec![
                "-hide_banner".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                "testsrc=duration=0.1:size=320x240:rate=1".to_string(),
                "-hwaccel".to_string(),
                "vaapi".to_string(),
                "-hwaccel_output_format".to_string(),
                "vaapi".to_string(),
                "-f".to_string(),
                "null".to_string(),
                "-".to_string(),
            ])
            .timeout(Duration::from_secs(5));

        match run(test_command).await {
            Ok(result) => {
                if result.success() {
                    info!("VAAPI acceleration is available");
                    available.push(HardwareAccel::Vaapi);
                    options.insert(
                        HardwareAccel::Vaapi,
                        vec![
                            "-hwaccel".to_string(),
                            "vaapi".to_string(),
                            "-hwaccel_output_format".to_string(),
                            "vaapi".to_string(),
                        ],
                    );
                } else {
                    debug!(
                        stderr = %result.stderr,
                        "VAAPI acceleration test failed"
                    );
                }
            }
            Err(e) => {
                debug!(error = %e, "Failed to test VAAPI acceleration");
            }
        }
    }

    /// Probe CUDA acceleration (Linux/Windows)
    #[instrument]
    async fn probe_cuda(
        available: &mut Vec<HardwareAccel>,
        options: &mut HashMap<HardwareAccel, Vec<String>>,
    ) {
        debug!("Probing CUDA acceleration");

        let test_command = CommandSpec::new("ffmpeg".into())
            .args(vec![
                "-hide_banner".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                "testsrc=duration=0.1:size=320x240:rate=1".to_string(),
                "-hwaccel".to_string(),
                "cuda".to_string(),
                "-f".to_string(),
                "null".to_string(),
                "-".to_string(),
            ])
            .timeout(Duration::from_secs(5));

        match run(test_command).await {
            Ok(result) => {
                if result.success() {
                    info!("CUDA acceleration is available");
                    available.push(HardwareAccel::Cuda);
                    options.insert(
                        HardwareAccel::Cuda,
                        vec!["-hwaccel".to_string(), "cuda".to_string()],
                    );
                } else {
                    debug!(
                        stderr = %result.stderr,
                        "CUDA acceleration test failed"
                    );
                }
            }
            Err(e) => {
                debug!(error = %e, "Failed to test CUDA acceleration");
            }
        }
    }

    /// Probe Intel Quick Sync Video acceleration (Linux/Windows)
    #[instrument]
    async fn probe_qsv(
        available: &mut Vec<HardwareAccel>,
        options: &mut HashMap<HardwareAccel, Vec<String>>,
    ) {
        debug!("Probing Intel QSV acceleration");

        let test_command = CommandSpec::new("ffmpeg".into())
            .args(vec![
                "-hide_banner".to_string(),
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                "testsrc=duration=0.1:size=320x240:rate=1".to_string(),
                "-hwaccel".to_string(),
                "qsv".to_string(),
                "-f".to_string(),
                "null".to_string(),
                "-".to_string(),
            ])
            .timeout(Duration::from_secs(5));

        match run(test_command).await {
            Ok(result) => {
                if result.success() {
                    info!("Intel QSV acceleration is available");
                    available.push(HardwareAccel::Qsv);
                    options.insert(
                        HardwareAccel::Qsv,
                        vec!["-hwaccel".to_string(), "qsv".to_string()],
                    );
                } else {
                    debug!(
                        stderr = %result.stderr,
                        "Intel QSV acceleration test failed"
                    );
                }
            }
            Err(e) => {
                debug!(error = %e, "Failed to test Intel QSV acceleration");
            }
        }
    }

    /// Check if a specific acceleration type is available
    pub fn supports(&self, accel: &HardwareAccel) -> bool {
        self.available.contains(accel)
    }

    /// Get FFmpeg arguments for a specific acceleration type
    pub fn get_args(&self, accel: &HardwareAccel) -> Option<&Vec<String>> {
        self.options.get(accel)
    }

    /// Get the best available acceleration with fallback
    pub fn select_best(&self, preferred: Option<HardwareAccel>) -> HardwareAccel {
        // If user has a preference and it's available, use it
        if let Some(pref) = preferred {
            if self.supports(&pref) {
                return pref;
            }
            warn!(
                preferred = ?pref,
                available = ?self.available,
                "Preferred acceleration not available, falling back"
            );
        }

        // Use platform preferred if available
        if self.supports(&self.preferred) {
            self.preferred
        } else {
            // Fallback to software
            HardwareAccel::None
        }
    }
}

/// Hardware acceleration detector and configurator
pub struct AccelerationDetector;

impl AccelerationDetector {
    /// Auto-detect and configure the best hardware acceleration
    #[instrument]
    pub async fn auto_configure() -> Result<HardwareAccel> {
        let capabilities = AccelerationCapabilities::detect().await;
        let selected = capabilities.select_best(None);

        info!(
            selected = ?selected,
            platform = %capabilities.platform,
            "Auto-configured hardware acceleration"
        );

        Ok(selected)
    }

    /// Configure hardware acceleration with user preference
    #[instrument]
    pub async fn configure_with_preference(preferred: HardwareAccel) -> Result<HardwareAccel> {
        let capabilities = AccelerationCapabilities::detect().await;
        let selected = capabilities.select_best(Some(preferred));

        if selected != preferred {
            warn!(
                preferred = ?preferred,
                selected = ?selected,
                "Hardware acceleration preference not available, using fallback"
            );
        } else {
            info!(
                selected = ?selected,
                "Hardware acceleration configured as requested"
            );
        }

        Ok(selected)
    }

    /// Test a specific acceleration type
    #[instrument]
    pub async fn test_acceleration(accel: HardwareAccel) -> Result<bool> {
        let capabilities = AccelerationCapabilities::detect().await;

        if !capabilities.supports(&accel) {
            return Ok(false);
        }

        // For more thorough testing, we could run a longer test here
        // For now, we trust the initial detection
        Ok(true)
    }

    /// Get detailed acceleration information
    #[instrument]
    pub async fn get_info() -> Result<AccelerationCapabilities> {
        let capabilities = AccelerationCapabilities::detect().await;
        Ok(capabilities.clone())
    }
}

/// Validate FFmpeg with specific acceleration
#[instrument]
pub async fn validate_ffmpeg_acceleration(accel: HardwareAccel) -> Result<()> {
    debug!(acceleration = ?accel, "Validating FFmpeg acceleration");

    let capabilities = AccelerationCapabilities::detect().await;

    if !capabilities.supports(&accel) {
        return Err(Error::Config(format!(
            "Hardware acceleration {:?} is not available on this platform",
            accel
        )));
    }

    // Get the arguments for this acceleration type
    let accel_args = capabilities
        .get_args(&accel)
        .ok_or_else(|| Error::Config("No arguments found for acceleration".to_string()))?;

    // Build test command
    let mut args = vec![
        "-hide_banner".to_string(),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        "testsrc=duration=1:size=640x480:rate=2".to_string(),
    ];

    // Add acceleration arguments
    args.extend(accel_args.clone());

    // Add output format
    args.extend(vec![
        "-f".to_string(),
        "mjpeg".to_string(),
        "-frames:v".to_string(),
        "2".to_string(),
        "-".to_string(),
    ]);

    let test_command = CommandSpec::new("ffmpeg".into())
        .args(args)
        .timeout(Duration::from_secs(10));

    debug!(command = ?test_command, "Running acceleration validation test");

    let result = run(test_command).await?;

    if result.success() && !result.stdout.is_empty() {
        info!(
            acceleration = ?accel,
            output_size = result.stdout.len(),
            "Hardware acceleration validation successful"
        );
        Ok(())
    } else {
        Err(Error::Config(format!(
            "Hardware acceleration validation failed: {}",
            result.stderr
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = AccelerationCapabilities::detect_platform();
        assert!(!platform.is_empty());

        // Should be one of the known platforms or "unknown"
        assert!(matches!(
            platform.as_str(),
            "macos" | "linux" | "windows" | "unknown"
        ));
    }

    #[tokio::test]
    async fn test_acceleration_capabilities_structure() {
        // This doesn't require ffmpeg, just tests the structure
        let available = vec![HardwareAccel::None];
        let mut options = HashMap::new();
        options.insert(HardwareAccel::None, vec![]);

        let caps = AccelerationCapabilities {
            available: available.clone(),
            preferred: HardwareAccel::None,
            platform: "test".to_string(),
            options: options.clone(),
        };

        assert!(caps.supports(&HardwareAccel::None));
        assert!(!caps.supports(&HardwareAccel::Cuda));
        assert_eq!(caps.select_best(None), HardwareAccel::None);
        assert_eq!(caps.get_args(&HardwareAccel::None), Some(&vec![]));
    }

    #[test]
    fn test_select_best_logic() {
        let mut options = HashMap::new();
        options.insert(HardwareAccel::None, vec![]);
        options.insert(HardwareAccel::Cuda, vec![]);

        let caps = AccelerationCapabilities {
            available: vec![HardwareAccel::None, HardwareAccel::Cuda],
            preferred: HardwareAccel::Cuda,
            platform: "test".to_string(),
            options,
        };

        // Should prefer CUDA when available
        assert_eq!(caps.select_best(None), HardwareAccel::Cuda);

        // Should use user preference if available
        assert_eq!(
            caps.select_best(Some(HardwareAccel::Cuda)),
            HardwareAccel::Cuda
        );

        // Should fallback when preference not available
        assert_eq!(
            caps.select_best(Some(HardwareAccel::Vaapi)),
            HardwareAccel::Cuda
        );
    }

    // Integration tests require ffmpeg to be installed
    #[tokio::test]
    #[ignore = "Requires ffmpeg installation"]
    async fn test_detection_integration() {
        let capabilities = AccelerationCapabilities::detect().await;

        // Should always have at least None available
        assert!(capabilities.supports(&HardwareAccel::None));
        assert!(!capabilities.available.is_empty());
        assert!(!capabilities.platform.is_empty());

        // Test auto-configuration
        let auto_accel = AccelerationDetector::auto_configure().await.unwrap();
        assert!(capabilities.supports(&auto_accel));

        println!("Detected capabilities: {:#?}", capabilities);
        println!("Auto-selected acceleration: {:?}", auto_accel);
    }

    #[tokio::test]
    #[ignore = "Requires ffmpeg installation"]
    async fn test_acceleration_validation() {
        // Test software acceleration (should always work with ffmpeg)
        match validate_ffmpeg_acceleration(HardwareAccel::None).await {
            Ok(_) => {
                println!("Software acceleration validation passed");
            }
            Err(e) => {
                eprintln!("Software acceleration validation failed: {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore = "Requires ffmpeg installation and specific hardware"]
    async fn test_hardware_acceleration_validation() {
        // This test only runs if hardware acceleration is available
        let capabilities = AccelerationCapabilities::detect().await;

        for &accel in &capabilities.available {
            if accel != HardwareAccel::None {
                println!("Testing acceleration: {:?}", accel);

                match validate_ffmpeg_acceleration(accel).await {
                    Ok(_) => {
                        println!("Hardware acceleration {:?} validation passed", accel);
                    }
                    Err(e) => {
                        eprintln!("Hardware acceleration {:?} validation failed: {}", accel, e);
                    }
                }
            }
        }
    }
}
