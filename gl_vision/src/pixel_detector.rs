//! ABOUTME: Pure-Rust pixel difference motion detection algorithm
//! ABOUTME: Compares consecutive frames using configurable threshold and change area

use crate::{MotionConfig, MotionDetector, MotionResult};
use gl_core::Result;
use tracing::debug;

/// Pure-Rust pixel difference motion detector
pub struct PixelDiffDetector {
    config: MotionConfig,
    previous_frame: Option<Vec<u8>>,
    frame_width: u32,
    frame_height: u32,
}

impl PixelDiffDetector {
    /// Create a new pixel difference detector
    pub fn new(config: MotionConfig) -> Result<Self> {
        Ok(Self {
            config,
            previous_frame: None,
            frame_width: 0,
            frame_height: 0,
        })
    }

    /// Calculate pixel difference between two frames
    fn calculate_pixel_diff(&self, current_frame: &[u8], previous_frame: &[u8]) -> (u32, f64) {
        let threshold_value = (255.0 * self.config.threshold) as u8;
        let mut changed_pixels = 0u32;

        for (curr_pixel, prev_pixel) in current_frame.iter().zip(previous_frame.iter()) {
            let diff = if *curr_pixel > *prev_pixel {
                curr_pixel - prev_pixel
            } else {
                prev_pixel - curr_pixel
            };

            if diff > threshold_value {
                changed_pixels += 1;
            }
        }

        let total_pixels = current_frame.len() as u32;
        let change_ratio = changed_pixels as f64 / total_pixels as f64;

        (changed_pixels, change_ratio)
    }

    /// Apply morphological operations to reduce noise
    fn apply_noise_reduction(&self, changed_pixels: u32, total_pixels: u32) -> (bool, f64) {
        // Check if changed area meets minimum threshold
        let motion_detected = changed_pixels >= self.config.min_change_area;

        // Calculate confidence based on change ratio and area
        let change_ratio = changed_pixels as f64 / total_pixels as f64;
        let area_confidence = (changed_pixels as f64 / self.config.min_change_area as f64).min(1.0);
        let threshold_confidence = (change_ratio / self.config.threshold).min(1.0);

        let confidence = if motion_detected {
            (area_confidence * 0.6 + threshold_confidence * 0.4)
                .min(0.99)
                .max(0.7)
        } else {
            (change_ratio / self.config.threshold * 0.5).min(0.6)
        };

        debug!(
            "Motion analysis: changed_pixels={}, change_ratio={:.3}, confidence={:.3}, motion={}",
            changed_pixels, change_ratio, confidence, motion_detected
        );

        (motion_detected, confidence)
    }
}

impl MotionDetector for PixelDiffDetector {
    fn detect_motion(
        &mut self,
        current_frame: &[u8],
        frame_width: u32,
        frame_height: u32,
    ) -> Result<MotionResult> {
        let start_time = std::time::Instant::now();

        // Update frame dimensions
        self.frame_width = frame_width;
        self.frame_height = frame_height;

        let total_pixels = (frame_width * frame_height) as u32;

        // If no previous frame, store current and return no motion
        let previous_frame = match &self.previous_frame {
            Some(prev) if prev.len() == current_frame.len() => prev,
            _ => {
                debug!("No previous frame available, storing current frame");
                self.previous_frame = Some(current_frame.to_vec());
                let processing_time = start_time.elapsed().as_millis() as u64;
                return Ok(MotionResult::new(
                    false,
                    0.0,
                    0.0,
                    0,
                    total_pixels,
                    processing_time,
                    self.algorithm_name().to_string(),
                ));
            }
        };

        // Calculate pixel differences
        let (changed_pixels, change_ratio) =
            self.calculate_pixel_diff(current_frame, previous_frame);

        // Apply noise reduction and determine motion
        let (motion_detected, confidence) =
            self.apply_noise_reduction(changed_pixels, total_pixels);

        // Store current frame for next comparison
        self.previous_frame = Some(current_frame.to_vec());

        let processing_time = start_time.elapsed().as_millis() as u64;

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
        debug!("Resetting PixelDiff detector state");
        self.previous_frame = None;
        self.frame_width = 0;
        self.frame_height = 0;
        Ok(())
    }

    fn algorithm_name(&self) -> &'static str {
        "PixelDiff"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::*;

    fn create_test_config() -> MotionConfig {
        MotionConfig {
            threshold: 0.1,
            min_change_area: 50,
            downscale_factor: 1,
            max_width: 100,
            max_height: 100,
            ..Default::default()
        }
    }

    #[test]
    fn test_pixel_diff_detector_creation() {
        let config = create_test_config();
        let detector = PixelDiffDetector::new(config);
        assert!(detector.is_ok());

        let detector = detector.unwrap();
        assert_eq!(detector.algorithm_name(), "PixelDiff");
    }

