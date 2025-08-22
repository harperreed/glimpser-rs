//! ABOUTME: Motion detection with pure-Rust pixel-diff and optional OpenCV MOG2 algorithms
//! ABOUTME: Analyzes video frames for motion with configurable thresholds and runtime selection

use gl_core::Result;
use image::{GrayImage, ImageBuffer, Luma};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

#[cfg(feature = "heavy_opencv")]
pub mod opencv_detector;
pub mod pixel_detector;

#[cfg(feature = "heavy_opencv")]
pub use opencv_detector::OpenCvDetector;
pub use pixel_detector::PixelDiffDetector;

// Re-export image types for benchmarks
pub use image;

/// Configuration for motion detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionConfig {
    /// Detection algorithm to use
    pub algorithm: MotionAlgorithm,
    /// Minimum change threshold (0.0 to 1.0)
    pub threshold: f64,
    /// Downscale factor for processing (1 = full size, 2 = half size, etc.)
    pub downscale_factor: u32,
    /// Maximum width after downscaling
    pub max_width: u32,
    /// Maximum height after downscaling
    pub max_height: u32,
    /// Minimum area of change to trigger motion (pixels)
    pub min_change_area: u32,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            algorithm: MotionAlgorithm::PixelDiff,
            threshold: 0.1,
            downscale_factor: 4,
            max_width: 320,
            max_height: 240,
            min_change_area: 100,
        }
    }
}

/// Available motion detection algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MotionAlgorithm {
    /// Pure-Rust pixel difference algorithm
    PixelDiff,
    /// OpenCV MOG2 background subtraction (requires heavy_opencv feature)
    Mog2,
}

impl Default for MotionAlgorithm {
    fn default() -> Self {
        Self::PixelDiff
    }
}

/// Result of motion detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionResult {
    /// Whether motion was detected
    pub motion_detected: bool,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
    /// Change ratio (0.0 to 1.0)
    pub change_ratio: f64,
    /// Number of changed pixels
    pub changed_pixels: u32,
    /// Total pixels analyzed
    pub total_pixels: u32,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Algorithm used for detection
    pub algorithm_used: String,
}

impl MotionResult {
    /// Create a new motion result
    pub fn new(
        motion_detected: bool,
        confidence: f64,
        change_ratio: f64,
        changed_pixels: u32,
        total_pixels: u32,
        processing_time_ms: u64,
        algorithm_used: String,
    ) -> Self {
        Self {
            motion_detected,
            confidence,
            change_ratio,
            changed_pixels,
            total_pixels,
            processing_time_ms,
            algorithm_used,
        }
    }
}

/// Trait for motion detection algorithms
pub trait MotionDetector: Send + Sync {
    /// Detect motion between two frames
    fn detect_motion(
        &mut self,
        current_frame: &[u8],
        frame_width: u32,
        frame_height: u32,
    ) -> Result<MotionResult>;

    /// Reset the detector state
    fn reset(&mut self) -> Result<()>;

    /// Get algorithm name
    fn algorithm_name(&self) -> &'static str;
}

/// Main motion detection service
pub struct MotionDetectionService {
    config: MotionConfig,
    detector: Box<dyn MotionDetector>,
}

impl MotionDetectionService {
    /// Create a new motion detection service
    pub fn new(config: MotionConfig) -> Result<Self> {
        let detector: Box<dyn MotionDetector> = match config.algorithm {
            MotionAlgorithm::PixelDiff => {
                info!("Creating PixelDiff motion detector");
                Box::new(PixelDiffDetector::new(config.clone())?)
            }
            MotionAlgorithm::Mog2 => {
                #[cfg(feature = "heavy_opencv")]
                {
                    info!("Creating OpenCV MOG2 motion detector");
                    Box::new(OpenCvDetector::new(config.clone())?)
                }
                #[cfg(not(feature = "heavy_opencv"))]
                {
                    warn!("MOG2 requested but heavy_opencv feature not enabled, falling back to PixelDiff");
                    Box::new(PixelDiffDetector::new(config.clone())?)
                }
            }
        };

        Ok(Self { config, detector })
    }

    /// Detect motion in a frame (JPEG/PNG bytes)
    pub fn detect_motion_from_bytes(&mut self, image_data: &[u8]) -> Result<MotionResult> {
        let start_time = std::time::Instant::now();

        // Decode image
        let img = image::load_from_memory(image_data)
            .map_err(|e| gl_core::Error::Validation(format!("Failed to decode image: {}", e)))?;

        // Convert to grayscale
        let gray_img = img.to_luma8();

        // Downscale if needed
        let processed_img = self.downscale_image(&gray_img)?;

        let mut result = self.detector.detect_motion(
            processed_img.as_raw(),
            processed_img.width(),
            processed_img.height(),
        )?;

        result.processing_time_ms = start_time.elapsed().as_millis() as u64;

        debug!(
            "Motion detection completed: {} in {}ms",
            if result.motion_detected {
                "MOTION"
            } else {
                "NO_MOTION"
            },
            result.processing_time_ms
        );

        Ok(result)
    }

