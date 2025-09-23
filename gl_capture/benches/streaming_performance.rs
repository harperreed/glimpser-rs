//! ABOUTME: Performance benchmarks for MJPEG streaming optimization
//! ABOUTME: Compares traditional process-spawning vs persistent process performance

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use gl_capture::{
    AccelerationDetector, CaptureSource, FfmpegConfig, FfmpegSource, HardwareAccel,
    StreamingFfmpegSource,
};
use std::{collections::HashMap, time::Duration};
use tokio::runtime::Runtime;

/// Benchmark configuration for testing
struct BenchConfig {
    name: String,
    input_url: String,
    frame_count: usize,
    ffmpeg_config: FfmpegConfig,
}

impl BenchConfig {
    fn test_pattern(name: &str, size: &str, rate: u32, frame_count: usize) -> Self {
        let mut input_options = HashMap::new();
        input_options.insert("f".to_string(), "lavfi".to_string());

        Self {
            name: name.to_string(),
            input_url: format!(
                "testsrc=duration={}:size={}:rate={}",
                frame_count * 2,
                size,
                rate
            ),
            frame_count,
            ffmpeg_config: FfmpegConfig {
                input_url: format!(
                    "testsrc=duration={}:size={}:rate={}",
                    frame_count * 2,
                    size,
                    rate
                ),
                input_options,
                hardware_accel: HardwareAccel::None, // Start with software
                timeout: Some(10),
                ..Default::default()
            },
        }
    }
}

/// Benchmark traditional FfmpegSource (process-spawning approach)
async fn bench_traditional_ffmpeg(
    config: &BenchConfig,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let source = FfmpegSource::new(config.ffmpeg_config.clone());
    let handle = source.start().await?;

    let start_time = std::time::Instant::now();

    for _i in 0..config.frame_count {
        let _frame = handle.snapshot().await?;
    }

    let duration = start_time.elapsed();
    handle.stop().await?;

    Ok(duration)
}

/// Benchmark optimized StreamingFfmpegSource (persistent process approach)
async fn bench_streaming_ffmpeg(
    config: &BenchConfig,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let source = StreamingFfmpegSource::from_ffmpeg_config(config.ffmpeg_config.clone()).await?;
    let handle = source.start().await?;

    let start_time = std::time::Instant::now();

    for _i in 0..config.frame_count {
        let _frame = handle.snapshot().await?;
    }

    let duration = start_time.elapsed();
    handle.stop().await?;

    Ok(duration)
}

/// Benchmark with hardware acceleration
async fn bench_streaming_ffmpeg_with_accel(
    config: &BenchConfig,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let source =
        StreamingFfmpegSource::with_auto_acceleration(config.ffmpeg_config.clone()).await?;
    let handle = source.start().await?;

    let start_time = std::time::Instant::now();

    for _i in 0..config.frame_count {
        let _frame = handle.snapshot().await?;
    }

    let duration = start_time.elapsed();
    handle.stop().await?;

    Ok(duration)
}

/// Performance comparison benchmark
fn bench_mjpeg_streaming_performance(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Test configurations
    let configs = vec![
        BenchConfig::test_pattern("small_frames", "320x240", 10, 10),
        BenchConfig::test_pattern("medium_frames", "640x480", 10, 10),
        BenchConfig::test_pattern("large_frames", "1280x720", 10, 5),
    ];

    for config in configs {
        let mut group = c.benchmark_group(&config.name);
        group.throughput(Throughput::Elements(config.frame_count as u64));
        group.measurement_time(Duration::from_secs(30));

        // Benchmark traditional approach
        group.bench_with_input(
            BenchmarkId::new("traditional_ffmpeg", &config.name),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    match bench_traditional_ffmpeg(config).await {
                        Ok(duration) => black_box(duration),
                        Err(_) => Duration::from_secs(999), // Mark failures with high duration
                    }
                });
            },
        );

        // Benchmark optimized streaming approach
        group.bench_with_input(
            BenchmarkId::new("streaming_ffmpeg", &config.name),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    match bench_streaming_ffmpeg(config).await {
                        Ok(duration) => black_box(duration),
                        Err(_) => Duration::from_secs(999), // Mark failures with high duration
                    }
                });
            },
        );

        // Benchmark with hardware acceleration
        group.bench_with_input(
            BenchmarkId::new("streaming_ffmpeg_hwaccel", &config.name),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    match bench_streaming_ffmpeg_with_accel(config).await {
                        Ok(duration) => black_box(duration),
                        Err(_) => Duration::from_secs(999), // Mark failures with high duration
                    }
                });
            },
        );

        group.finish();
    }
}

/// Hardware acceleration detection benchmark
fn bench_hardware_acceleration_detection(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("hardware_accel_detection", |b| {
        b.to_async(&rt).iter(|| async {
            let _accel = AccelerationDetector::auto_configure()
                .await
                .unwrap_or(HardwareAccel::None);
            black_box(_accel)
        });
    });
}

/// Frame extraction latency test
fn bench_frame_extraction_latency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let config = BenchConfig::test_pattern("latency_test", "640x480", 30, 1);

    let mut group = c.benchmark_group("frame_extraction_latency");
    group.measurement_time(Duration::from_secs(20));

    // Traditional single frame latency
    group.bench_function("traditional_single_frame", |b| {
        b.to_async(&rt).iter(|| async {
            let source = FfmpegSource::new(config.ffmpeg_config.clone());
            match source.start().await {
                Ok(handle) => {
                    let start = std::time::Instant::now();
                    let _frame = handle.snapshot().await;
                    let latency = start.elapsed();
                    let _ = handle.stop().await;
                    black_box(latency)
                }
                Err(_) => black_box(Duration::from_secs(999)),
            }
        });
    });

    // Streaming approach latency (after warmup)
    group.bench_function("streaming_single_frame", |b| {
        b.to_async(&rt).iter(|| async {
            match StreamingFfmpegSource::from_ffmpeg_config(config.ffmpeg_config.clone()).await {
                Ok(source) => match source.start().await {
                    Ok(handle) => {
                        let start = std::time::Instant::now();
                        let _frame = handle.snapshot().await;
                        let latency = start.elapsed();
                        let _ = handle.stop().await;
                        black_box(latency)
                    }
                    Err(_) => black_box(Duration::from_secs(999)),
                },
                Err(_) => black_box(Duration::from_secs(999)),
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_mjpeg_streaming_performance,
    bench_hardware_acceleration_detection,
    bench_frame_extraction_latency
);
criterion_main!(benches);