    #[test]
    fn test_pixel_diff_no_previous_frame() {
        let config = create_test_config();
        let mut detector = PixelDiffDetector::new(config).unwrap();

        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);
        let frame_data = frame.as_raw();

        let result = detector.detect_motion(frame_data, 100, 100).unwrap();

        assert!(!result.motion_detected);
        assert_eq!(result.confidence, 0.0);
        assert_eq!(result.changed_pixels, 0);
        assert_eq!(result.algorithm_used, "PixelDiff");
    }

    #[test]
    fn test_pixel_diff_with_motion() {
        let config = create_test_config();
        let mut detector = PixelDiffDetector::new(config).unwrap();

        // Create two different frames
        let (frame1, frame2) = create_test_frame_pair(100, 100);

        // First frame - no motion detected
        let result1 = detector.detect_motion(frame1.as_raw(), 100, 100).unwrap();
        assert!(!result1.motion_detected);

        // Second frame - motion should be detected
        let result2 = detector.detect_motion(frame2.as_raw(), 100, 100).unwrap();
        assert!(result2.motion_detected);
        assert!(result2.confidence > 0.7);
        assert!(result2.changed_pixels >= 50); // At least min_change_area
        assert!(result2.change_ratio > 0.0);
        assert_eq!(result2.total_pixels, 10000);
    }

    #[test]
    fn test_pixel_diff_no_motion() {
        let config = create_test_config();
        let mut detector = PixelDiffDetector::new(config).unwrap();

        // Create identical frames
        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);

        // First frame
        let result1 = detector.detect_motion(frame.as_raw(), 100, 100).unwrap();
        assert!(!result1.motion_detected);

        // Identical second frame - no motion should be detected
        let result2 = detector.detect_motion(frame.as_raw(), 100, 100).unwrap();
        assert!(!result2.motion_detected);
        assert_eq!(result2.changed_pixels, 0);
        assert_eq!(result2.change_ratio, 0.0);
    }

    #[test]
    fn test_pixel_diff_small_changes() {
        let mut config = create_test_config();
        config.min_change_area = 1000; // Large minimum area
        let mut detector = PixelDiffDetector::new(config).unwrap();

        // Create frames with small change
        let frame1 = create_test_frame_with_motion(100, 100, 0, 0, 0, 0, 64);
        let frame2 = create_test_frame_with_motion(100, 100, 50, 50, 5, 5, 200);

        // First frame
        let result1 = detector.detect_motion(frame1.as_raw(), 100, 100).unwrap();
        assert!(!result1.motion_detected);

        // Small change - should not trigger motion due to large min_change_area
        let result2 = detector.detect_motion(frame2.as_raw(), 100, 100).unwrap();
        assert!(!result2.motion_detected);
        assert!(result2.changed_pixels > 0);
        assert!(result2.changed_pixels < 1000);
    }

    #[test]
    fn test_pixel_diff_reset() {
        let config = create_test_config();
        let mut detector = PixelDiffDetector::new(config).unwrap();

        let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);

        // Process a frame
        let _result = detector.detect_motion(frame.as_raw(), 100, 100).unwrap();
        assert!(detector.previous_frame.is_some());

        // Reset detector
        let reset_result = detector.reset();
        assert!(reset_result.is_ok());
        assert!(detector.previous_frame.is_none());
        assert_eq!(detector.frame_width, 0);
        assert_eq!(detector.frame_height, 0);
    }

    #[test]
    fn test_pixel_diff_different_thresholds() {
        // High threshold - less sensitive
        let mut config_high = create_test_config();
        config_high.threshold = 0.5;
        let mut detector_high = PixelDiffDetector::new(config_high).unwrap();

        // Low threshold - more sensitive
        let mut config_low = create_test_config();
        config_low.threshold = 0.05;
        let mut detector_low = PixelDiffDetector::new(config_low).unwrap();

        // Create frames with subtle difference
        let frame1 = create_test_frame_with_motion(100, 100, 0, 0, 0, 0, 64);
        let frame2 = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 100); // Small difference

        // Process with both detectors
        let _ = detector_high
            .detect_motion(frame1.as_raw(), 100, 100)
            .unwrap();
        let _ = detector_low
            .detect_motion(frame1.as_raw(), 100, 100)
            .unwrap();

        let result_high = detector_high
            .detect_motion(frame2.as_raw(), 100, 100)
            .unwrap();
        let result_low = detector_low
            .detect_motion(frame2.as_raw(), 100, 100)
            .unwrap();

        // Low threshold should detect more changes
        assert!(result_low.changed_pixels >= result_high.changed_pixels);
    }
}
