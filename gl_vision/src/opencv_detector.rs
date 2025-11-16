//! ABOUTME: OpenCV MOG2 background subtraction motion detection algorithm
//! ABOUTME: Uses advanced computer vision techniques for robust motion detection

#[cfg(feature = "heavy_opencv")]
use crate::{MotionConfig, MotionDetector, MotionResult};
#[cfg(feature = "heavy_opencv")]
use gl_core::Result;
#[cfg(feature = "heavy_opencv")]
use opencv::{
    core::{Mat, Size, CV_8UC1},
    prelude::*,
    video::{BackgroundSubtractorMOG2, BackgroundSubtractorMOG2Trait},
};
#[cfg(feature = "heavy_opencv")]
use tracing::debug;

#[cfg(feature = "heavy_opencv")]
/// OpenCV MOG2 background subtraction motion detector
pub struct OpenCvDetector {
    config: MotionConfig,
    mog2: opencv::core::Ptr<dyn BackgroundSubtractorMOG2Trait>,
    initialized: bool,
}

#[cfg(feature = "heavy_opencv")]
impl OpenCvDetector {
    /// Create a new OpenCV MOG2 detector
    pub fn new(config: MotionConfig) -> Result<Self> {
        // Create MOG2 background subtractor with optimized parameters
        let mog2 = opencv::video::create_background_subtractor_mog2(
            500,  // history - number of frames to store
            16.0, // varThreshold - threshold for the squared Mahalanobis distance
            true, // detectShadows - detect and remove shadows
        )
        .map_err(|e| gl_core::Error::Database(format!("Failed to create MOG2 detector: {}", e)))?;

        debug!("Created OpenCV MOG2 detector");

        Ok(Self {
            config,
            mog2,
            initialized: false,
        })
    }

    /// Count non-zero pixels in the foreground mask
    fn count_foreground_pixels(&self, mask: &Mat) -> Result<u32> {
        let non_zero = opencv::core::count_non_zero(mask).map_err(|e| {
            gl_core::Error::Database(format!("Failed to count non-zero pixels: {}", e))
        })?;
        Ok(non_zero as u32)
    }

    /// Apply morphological operations to clean up the mask
    fn post_process_mask(&self, mask: &mut Mat) -> Result<()> {
        // Create morphological kernel
        let kernel = opencv::imgproc::get_structuring_element(
            opencv::imgproc::MORPH_ELLIPSE,
            Size::new(3, 3),
            opencv::core::Point::new(-1, -1),
        )
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to create morphological kernel: {}", e))
        })?;

        // Apply opening (erosion followed by dilation) to remove noise
        let mut temp = Mat::default();
        opencv::imgproc::morphology_ex(
            mask,
            &mut temp,
            opencv::imgproc::MORPH_OPEN,
            &kernel,
            opencv::core::Point::new(-1, -1),
            1,
            opencv::core::BORDER_CONSTANT,
            opencv::imgproc::morphology_default_border_value().unwrap(),
        )
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to apply morphological opening: {}", e))
        })?;

        // Apply closing (dilation followed by erosion) to fill holes
        opencv::imgproc::morphology_ex(
            &temp,
            mask,
            opencv::imgproc::MORPH_CLOSE,
            &kernel,
            opencv::core::Point::new(-1, -1),
            1,
            opencv::core::BORDER_CONSTANT,
            opencv::imgproc::morphology_default_border_value().unwrap(),
        )
        .map_err(|e| {
            gl_core::Error::Database(format!("Failed to apply morphological closing: {}", e))
        })?;

        Ok(())
    }
}

#[cfg(feature = "heavy_opencv")]
impl MotionDetector for OpenCvDetector {
    fn detect_motion(
        &mut self,
        current_frame: &[u8],
        frame_width: u32,
        frame_height: u32,
    ) -> Result<MotionResult> {
        let start_time = std::time::Instant::now();

        // Create OpenCV Mat from raw grayscale data
        let frame_mat = unsafe {
            Mat::new_rows_cols_with_data(
                frame_height as i32,
                frame_width as i32,
                CV_8UC1,
                current_frame.as_ptr() as *mut std::ffi::c_void,
                opencv::core::Mat_AUTO_STEP,
            )
            .map_err(|e| gl_core::Error::Database(format!("Failed to create OpenCV Mat: {}", e)))?
        };

        // Apply background subtraction
        let mut foreground_mask = Mat::default();
        self.mog2
            .apply(&frame_mat, &mut foreground_mask, -1.0)
            .map_err(|e| gl_core::Error::Database(format!("Failed to apply MOG2: {}", e)))?;

        // Post-process the mask to reduce noise
        self.post_process_mask(&mut foreground_mask)?;

        // Count foreground pixels
        let changed_pixels = self.count_foreground_pixels(&foreground_mask)?;
        let total_pixels = (frame_width * frame_height) as u32;
        let change_ratio = changed_pixels as f64 / total_pixels as f64;

        // Determine motion based on configuration
        let motion_detected =
            changed_pixels >= self.config.min_change_area && change_ratio >= self.config.threshold;

        // Calculate confidence based on change ratio and consistency
        let confidence = if motion_detected {
            let area_confidence =
                (changed_pixels as f64 / self.config.min_change_area as f64).min(1.0);
            let threshold_confidence = (change_ratio / self.config.threshold).min(1.0);
            (area_confidence * 0.5 + threshold_confidence * 0.5)
                .min(0.99)
                .max(0.8)
        } else {
            (change_ratio / self.config.threshold * 0.6).min(0.7)
        };

        // Mark as initialized after first frame
        if !self.initialized {
            self.initialized = true;
        }

        let processing_time = start_time.elapsed().as_millis() as u64;

        debug!("MOG2 motion analysis: changed_pixels={}, change_ratio={:.3}, confidence={:.3}, motion={}",
               changed_pixels, change_ratio, confidence, motion_detected);

        Ok(MotionResult::new(
            motion_detected,
            confidence,
            change_ratio,
            changed_pixels,
            total_pixels,
            processing_time,
            self.algorithm_name().to_string(),
        ))
    }

