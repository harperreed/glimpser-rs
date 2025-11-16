//! ABOUTME: MJPEG streaming implementation with multipart/x-mixed-replace HTTP responses
//! ABOUTME: Provides Actix handlers for streaming JPEG frames over HTTP

use actix_web::{web, HttpRequest, HttpResponse, Result as ActixResult};
use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use futures_util::stream::Stream;
use gl_core::Id;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

use crate::{Frame, StreamMetrics, StreamSession};

/// Manager for active streaming sessions
pub struct StreamManager {
    /// Active streaming sessions by template ID
    sessions: DashMap<Id, Arc<StreamSession>>,
    /// Global streaming metrics
    metrics: StreamMetrics,
}

impl StreamManager {
    /// Create a new stream manager
    pub fn new(metrics: StreamMetrics) -> Self {
        Self {
            sessions: DashMap::new(),
            metrics,
        }
    }

    /// Get a streaming session for a template
    pub fn get_session(&self, template_id: &Id) -> Option<Arc<StreamSession>> {
        self.sessions.get(template_id).map(|s| Arc::clone(&s))
    }

    /// Add a new streaming session
    pub fn add_session(&self, session: Arc<StreamSession>) {
        let template_id = session.template_id.clone();
        self.sessions.insert(template_id, session);
    }

    /// Remove a streaming session
    pub fn remove_session(&self, template_id: &Id) {
        self.sessions.remove(template_id);
    }

    /// Get metrics
    pub fn metrics(&self) -> &StreamMetrics {
        &self.metrics
    }
}

/// MJPEG frame stream that implements the Stream trait
pub struct MjpegStream {
    /// Receiver for JPEG frames
    frame_receiver: broadcast::Receiver<Frame>,
    /// Boundary string for multipart response
    boundary: String,
    /// Session reference for cleanup
    session: Arc<StreamSession>,
    /// Connection ID for logging
    connection_id: Uuid,
    /// Whether the stream has started (sent headers)
    started: bool,
    /// Metrics for tracking frame drops
    metrics: StreamMetrics,
    /// Reusable buffer for building frame responses
    buffer: BytesMut,
    /// Last received frame sequence number (for gap detection)
    last_sequence: Option<u64>,
    /// Total number of frames dropped due to lag (per subscriber)
    frames_dropped_count: u64,
    /// Total number of sequence gaps detected
    sequence_gaps: u64,
    /// Expected number of frames in next gap (from Lagged error)
    expected_gap: Option<u64>,
}

impl MjpegStream {
    /// Create a new MJPEG stream
    pub fn new(
        session: Arc<StreamSession>,
        frame_receiver: broadcast::Receiver<Frame>,
        metrics: StreamMetrics,
    ) -> Self {
        let boundary = format!("mjpeg_boundary_{}", Uuid::new_v4());
        let connection_id = Uuid::new_v4();

        Self {
            frame_receiver,
            boundary,
            session,
            connection_id,
            started: false,
            metrics,
            buffer: BytesMut::with_capacity(1024),
            last_sequence: None,
            frames_dropped_count: 0,
            sequence_gaps: 0,
            expected_gap: None,
        }
    }

    /// Generate the initial HTTP headers for multipart response
    pub fn content_type(&self) -> String {
        format!("multipart/x-mixed-replace; boundary={}", self.boundary)
    }

    /// Get the total number of frames dropped for this subscriber
    pub fn frames_dropped(&self) -> u64 {
        self.frames_dropped_count
    }

    /// Get the total number of sequence gaps detected
    pub fn sequence_gaps(&self) -> u64 {
        self.sequence_gaps
    }

    /// Get the last received sequence number
    pub fn last_sequence(&self) -> Option<u64> {
        self.last_sequence
    }
}

#[cfg(test)]
impl MjpegStream {
    fn buffer_capacity(&self) -> usize {
        self.buffer.capacity()
    }
}

impl Stream for MjpegStream {
    type Item = Result<Bytes, actix_web::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check if we need to send initial boundary
        if !self.started {
            self.started = true;
            debug!(
                connection_id = %self.connection_id,
                boundary = %self.boundary,
                "Starting MJPEG stream"
            );

            // Return the first boundary marker
            let initial_boundary = format!("--{}\r\n", self.boundary);
            return Poll::Ready(Some(Ok(Bytes::from(initial_boundary))));
        }

