//! ABOUTME: Integration tests for motion detection with synthetic frame pairs
//! ABOUTME: Tests known motion scenarios and algorithm performance comparisons

use gl_vision::{
    utils::{create_test_frame_pair, create_test_frame_with_motion, image_to_jpeg_bytes},
    MotionAlgorithm, MotionConfig, MotionDetectionService,
};

/// Test motion detection service creation with different algorithms
#[tokio::test]
async fn test_motion_service_creation() {
    // Test PixelDiff algorithm
    let config_pixel = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        ..Default::default()
    };
    let service = MotionDetectionService::new(config_pixel);
    assert!(service.is_ok());

    // Test MOG2 algorithm behavior based on feature availability
    #[cfg(feature = "heavy_opencv")]
    {
        let config_mog2 = MotionConfig {
            algorithm: MotionAlgorithm::Mog2,
            ..Default::default()
        };
        let service = MotionDetectionService::new(config_mog2);
        assert!(service.is_ok());
    }

    #[cfg(not(feature = "heavy_opencv"))]
    {
        // Should fall back to PixelDiff when OpenCV not available
        let config_mog2 = MotionConfig {
            algorithm: MotionAlgorithm::Mog2,
            ..Default::default()
        };
        let service = MotionDetectionService::new(config_mog2);
        assert!(service.is_ok());
    }
}

/// Test motion detection with known motion scenarios
#[tokio::test]
async fn test_known_motion_detection() {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.05,
        min_change_area: 50,
        downscale_factor: 1,
        max_width: 100,
        max_height: 100,
    };

    let mut service = MotionDetectionService::new(config).unwrap();

    // Create synthetic frame pair with known motion
    let (frame1, frame2) = create_test_frame_pair(100, 100);

    // Convert frames to JPEG bytes
    let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
    let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

    // First frame - baseline
    let result1 = service.detect_motion_from_bytes(&frame1_bytes).unwrap();
    assert!(!result1.motion_detected); // No previous frame
    assert_eq!(result1.changed_pixels, 0);
    assert_eq!(result1.confidence, 0.0);

    // Second frame - should detect motion
    let result2 = service.detect_motion_from_bytes(&frame2_bytes).unwrap();
    assert!(result2.motion_detected);
    assert!(result2.changed_pixels > 50); // Above min_change_area
    assert!(result2.confidence > 0.7);
    assert!(result2.change_ratio > 0.0);
    assert!(result2.processing_time_ms > 0);
    assert_eq!(result2.algorithm_used, "PixelDiff");
}

/// Test motion detection with no motion (identical frames)
#[tokio::test]
async fn test_no_motion_detection() {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: 100,
        max_height: 100,
    };

    let mut service = MotionDetectionService::new(config).unwrap();

    // Create identical frames
    let frame = create_test_frame_with_motion(100, 100, 20, 20, 30, 30, 200);
    let frame_bytes = image_to_jpeg_bytes(&frame).unwrap();

    // First frame - baseline
    let result1 = service.detect_motion_from_bytes(&frame_bytes).unwrap();
    assert!(!result1.motion_detected);

    // Identical second frame - no motion
    let result2 = service.detect_motion_from_bytes(&frame_bytes).unwrap();
    assert!(!result2.motion_detected);
    assert_eq!(result2.changed_pixels, 0);
    assert_eq!(result2.change_ratio, 0.0);
    assert!(result2.confidence < 0.5);
}

/// Test motion detection with different threshold sensitivities
#[tokio::test]
async fn test_threshold_sensitivity() {
    // High threshold - less sensitive
    let config_high = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.3,
        min_change_area: 10,
        downscale_factor: 1,
        max_width: 100,
        max_height: 100,
    };

    // Low threshold - more sensitive
    let config_low = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.05,
        min_change_area: 10,
        downscale_factor: 1,
        max_width: 100,
        max_height: 100,
    };

    let mut service_high = MotionDetectionService::new(config_high).unwrap();
    let mut service_low = MotionDetectionService::new(config_low).unwrap();

    // Create frames with subtle difference
    let frame1 = create_test_frame_with_motion(100, 100, 0, 0, 0, 0, 64);
    let frame2 = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 100); // Subtle change

    let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
    let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

    // Process with both services
    let _ = service_high
        .detect_motion_from_bytes(&frame1_bytes)
        .unwrap();
    let _ = service_low.detect_motion_from_bytes(&frame1_bytes).unwrap();

    let result_high = service_high
        .detect_motion_from_bytes(&frame2_bytes)
        .unwrap();
    let result_low = service_low.detect_motion_from_bytes(&frame2_bytes).unwrap();

    // Low threshold should be more sensitive to changes
    assert!(result_low.changed_pixels >= result_high.changed_pixels);
}

