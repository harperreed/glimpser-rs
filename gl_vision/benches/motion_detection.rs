//! ABOUTME: Benchmark tests comparing motion detection algorithm performance
//! ABOUTME: Uses criterion for statistical analysis of PixelDiff vs MOG2 performance

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use gl_vision::{
    utils::{create_test_frame_pair, create_test_frame_with_motion, image_to_jpeg_bytes},
    MotionAlgorithm, MotionConfig, MotionDetectionService,
};

/// Benchmark pixel difference algorithm performance
fn bench_pixel_diff_algorithm(c: &mut Criterion) {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: 320,
        max_height: 240,
    };

    let mut group = c.benchmark_group("pixel_diff_performance");

    // Test different frame sizes
    let frame_sizes = vec![
        (64, 64, "64x64"),
        (128, 128, "128x128"),
        (320, 240, "320x240"),
        (640, 480, "640x480"),
    ];

    for (width, height, size_name) in frame_sizes {
        let mut service = MotionDetectionService::new(config.clone()).unwrap();

        // Create test frame pair with known motion
        let frame1 = create_test_frame_with_motion(width, height, 0, 0, 0, 0, 64);
        let frame2 = create_test_frame_with_motion(width, height, 10, 10, 50, 50, 200);

        let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
        let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

        // Initialize with first frame
        let _ = service.detect_motion_from_bytes(&frame1_bytes).unwrap();

        group.bench_with_input(
            BenchmarkId::new("from_bytes", size_name),
            &frame2_bytes,
            |b, frame_bytes| {
                b.iter(|| {
                    service.detect_motion_from_bytes(frame_bytes).unwrap();
                });
            },
        );

        // Benchmark raw frame processing
        let frame2_raw = frame2.as_raw();
        group.bench_with_input(
            BenchmarkId::new("from_raw", size_name),
            frame2_raw,
            |b, frame_data| {
                b.iter(|| {
                    service
                        .detect_motion_from_frame(frame_data, width, height)
                        .unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark OpenCV MOG2 algorithm performance (if available)
#[cfg(feature = "heavy_opencv")]
fn bench_mog2_algorithm(c: &mut Criterion) {
    let config = MotionConfig {
        algorithm: MotionAlgorithm::Mog2,
        threshold: 0.05,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: 320,
        max_height: 240,
    };

    let mut group = c.benchmark_group("mog2_performance");

    let frame_sizes = vec![
        (128, 128, "128x128"),
        (320, 240, "320x240"),
        (640, 480, "640x480"),
    ];

    for (width, height, size_name) in frame_sizes {
        let mut service = MotionDetectionService::new(config.clone()).unwrap();

        // Create test frame sequence for MOG2 to learn background
        let background = create_test_frame_with_motion(width, height, 0, 0, 0, 0, 64);
        let motion_frame = create_test_frame_with_motion(width, height, 20, 20, 100, 100, 200);

        let background_bytes = image_to_jpeg_bytes(&background).unwrap();
        let motion_bytes = image_to_jpeg_bytes(&motion_frame).unwrap();

        // Initialize MOG2 with several background frames
        for _ in 0..5 {
            let _ = service.detect_motion_from_bytes(&background_bytes).unwrap();
        }

        group.bench_with_input(
            BenchmarkId::new("from_bytes", size_name),
            &motion_bytes,
            |b, frame_bytes| {
                b.iter(|| {
                    service.detect_motion_from_bytes(frame_bytes).unwrap();
                });
            },
        );

        // Benchmark raw frame processing
        let motion_raw = motion_frame.as_raw();
        group.bench_with_input(
            BenchmarkId::new("from_raw", size_name),
            motion_raw,
            |b, frame_data| {
                b.iter(|| {
                    service
                        .detect_motion_from_frame(frame_data, width, height)
                        .unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Compare algorithm performance head-to-head
fn bench_algorithm_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("algorithm_comparison");

    let test_size = (320u32, 240u32);
    let (width, height) = test_size;

    // PixelDiff configuration
    let pixel_config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: width,
        max_height: height,
    };

    let mut pixel_service = MotionDetectionService::new(pixel_config).unwrap();

    // Create test frames
    let (frame1, frame2) = create_test_frame_pair(width, height);
    let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
    let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

    // Initialize PixelDiff
    let _ = pixel_service
        .detect_motion_from_bytes(&frame1_bytes)
        .unwrap();

    group.bench_function("pixel_diff_320x240", |b| {
        b.iter(|| {
            pixel_service
                .detect_motion_from_bytes(&frame2_bytes)
                .unwrap();
        });
    });

    #[cfg(feature = "heavy_opencv")]
    {
        // MOG2 configuration
        let mog2_config = MotionConfig {
            algorithm: MotionAlgorithm::Mog2,
            threshold: 0.05,
            min_change_area: 100,
            downscale_factor: 1,
            max_width: width,
            max_height: height,
        };

        let mut mog2_service = MotionDetectionService::new(mog2_config).unwrap();

        // Initialize MOG2 with background
        for _ in 0..5 {
            let _ = mog2_service
                .detect_motion_from_bytes(&frame1_bytes)
                .unwrap();
        }

        group.bench_function("mog2_320x240", |b| {
            b.iter(|| {
                mog2_service
                    .detect_motion_from_bytes(&frame2_bytes)
                    .unwrap();
            });
        });
    }

    group.finish();
}

/// Benchmark downscaling performance impact
fn bench_downscaling_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("downscaling_performance");

    let base_config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 100,
        downscale_factor: 1,
        max_width: 1000,
        max_height: 1000,
    };

    // Test different downscaling factors
    let downscale_factors = vec![1, 2, 4, 8];
    let test_frame_size = (640u32, 480u32);

    for factor in downscale_factors {
        let config = MotionConfig {
            downscale_factor: factor,
            ..base_config.clone()
        };

        let mut service = MotionDetectionService::new(config).unwrap();

        let (frame1, frame2) = create_test_frame_pair(test_frame_size.0, test_frame_size.1);
        let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
        let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

        // Initialize
        let _ = service.detect_motion_from_bytes(&frame1_bytes).unwrap();

        group.bench_with_input(
            BenchmarkId::new("downscale_factor", factor),
            &frame2_bytes,
            |b, frame_bytes| {
                b.iter(|| {
                    service.detect_motion_from_bytes(frame_bytes).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark threshold sensitivity impact on performance
fn bench_threshold_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("threshold_performance");

    let thresholds = vec![0.01, 0.05, 0.1, 0.2, 0.5];
    let test_frame_size = (320u32, 240u32);

    for threshold in thresholds {
        let config = MotionConfig {
            algorithm: MotionAlgorithm::PixelDiff,
            threshold,
            min_change_area: 50,
            downscale_factor: 1,
            max_width: test_frame_size.0,
            max_height: test_frame_size.1,
        };

        let mut service = MotionDetectionService::new(config).unwrap();

        let (frame1, frame2) = create_test_frame_pair(test_frame_size.0, test_frame_size.1);
        let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
        let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

        // Initialize
        let _ = service.detect_motion_from_bytes(&frame1_bytes).unwrap();

        group.bench_with_input(
            BenchmarkId::new("threshold", (threshold * 100.0) as u32),
            &frame2_bytes,
            |b, frame_bytes| {
                b.iter(|| {
                    service.detect_motion_from_bytes(frame_bytes).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark motion detection with realistic video scenarios
fn bench_realistic_scenarios(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic_scenarios");

    let config = MotionConfig {
        algorithm: MotionAlgorithm::PixelDiff,
        threshold: 0.1,
        min_change_area: 200,
        downscale_factor: 2,
        max_width: 320,
        max_height: 240,
    };

    let scenarios = vec![
        ("no_motion", create_identical_frames(320, 240)),
        ("small_motion", create_small_motion_frames(320, 240)),
        ("large_motion", create_large_motion_frames(320, 240)),
        ("multiple_objects", create_multi_object_frames(320, 240)),
    ];

    for (scenario_name, (frame1, frame2)) in scenarios {
        let mut service = MotionDetectionService::new(config.clone()).unwrap();

        let frame1_bytes = image_to_jpeg_bytes(&frame1).unwrap();
        let frame2_bytes = image_to_jpeg_bytes(&frame2).unwrap();

        // Initialize
        let _ = service.detect_motion_from_bytes(&frame1_bytes).unwrap();

        group.bench_with_input(
            BenchmarkId::new("scenario", scenario_name),
            &frame2_bytes,
            |b, frame_bytes| {
                b.iter(|| {
                    service.detect_motion_from_bytes(frame_bytes).unwrap();
                });
            },
        );
    }

    group.finish();
}

// Helper functions for realistic test scenarios
fn create_identical_frames(
    width: u32,
    height: u32,
) -> (gl_vision::image::GrayImage, gl_vision::image::GrayImage) {
    let frame1 = create_test_frame_with_motion(width, height, 0, 0, 0, 0, 64);
    let frame2 = frame1.clone();
    (frame1, frame2)
}

fn create_small_motion_frames(
    width: u32,
    height: u32,
) -> (gl_vision::image::GrayImage, gl_vision::image::GrayImage) {
    let frame1 = create_test_frame_with_motion(width, height, 0, 0, 0, 0, 64);
    let frame2 = create_test_frame_with_motion(width, height, width / 4, height / 4, 20, 20, 128);
    (frame1, frame2)
}

fn create_large_motion_frames(
    width: u32,
    height: u32,
) -> (gl_vision::image::GrayImage, gl_vision::image::GrayImage) {
    let frame1 = create_test_frame_with_motion(width, height, 0, 0, 0, 0, 64);
    let frame2 = create_test_frame_with_motion(width, height, 50, 50, width / 3, height / 3, 200);
    (frame1, frame2)
}

fn create_multi_object_frames(
    width: u32,
    height: u32,
) -> (gl_vision::image::GrayImage, gl_vision::image::GrayImage) {
    use gl_vision::image::{ImageBuffer, Luma};

    let mut frame1 = ImageBuffer::from_pixel(width, height, Luma([64u8]));
    let mut frame2 = ImageBuffer::from_pixel(width, height, Luma([64u8]));

    // Add multiple objects in different positions between frames
    let objects = vec![
        (20, 20, 30, 30, 128),
        (100, 50, 40, 25, 180),
        (200, 150, 50, 40, 220),
    ];

    for (x, y, w, h, intensity) in objects {
        // Object in frame1
        for dy in 0..h.min(height.saturating_sub(y)) {
            for dx in 0..w.min(width.saturating_sub(x)) {
                if x + dx < width && y + dy < height {
                    frame1.put_pixel(x + dx, y + dy, Luma([intensity]));
                }
            }
        }

        // Same object moved in frame2
        let new_x = (x + 10).min(width.saturating_sub(w));
        let new_y = (y + 5).min(height.saturating_sub(h));

        for dy in 0..h.min(height.saturating_sub(new_y)) {
            for dx in 0..w.min(width.saturating_sub(new_x)) {
                if new_x + dx < width && new_y + dy < height {
                    frame2.put_pixel(new_x + dx, new_y + dy, Luma([intensity]));
                }
            }
        }
    }

    (frame1, frame2)
}

// Configure benchmark groups
#[cfg(feature = "heavy_opencv")]
criterion_group!(
    benches,
    bench_pixel_diff_algorithm,
    bench_mog2_algorithm,
    bench_algorithm_comparison,
    bench_downscaling_performance,
    bench_threshold_performance,
    bench_realistic_scenarios
);

#[cfg(not(feature = "heavy_opencv"))]
criterion_group!(
    benches,
    bench_pixel_diff_algorithm,
    bench_algorithm_comparison,
    bench_downscaling_performance,
    bench_threshold_performance,
    bench_realistic_scenarios
);

criterion_main!(benches);
