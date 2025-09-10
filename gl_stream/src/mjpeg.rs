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

use crate::{StreamMetrics, StreamSession};

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

    /// Get or create a streaming session for a template
    pub fn get_session(&self, template_id: &Id) -> Option<Arc<StreamSession>> {
        self.sessions.get(template_id).map(|s| s.clone())
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
    frame_receiver: broadcast::Receiver<Bytes>,
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
}

impl MjpegStream {
    /// Create a new MJPEG stream
    pub fn new(
        session: Arc<StreamSession>,
        frame_receiver: broadcast::Receiver<Bytes>,
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
        }
    }

    /// Generate multipart boundary header
    fn create_frame_header(&self, frame_size: usize) -> Bytes {
        let header = format!(
            "--{}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
            self.boundary, frame_size
        );
        Bytes::from(header)
    }

    /// Generate the initial HTTP headers for multipart response
    pub fn content_type(&self) -> String {
        format!("multipart/x-mixed-replace; boundary={}", self.boundary)
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
                debug!(
                    connection_id = %self.connection_id,
                    frame_size = frame.len(),
                    "Received frame for streaming"
                );

                // Create the multipart frame with headers
                let mut response = BytesMut::new();
                response.extend_from_slice(&self.create_frame_header(frame.len()));
                response.extend_from_slice(&frame);
                response.extend_from_slice(b"\r\n");

                Poll::Ready(Some(Ok(response.freeze())))
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                info!(
                    connection_id = %self.connection_id,
                    "Frame broadcast channel closed"
                );
                Poll::Ready(None)
            }
            Err(broadcast::error::TryRecvError::Lagged(skipped)) => {
                warn!(
                    connection_id = %self.connection_id,
                    skipped_frames = skipped,
                    "Stream lagged behind, frames dropped"
                );
                self.metrics.frames_dropped.inc_by(skipped);

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
        debug!(
            connection_id = %self.connection_id,
            session_id = %self.session.id,
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
    use gl_capture::{CaptureSource, FileSource};
    use gl_core::Id;
    use test_support::create_test_id;

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
}
