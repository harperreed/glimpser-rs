// ABOUTME: Benchmark for MJPEG frame processing
// ABOUTME: Measures streaming performance across frames
use bytes::Bytes;
use criterion::{criterion_group, criterion_main, Criterion};
use futures_util::StreamExt;
use gl_capture::{file_source::FileSource, CaptureSource};
use gl_core::Id;
use gl_stream::{MjpegStream, StreamConfig, StreamMetrics, StreamSession};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;

fn mjpeg_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    c.bench_function("mjpeg_stream_frame", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tmp_file = std::env::temp_dir().join("bench.mp4");
                let _ = std::fs::File::create(&tmp_file).unwrap();
                let source = FileSource::new(&tmp_file);
                let capture = source.start().await.unwrap();
                let session = Arc::new(StreamSession::new(
                    Id::new(),
                    capture,
                    StreamConfig::default(),
                    StreamMetrics::default(),
                ));
                let (tx, rx) = broadcast::channel(16);
                let stream = MjpegStream::new(session, rx, StreamMetrics::default());
                tokio::pin!(stream);
                stream.next().await; // boundary
                tx.send(Bytes::from_static(b"frame data")).unwrap();
                stream.next().await.unwrap().unwrap();
            })
        })
    });
}

criterion_group!(benches, mjpeg_benchmark);
criterion_main!(benches);