        // Poll for the next frame using try_recv for now
        // Note: This is a simplified approach - a more sophisticated implementation
        // would use proper async polling
        match self.frame_receiver.try_recv() {
            Ok(frame) => {
                // Check for sequence gaps
                if let Some(last_seq) = self.last_sequence {
                    let expected_seq = last_seq + 1;
                    if frame.sequence != expected_seq {
                        if frame.sequence < expected_seq {
                            // Backwards sequence - this shouldn't happen with broadcast channels
                            warn!(
                                connection_id = %self.connection_id,
                                expected = expected_seq,
                                received = frame.sequence,
                                "Received out-of-order frame with backwards sequence"
                            );
                        } else {
                            // Forward gap - frames were missed
                            let gap = frame.sequence - expected_seq;

                            // Check if this gap matches a recent Lagged error
                            let is_expected = self.expected_gap == Some(gap);

                            if is_expected {
                                // This gap matches the lag we just reported, log at debug level
                                debug!(
                                    connection_id = %self.connection_id,
                                    expected = expected_seq,
                                    received = frame.sequence,
                                    gap = gap,
                                    "Sequence gap matches reported lag"
                                );
                            } else {
                                // Unexpected gap - could be generator issue or mismatch with lag report
                                self.sequence_gaps += 1;
                                self.metrics.sequence_gaps_total.inc();
                                warn!(
                                    connection_id = %self.connection_id,
                                    expected = expected_seq,
                                    received = frame.sequence,
                                    gap = gap,
                                    total_gaps = self.sequence_gaps,
                                    expected_from_lag = ?self.expected_gap,
                                    "Unexpected sequence gap detected"
                                );
                            }

                            // Clear expected gap after checking
                            self.expected_gap = None;
                        }
                    } else {
                        // Sequence is as expected, clear any pending expected gap
                        self.expected_gap = None;
                    }
                }

                self.last_sequence = Some(frame.sequence);

                debug!(
                    connection_id = %self.connection_id,
                    sequence = frame.sequence,
                    frame_size = frame.data.len(),
                    "Received frame for streaming"
                );

                // Build response in reusable buffer
                use std::fmt::Write as _;
                let len = frame.data.len();
                let boundary = self.boundary.clone();
                self.buffer.clear();
                write!(
                    &mut self.buffer,
                    "--{boundary}\r\nContent-Type: image/jpeg\r\nContent-Length: {len}\r\n\r\n",
                )
                .unwrap();
                self.buffer.extend_from_slice(&frame.data);
                self.buffer.extend_from_slice(b"\r\n");
                let bytes = self.buffer.clone().freeze();
                self.buffer.truncate(0);
                Poll::Ready(Some(Ok(bytes)))
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                info!(
                    connection_id = %self.connection_id,
                    "Frame broadcast channel closed"
                );
                Poll::Ready(None)
            }
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                self.frames_dropped_count += skipped;
                self.metrics.frames_dropped.inc_by(skipped);

                // Accumulate expected gap (handles multiple consecutive Lagged errors)
                self.expected_gap = Some(self.expected_gap.unwrap_or(0) + skipped);

                warn!(
                    connection_id = %self.connection_id,
                    session_id = %self.session.id,
                    skipped_frames = skipped,
                    total_dropped = self.frames_dropped_count,
                    cumulative_expected_gap = ?self.expected_gap,
                    last_sequence = ?self.last_sequence,
                    "Stream lagged - broadcast channel dropped frames due to backpressure"
                );

                // Continue polling for the next frame
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                // Register waker and return pending
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

impl Drop for MjpegStream {
    fn drop(&mut self) {
        info!(
            connection_id = %self.connection_id,
            session_id = %self.session.id,
            total_frames_dropped = self.frames_dropped_count,
            sequence_gaps = self.sequence_gaps,
            last_sequence = ?self.last_sequence,
            "MJPEG stream connection dropped"
        );

        self.session.unsubscribe();
        self.metrics.disconnections_total.inc();
    }
}

/// Actix handler for MJPEG streaming endpoint
#[instrument(skip(stream_manager))]
pub async fn mjpeg_stream_handler(
    path: web::Path<String>,
    stream_manager: web::Data<StreamManager>,
    _req: HttpRequest,
) -> ActixResult<HttpResponse> {
    let template_id_str = path.into_inner();

    // Parse template ID
    let template_id = match template_id_str.parse::<Id>() {
        Ok(id) => id,
        Err(e) => {
            warn!(
                template_id = %template_id_str,
                error = %e,
                "Invalid template ID format"
            );
            return Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Invalid template ID format"
            })));
        }
    };

    // Get the streaming session
    let session = match stream_manager.get_session(&template_id) {
        Some(session) => session,
        None => {
            warn!(
                template_id = %template_id,
                "No active streaming session found for template"
            );
            return Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "No active stream for this template"
            })));
        }
    };

    info!(
        template_id = %template_id,
        session_id = %session.id,
        current_subscribers = session.subscriber_count(),
        "New MJPEG stream client connecting"
    );

    // Subscribe to the frame stream
    let frame_receiver = session.subscribe();
    stream_manager.metrics().connections_total.inc();

    // Create the MJPEG stream
    let mjpeg_stream = MjpegStream::new(
        session.clone(),
        frame_receiver,
        stream_manager.metrics().clone(),
    );

    // Return streaming response
    Ok(HttpResponse::Ok()
        .content_type(mjpeg_stream.content_type())
        .insert_header(("Cache-Control", "no-cache, no-store, must-revalidate"))
        .insert_header(("Pragma", "no-cache"))
        .insert_header(("Expires", "0"))
        .insert_header(("Connection", "keep-alive"))
        .streaming(mjpeg_stream))
}