/// Test motion detection with different frame sizes and downscaling
#[tokio::test]
async fn test_downscaling_behavior() {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 25,
        downscale_factor: 4, // Downscale to 1/4 size
        max_width: 100,
        max_height: 100,
    };

    let mut service = MotionDetectionService::new(config).unwrap();

    // Create large frames that will be downscaled
    let frame1 = create_test_frame_with_motion(400, 400, 0, 0, 0, 0, 64);
    let frame2 = create_test_frame_with_motion(400, 400, 40, 40, 80, 80, 200);

    let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
    let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

    // Process frames
    let result1 = service.detect_motion_from_bytes(&frame1_bytes).unwrap();
    assert!(!result1.motion_detected);

    let result2 = service.detect_motion_from_bytes(&frame2_bytes).unwrap();

    // Should work with downscaled frames
    assert!(result2.total_pixels < 400 * 400); // Frames were downscaled
    assert!(result2.total_pixels <= 100 * 100); // Within max dimensions
}

/// Test motion detection service configuration updates
#[tokio::test]
async fn test_config_updates() {
    let initial_config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        ..Default::default()
    };

    let mut service = MotionDetectionService::new(initial_config).unwrap();
    assert_eq!(service.config().threshold, 0.1);

    // Update configuration
    let new_config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.2,
        min_change_area: 200,
        ..Default::default()
    };

    let update_result = service.update_config(new_config);
    assert!(update_result.is_ok());
    assert_eq!(service.config().threshold, 0.2);
    assert_eq!(service.config().min_change_area, 200);
}

/// Test motion detection with raw frame data
#[tokio::test]
async fn test_raw_frame_detection() {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: 100,
        max_height: 100,
    };

    let mut service = MotionDetectionService::new(config).unwrap();

    // Create raw grayscale frame data
    let frame1_data = vec![64u8; 100 * 100]; // Uniform gray
    let mut frame2_data = vec![64u8; 100 * 100];

    // Add motion region to second frame
    for y in 10..30 {
        for x in 10..30 {
            frame2_data[y * 100 + x] = 200; // Bright region
        }
    }

    // Process frames
    let result1 = service
        .detect_motion_from_frame(&frame1_data, 100, 100)
        .unwrap();
    assert!(!result1.motion_detected);

    let result2 = service
        .detect_motion_from_frame(&frame2_data, 100, 100)
        .unwrap();
    assert!(result2.motion_detected);
    assert!(result2.changed_pixels >= 100); // 20x20 = 400 pixels changed
    assert!(result2.confidence > 0.7);
}

/// Test motion detection service reset functionality
#[tokio::test]
async fn test_service_reset() {
    let config = MotionConfig::default();
    let mut service = MotionDetectionService::new(config).unwrap();

    // Process a frame to initialize internal state
    let frame = create_test_frame_with_motion(100, 100, 10, 10, 20, 20, 200);
    let frame_bytes = image_to_jpeg_bytes(&frame).unwrap();
    let _ = service.detect_motion_from_bytes(&frame_bytes).unwrap();

    // Reset service
    let reset_result = service.reset();
    assert!(reset_result.is_ok());

    // Next frame should behave like first frame (no previous frame)
    let result = service.detect_motion_from_bytes(&frame_bytes).unwrap();
    assert!(!result.motion_detected);
    assert_eq!(result.changed_pixels, 0);
}

/// Test error handling with invalid frame data
#[tokio::test]
async fn test_error_handling() {
    let config = MotionConfig::default();
    let mut service = MotionDetectionService::new(config).unwrap();

    // Test with invalid image data
    let invalid_data = vec![0u8; 10]; // Too small to be valid image
    let result = service.detect_motion_from_bytes(&invalid_data);
    assert!(result.is_err());

    // Test with mismatched frame dimensions
    let frame_data = vec![128u8; 50]; // Not matching declared dimensions
    let result = service.detect_motion_from_frame(&frame_data, 100, 100);
    assert!(result.is_err());
}

/// Performance test comparing different motion detection scenarios
/// Optional heavy benchmark; ignored by default.
/// Run with: `cargo test -p gl_vision -- --ignored` (use `--ignored --nocapture` to see timings).
#[ignore = "heavy benchmark; run with --ignored"]
#[tokio::test]
async fn test_motion_detection_performance() {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 2, // Moderate downscaling
        max_width: 200,
        max_height: 200,
    };

    let mut service = MotionDetectionService::new(config).unwrap();

    // Create larger test frames
    let frame1 = create_test_frame_with_motion(200, 200, 0, 0, 0, 0, 64);
    let frame2 = create_test_frame_with_motion(200, 200, 50, 50, 100, 100, 200);

    let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
    let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

    let start_time = std::time::Instant::now();

    // Process multiple frames to get average performance
    let mut total_processing_time = 0u64;
    let iterations = 10;

    for i in 0..iterations {
        let frame_bytes = if i % 2 == 0 {
            &frame1_bytes
        } else {
            &frame2_bytes
        };
        let result = service.detect_motion_from_bytes(frame_bytes).unwrap();
        total_processing_time += result.processing_time_ms;
    }

    let total_time = start_time.elapsed();
    let avg_processing_time = total_processing_time as f64 / iterations as f64;

    println!("Motion detection performance:");
    println!("  Total time: {:?}", total_time);
    println!("  Average processing time: {:.2}ms", avg_processing_time);
    println!("  Iterations: {}", iterations);

    // Performance should be reasonable (under 100ms per frame for this test size)
    assert!(
        avg_processing_time < 100.0,
        "Motion detection too slow: {:.2}ms",
        avg_processing_time
    );
}