    /// Detect motion from raw grayscale frame data
    pub fn detect_motion_from_frame(
        &mut self,
        frame_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<MotionResult> {
        let start_time = std::time::Instant::now();

        // Create grayscale image from raw data
        let gray_img =
            GrayImage::from_raw(width, height, frame_data.to_vec()).ok_or_else(|| {
                gl_core::Error::Validation("Invalid frame data dimensions".to_string())
            })?;

        // Downscale if needed
        let processed_img = self.downscale_image(&gray_img)?;

        let mut result = self.detector.detect_motion(
            processed_img.as_raw(),
            processed_img.width(),
            processed_img.height(),
        )?;

        result.processing_time_ms = start_time.elapsed().as_millis() as u64;

        debug!(
            "Motion detection completed: {} in {}ms",
            if result.motion_detected {
                "MOTION"
            } else {
                "NO_MOTION"
            },
            result.processing_time_ms
        );

        Ok(result)
    }

    /// Reset detector state
    pub fn reset(&mut self) -> Result<()> {
        self.detector.reset()
    }

    /// Get current configuration
    pub fn config(&self) -> &MotionConfig {
        &self.config
    }

    /// Update configuration (creates new detector)
    pub fn update_config(&mut self, config: MotionConfig) -> Result<()> {
        let new_service = Self::new(config)?;
        *self = new_service;
        Ok(())
    }

    /// Downscale image according to configuration
    fn downscale_image(&self, img: &GrayImage) -> Result<GrayImage> {
        let (orig_width, orig_height) = img.dimensions();

        // Calculate target dimensions
        let target_width = (orig_width / self.config.downscale_factor).min(self.config.max_width);
        let target_height =
            (orig_height / self.config.downscale_factor).min(self.config.max_height);

        if target_width == orig_width && target_height == orig_height {
            return Ok(img.clone());
        }

        debug!(
            "Downscaling image from {}x{} to {}x{}",
            orig_width, orig_height, target_width, target_height
        );

        let resized = image::imageops::resize(
            img,
            target_width,
            target_height,
            image::imageops::FilterType::Nearest,
        );

        Ok(resized)
    }
}

/// Utility functions for image processing
pub mod utils {
    use super::*;

    /// Create a synthetic frame with motion in specified region
    pub fn create_test_frame_with_motion(
        width: u32,
        height: u32,
        motion_x: u32,
        motion_y: u32,
        motion_width: u32,
        motion_height: u32,
        intensity: u8,
    ) -> GrayImage {
        let mut img = ImageBuffer::from_pixel(width, height, Luma([64u8])); // Dark gray background

        // Add motion region
        for y in motion_y..(motion_y + motion_height).min(height) {
            for x in motion_x..(motion_x + motion_width).min(width) {
                img.put_pixel(x, y, Luma([intensity]));
            }
        }

        img
    }

    /// Create a synthetic frame pair for testing
    pub fn create_test_frame_pair(width: u32, height: u32) -> (GrayImage, GrayImage) {
        let frame1 = ImageBuffer::from_pixel(width, height, Luma([64u8]));
        let frame2 = create_test_frame_with_motion(width, height, 10, 10, 50, 50, 200);
        (frame1, frame2)
    }

    /// Convert image to JPEG bytes for testing
    pub fn image_to_jpeg_bytes(img: &GrayImage) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let rgb_img = image::DynamicImage::ImageLuma8(img.clone()).to_rgb8();
        rgb_img
            .write_to(
                &mut std::io::Cursor::new(&mut buffer),
                image::ImageFormat::Jpeg,
            )
            .map_err(|e| gl_core::Error::Validation(format!("Failed to encode JPEG: {}", e)))?;
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::utils::*;
    use super::*;

    #[test]
    fn test_motion_config_default() {
        let config = MotionConfig::default();
        assert_eq!(config.threshold, 0.1);
        assert_eq!(config.downscale_factor, 4);
        assert_eq!(config.max_width, 320);
        assert_eq!(config.max_height, 240);
        assert!(matches!(config.algorithm, MotionAlgorithm::PixelDiff));
    }

    #[test]
    fn test_motion_config_serialization() {
        let config = MotionConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MotionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.threshold, deserialized.threshold);
        assert_eq!(config.downscale_factor, deserialized.downscale_factor);
    }

    #[tokio::test]
    async fn test_motion_detection_service_creation() {
        let config = MotionConfig::default();
        let service = MotionDetectionService::new(config);
        assert!(service.is_ok());

        let service = service.unwrap();
        assert_eq!(service.config().threshold, 0.1);
    }

    #[test]
    fn test_create_test_frame_with_motion() {
        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);
        assert_eq!(frame.dimensions(), (100, 100));

        // Check background pixel
        assert_eq!(frame.get_pixel(5, 5).0[0], 64);

        // Check motion region pixel
        assert_eq!(frame.get_pixel(15, 15).0[0], 200);
    }

    #[test]
    fn test_create_test_frame_pair() {
        let (frame1, frame2) = create_test_frame_pair(100, 100);
        assert_eq!(frame1.dimensions(), (100, 100));
        assert_eq!(frame2.dimensions(), (100, 100));

        // Frame1 should be uniform
        assert_eq!(frame1.get_pixel(50, 50).0[0], 64);

        // Frame2 should have motion region
        assert_eq!(frame2.get_pixel(20, 20).0[0], 200);
    }

    #[test]
    fn test_image_to_jpeg_bytes() {
        let frame = create_test_frame_with_motion(50, 50, 5, 5, 10, 10, 200);
        let jpeg_bytes = image_to_jpeg_bytes(&frame);
        assert!(jpeg_bytes.is_ok());

        let bytes = jpeg_bytes.unwrap();
        assert!(!bytes.is_empty());
        assert!(bytes.len() > 100); // JPEG should have some reasonable size
    }
}