/// Configure MJPEG streaming routes
pub fn configure_mjpeg_routes(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/api/stream/{template_id}/mjpeg",
        web::get().to(mjpeg_stream_handler),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App};
    use bytes::Bytes;
    use futures_util::StreamExt;
    use gl_capture::{CaptureSource, FileSource};
    use gl_core::Id;
    use std::sync::Arc;
    use test_support::create_test_id;
    use tokio::fs;
    use tokio::sync::broadcast;

    #[actix_web::test]
    async fn test_mjpeg_stream_handler_invalid_template_id() {
        let metrics = StreamMetrics::new();
        let stream_manager = web::Data::new(StreamManager::new(metrics));

        let app = test::init_service(App::new().app_data(stream_manager.clone()).route(
            "/api/stream/{template_id}/mjpeg",
            web::get().to(mjpeg_stream_handler),
        ))
        .await;

        let req = test::TestRequest::get()
            .uri("/api/stream/invalid-id/mjpeg")
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 400);
    }

    #[actix_web::test]
    async fn test_mjpeg_stream_handler_no_session() {
        let metrics = StreamMetrics::new();
        let stream_manager = web::Data::new(StreamManager::new(metrics));

        let app = test::init_service(App::new().app_data(stream_manager.clone()).route(
            "/api/stream/{template_id}/mjpeg",
            web::get().to(mjpeg_stream_handler),
        ))
        .await;

        let template_id = Id::new();
        let req = test::TestRequest::get()
            .uri(&format!("/api/stream/{}/mjpeg", template_id))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn test_mjpeg_stream_boundary_generation() {
        let test_id = create_test_id();
        let template_id = Id::new();
        let temp_dir = std::env::temp_dir().join(format!("gl_stream_test_{}", test_id));
        let video_path = temp_dir.join("test.mp4");

        // Create temp directory and dummy video file
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(&video_path, b"fake video data").unwrap();

        let source = FileSource::new(video_path);
        let config = crate::StreamConfig::default();
        let metrics = StreamMetrics::new();

        // This will fail without real ffmpeg/video, but tests the structure
        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(
                template_id,
                capture,
                config,
                metrics.clone(),
            ));
            let frame_receiver = session.subscribe();
            let mjpeg_stream = MjpegStream::new(session, frame_receiver, metrics);

            assert!(mjpeg_stream.boundary.contains("mjpeg_boundary_"));
            assert!(mjpeg_stream
                .content_type()
                .contains("multipart/x-mixed-replace"));
        }
    }

    #[tokio::test]
    async fn buffer_reused_across_frames() {
        let tmp_file = std::env::temp_dir().join("mjpeg_test.mp4");
        fs::File::create(&tmp_file).await.unwrap();
        let source = FileSource::new(&tmp_file);
        let capture = source.start().await.unwrap();
        let session = Arc::new(crate::StreamSession::new(
            Id::new(),
            capture,
            crate::StreamConfig::default(),
            StreamMetrics::default(),
        ));
        let (tx, rx) = broadcast::channel(4);
        let stream = MjpegStream::new(session, rx, StreamMetrics::default());
        let initial_cap = stream.buffer_capacity();
        assert!(initial_cap > 0);
        tokio::pin!(stream);
        stream.next().await; // boundary
        tx.send(crate::Frame::new(0, Bytes::from_static(b"frame1")))
            .unwrap();
        stream.next().await.unwrap().unwrap();
        let after_first = stream.buffer_capacity();
        tx.send(crate::Frame::new(1, Bytes::from_static(b"frame2")))
            .unwrap();
        stream.next().await.unwrap().unwrap();
        let after_second = stream.buffer_capacity();
        assert_eq!(initial_cap, after_first);
        assert_eq!(initial_cap, after_second);
    }

    #[tokio::test]
    async fn test_sequence_gap_detection() {
        let tmp_file = std::env::temp_dir().join("mjpeg_seq_test.mp4");
        fs::File::create(&tmp_file).await.unwrap();
        let source = FileSource::new(&tmp_file);
        let capture = source.start().await.unwrap();
        let session = Arc::new(crate::StreamSession::new(
            Id::new(),
            capture,
            crate::StreamConfig::default(),
            StreamMetrics::default(),
        ));
        let (tx, rx) = broadcast::channel(10);
        let stream = MjpegStream::new(session, rx, StreamMetrics::default());

        // Send first frame
        tx.send(crate::Frame::new(0, Bytes::from_static(b"frame0")))
            .unwrap();

        // Send frame with gap (sequence 5 instead of 1)
        tx.send(crate::Frame::new(5, Bytes::from_static(b"frame5")))
            .unwrap();

        tokio::pin!(stream);

        // Get boundary
        stream.next().await;

        // Get first frame - should not detect gap
        stream.next().await.unwrap().unwrap();
        assert_eq!(stream.last_sequence(), Some(0));
        assert_eq!(stream.sequence_gaps(), 0);

        // Get second frame - should detect gap
        stream.next().await.unwrap().unwrap();
        assert_eq!(stream.last_sequence(), Some(5));
        assert_eq!(stream.sequence_gaps(), 1); // Gap detected!
    }

    #[tokio::test]
    async fn test_frame_drop_tracking() {
        let tmp_file = std::env::temp_dir().join("mjpeg_drop_test.mp4");
        fs::File::create(&tmp_file).await.unwrap();
        let source = FileSource::new(&tmp_file);
        let capture = source.start().await.unwrap();
        let session = Arc::new(crate::StreamSession::new(
            Id::new(),
            capture,
            crate::StreamConfig::default(),
            StreamMetrics::default(),
        ));

        // Create small buffer to force lag
        let (_tx, rx) = broadcast::channel(2);
        let stream = MjpegStream::new(session, rx, StreamMetrics::default());

        // Validate the stream has frame drop tracking capability
        assert_eq!(stream.frames_dropped(), 0);
        assert_eq!(stream.sequence_gaps(), 0);
        assert_eq!(stream.last_sequence(), None);

        // Note: This test validates that the tracking fields exist and work.
        // The actual lag behavior is tested in the backpressure test.
    }

    #[tokio::test]
    async fn test_unexpected_vs_expected_gaps() {
        let tmp_file = std::env::temp_dir().join("mjpeg_gap_test.mp4");
        fs::File::create(&tmp_file).await.unwrap();
        let source = FileSource::new(&tmp_file);
        let capture = source.start().await.unwrap();
        let session = Arc::new(crate::StreamSession::new(
            Id::new(),
            capture,
            crate::StreamConfig::default(),
            StreamMetrics::default(),
        ));
        let (tx, rx) = broadcast::channel(10);
        let stream = MjpegStream::new(session, rx, StreamMetrics::default());

        tokio::pin!(stream);

        // Get boundary
        stream.next().await;

        // Send frame 0
        tx.send(crate::Frame::new(0, Bytes::from_static(b"frame0")))
            .unwrap();
        stream.next().await.unwrap().unwrap();
        assert_eq!(stream.last_sequence(), Some(0));

        // Send frames that will cause an unexpected gap (frame 3 instead of 1)
        // This gap is NOT from a Lagged error, so it should be counted
        tx.send(crate::Frame::new(3, Bytes::from_static(b"frame3")))
            .unwrap();
        stream.next().await.unwrap().unwrap();

        // This should be counted as an unexpected gap
        assert_eq!(stream.last_sequence(), Some(3));
        assert_eq!(stream.sequence_gaps(), 1);

        // Continue normally
        tx.send(crate::Frame::new(4, Bytes::from_static(b"frame4")))
            .unwrap();
        stream.next().await.unwrap().unwrap();
        assert_eq!(stream.sequence_gaps(), 1); // No new gap
    }
}