    fn reset(&mut self) -> Result<()> {
        debug!("Resetting OpenCV MOG2 detector state");

        // Recreate the MOG2 detector to reset its internal state
        self.mog2 =
            opencv::video::create_background_subtractor_mog2(500, 16.0, true).map_err(|e| {
                gl_core::Error::Database(format!("Failed to recreate MOG2 detector: {}", e))
            })?;

        self.initialized = false;
        Ok(())
    }

    fn algorithm_name(&self) -> &'static str {
        "MOG2"
    }
}

// Stub implementation when OpenCV feature is not enabled
#[cfg(not(feature = "heavy_opencv"))]
pub struct OpenCvDetector;

#[cfg(not(feature = "heavy_opencv"))]
impl OpenCvDetector {
    pub fn new(_config: crate::MotionConfig) -> Result<Self> {
        Err(gl_core::Error::Config(
            "OpenCV not available - heavy_opencv feature not enabled".to_string(),
        ))
    }
}

#[cfg(all(test, feature = "heavy_opencv"))]
mod tests {
    use super::*;
    use crate::utils::*;

    fn create_test_config() -> MotionConfig {
        MotionConfig {
            threshold: 0.05,
            min_change_area: 100,
            downscale_factor: 1,
            max_width: 100,
            max_height: 100,
            ..Default::default()
        }
    }

    #[test]
    fn test_opencv_detector_creation() {
        let config = create_test_config();
        let detector = OpenCvDetector::new(config);
        assert!(detector.is_ok());

        let detector = detector.unwrap();
        assert_eq!(detector.algorithm_name(), "MOG2");
        assert!(!detector.initialized);
    }

    #[test]
    fn test_opencv_detector_first_frame() {
        let config = create_test_config();
        let mut detector = OpenCvDetector::new(config).unwrap();

        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);
        let frame_data = frame.as_raw();

        let result = detector.detect_motion(frame_data, 100, 100).unwrap();

        // First frame typically shows high motion as background is being learned
        assert_eq!(result.algorithm_used, "MOG2");
        assert_eq!(result.total_pixels, 10000);
        assert!(detector.initialized);
    }

    #[test]
    fn test_opencv_detector_with_motion() {
        let config = create_test_config();
        let mut detector = OpenCvDetector::new(config).unwrap();

        // Create stable background first
        let background = create_test_frame_with_motion(100, 100, 0, 0, 0, 0, 64);

        // Process several background frames to establish baseline
        for _ in 0..10 {
            let _ = detector
                .detect_motion(background.as_raw(), 100, 100)
                .unwrap();
        }

        // Now introduce motion
        let motion_frame = create_test_frame_with_motion(100, 100, 20, 20, 30, 30, 200);
        let result = detector
            .detect_motion(motion_frame.as_raw(), 100, 100)
            .unwrap();

        // Should detect motion after background is learned
        assert!(result.changed_pixels > 0);
        assert!(result.change_ratio > 0.0);
        assert!(result.processing_time_ms > 0);
    }

    #[test]
    fn test_opencv_detector_reset() {
        let config = create_test_config();
        let mut detector = OpenCvDetector::new(config).unwrap();

        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);

        // Process a frame
        let _result = detector.detect_motion(frame.as_raw(), 100, 100).unwrap();
        assert!(detector.initialized);

        // Reset detector
        let reset_result = detector.reset();
        assert!(reset_result.is_ok());
        assert!(!detector.initialized);
    }

    #[test]
    fn test_count_foreground_pixels() {
        let config = create_test_config();
        let detector = OpenCvDetector::new(config).unwrap();

        // Create a simple binary mask
        let mask_data = vec![0u8; 10000]; // 100x100 black mask
        let mask = unsafe {
            Mat::new_rows_cols_with_data(
                100,
                100,
                CV_8UC1,
                mask_data.as_ptr() as *mut std::ffi::c_void,
                opencv::core::Mat_AUTO_STEP,
            )
            .unwrap()
        };

        let count = detector.count_foreground_pixels(&mask).unwrap();
        assert_eq!(count, 0); // All black pixels
    }
}

#[cfg(all(test, not(feature = "heavy_opencv")))]
mod tests_without_opencv {
    use super::*;

    #[test]
    fn test_opencv_detector_creation_without_feature() {
        let config = crate::MotionConfig::default();
        let detector = OpenCvDetector::new(config);
        assert!(detector.is_err());

        let err = detector.unwrap_err();
        assert!(matches!(err, gl_core::Error::Config(_)));
    }
}
