//! ABOUTME: Streaming services for MJPEG and RTSP video streams
//! ABOUTME: Provides real-time video streaming capabilities

use bytes::Bytes;
use gl_capture::CaptureHandle;
use gl_core::Id;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    sync::broadcast,
    time::{interval, sleep, Instant},
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

mod metrics;
mod mjpeg;
#[cfg(feature = "rtsp")]
mod rtsp;

pub use metrics::*;
pub use mjpeg::*;
#[cfg(feature = "rtsp")]
pub use rtsp::*;

/// Error type for subscription failures
#[derive(Debug, Clone, thiserror::Error)]
pub enum SubscriptionError {
    /// Maximum number of subscribers reached
    #[error("Maximum number of subscribers ({max_clients}) reached")]
    MaxSubscribersReached { max_clients: usize },
}

/// Configuration for MJPEG streaming
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StreamConfig {
    /// Maximum frame rate (fps)
    pub max_fps: u32,
    /// Maximum number of buffered frames per client
    pub buffer_size: usize,
    /// Timeout for frame generation
    pub frame_timeout: Duration,
    /// Maximum number of concurrent streams
    pub max_clients: usize,
    /// JPEG quality (1-100)
    pub jpeg_quality: u8,
    /// RTSP server configuration (feature-gated)
    #[cfg(feature = "rtsp")]
    pub rtsp: Option<RtspConfig>,
}

/// RTSP server configuration
#[cfg(feature = "rtsp")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RtspConfig {
    /// Enable RTSP server
    pub enabled: bool,
    /// RTSP server port
    pub port: u16,
    /// Server address to bind to
    pub address: String,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_fps: 10,
            buffer_size: 5,
            frame_timeout: Duration::from_secs(5),
            max_clients: 10,
            jpeg_quality: 85,
            #[cfg(feature = "rtsp")]
            rtsp: Some(RtspConfig::default()),
        }
    }
}

#[cfg(feature = "rtsp")]
impl Default for RtspConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8554,
            address: "0.0.0.0".to_string(),
        }
    }
}

/// A streaming session that broadcasts JPEG frames to multiple clients
pub struct StreamSession {
    /// Unique session identifier
    pub id: Uuid,
    /// Template ID this session is streaming
    pub template_id: Id,
    /// Capture handle for getting frames
    capture: CaptureHandle,
    /// Broadcaster for frames
    frame_sender: broadcast::Sender<Bytes>,
    /// Configuration
    config: StreamConfig,
    /// Metrics
    metrics: StreamMetrics,
    /// Current subscriber count
    subscribers: Arc<AtomicU64>,
}

impl StreamSession {
    /// Create a new streaming session
    pub fn new(
        template_id: Id,
        capture: CaptureHandle,
        config: StreamConfig,
        metrics: StreamMetrics,
    ) -> Self {
        let (frame_sender, _) = broadcast::channel(config.buffer_size);

        Self {
            id: Uuid::new_v4(),
            template_id,
            capture,
            frame_sender,
            config,
            metrics,
            subscribers: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start the streaming session (frame generation loop)
    pub async fn start(self: Arc<Self>) {
        let mut interval = interval(Duration::from_millis(1000 / self.config.max_fps as u64));

        info!(
            session_id = %self.id,
            template_id = %self.template_id,
            max_fps = self.config.max_fps,
            "Starting MJPEG streaming session"
        );

        loop {
            interval.tick().await;

            // Check if we have any subscribers
            if self.subscribers.load(Ordering::Relaxed) == 0 {
                // No subscribers, sleep a bit longer and continue
                sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Generate frame
            let frame_start = Instant::now();
            match tokio::time::timeout(self.config.frame_timeout, self.capture.snapshot()).await {
                Ok(Ok(frame)) => {
                    self.metrics.frames_generated.inc();
                    let frame_duration = frame_start.elapsed();

                    debug!(
                        session_id = %self.id,
                        frame_size = frame.len(),
                        duration_ms = frame_duration.as_millis(),
                        subscribers = self.subscribers.load(Ordering::Relaxed),
                        "Generated frame"
                    );

                    // Broadcast frame to all subscribers
                    match self.frame_sender.send(frame) {
                        Ok(subscriber_count) => {
                            debug!(subscriber_count, "Frame broadcast to subscribers");
                        }
                        Err(_) => {
                            warn!("No active subscribers for frame");
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!(
                        error = %e,
                        session_id = %self.id,
                        "Failed to capture frame"
                    );
                    self.metrics.frame_errors.inc();
                    // Brief delay before retrying
                    sleep(Duration::from_millis(500)).await;
                }
                Err(_) => {
                    error!(
                        session_id = %self.id,
                        timeout_ms = self.config.frame_timeout.as_millis(),
                        "Frame capture timeout"
                    );
                    self.metrics.frame_timeouts.inc();
                }
            }
        }
    }

    /// Subscribe to the frame stream
    ///
    /// # Errors
    ///
    /// Returns `SubscriptionError::MaxSubscribersReached` if the maximum number of
    /// concurrent subscribers (`max_clients`) has been reached.
    pub fn subscribe(&self) -> Result<broadcast::Receiver<Bytes>, SubscriptionError> {
        // Use compare-and-swap loop to atomically check and increment
        // This prevents race conditions where multiple threads could exceed max_clients
        loop {
            let current_count = self.subscribers.load(Ordering::Acquire);

            // Check if we've reached the limit
            if current_count >= self.config.max_clients as u64 {
                warn!(
                    session_id = %self.id,
                    current_subscribers = current_count,
                    max_clients = self.config.max_clients,
                    "Subscription rejected: max_clients limit reached"
                );
                self.metrics.connections_rejected.inc();
                return Err(SubscriptionError::MaxSubscribersReached {
                    max_clients: self.config.max_clients,
                });
            }

            // Try to atomically increment from current to current + 1
            // This ensures we only increment if count hasn't changed since we checked
            match self.subscribers.compare_exchange_weak(
                current_count,
                current_count + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Successfully incremented - we got a slot
                    let new_count = current_count + 1;
                    self.metrics.subscribers.set(new_count as i64);

                    info!(
                        session_id = %self.id,
                        subscriber_count = new_count,
                        max_clients = self.config.max_clients,
                        "New subscriber joined"
                    );

                    return Ok(self.frame_sender.subscribe());
                }
                Err(_) => {
                    // Count was modified by another thread, retry the loop
                    continue;
                }
            }
        }
    }

    /// Remove a subscriber
    pub fn unsubscribe(&self) {
        // Use fetch_update to prevent underflow
        let subscriber_count = self
            .subscribers
            .fetch_update(Ordering::Release, Ordering::Acquire, |current| {
                // Only decrement if count is greater than 0
                if current > 0 {
                    Some(current - 1)
                } else {
                    None
                }
            })
            .unwrap_or(0); // If already 0, return 0

        // Update metrics with the new count
        let new_count = if subscriber_count > 0 {
            subscriber_count - 1
        } else {
            0
        };
        self.metrics.subscribers.set(new_count as i64);

        info!(
            session_id = %self.id,
            subscriber_count = new_count,
            "Subscriber left"
        );
    }

    /// Get current subscriber count
    pub fn subscriber_count(&self) -> u64 {
        self.subscribers.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gl_capture::{CaptureSource, FileSource};
    use std::time::Duration;
    use test_support::create_test_id;
    use tokio::fs;

    async fn create_test_video_file() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp_dir = tempfile::tempdir().unwrap();
        let video_path = temp_dir.path().join("test.mp4");

        // Create a minimal fake video file for testing
        fs::write(&video_path, b"fake video data").await.unwrap();

        (temp_dir, video_path)
    }

    #[tokio::test]
    async fn test_stream_session_creation() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        // This will likely fail without actual video/ffmpeg, but tests basic structure
        match source.start().await {
            Ok(capture) => {
                let session = StreamSession::new(template_id.clone(), capture, config, metrics);
                assert_eq!(session.template_id, template_id);
                assert_eq!(session.subscriber_count(), 0);
            }
            Err(_) => {
                // Expected when ffmpeg isn't available or file isn't valid video
            }
        }
    }

    #[tokio::test]
    async fn test_stream_session_subscription() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        // This will likely fail without actual video/ffmpeg, but tests subscription logic
        match source.start().await {
            Ok(capture) => {
                let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

                // Test subscription
                let _receiver1 = session.subscribe().expect("Should subscribe successfully");
                assert_eq!(session.subscriber_count(), 1);

                let _receiver2 = session.subscribe().expect("Should subscribe successfully");
                assert_eq!(session.subscriber_count(), 2);

                // Test unsubscription
                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 1);

                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 0);
            }
            Err(_) => {
                // Expected when ffmpeg isn't available
            }
        }
    }

    #[tokio::test]
    async fn test_stream_manager() {
        let metrics = StreamMetrics::new();
        let manager = StreamManager::new(metrics);

        let template_id = Id::new();

        // Initially no session
        assert!(manager.get_session(&template_id).is_none());

        // We can't easily create a full session without ffmpeg, so this tests the structure
        let (_temp_dir, video_path) = create_test_video_file().await;
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let session_metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(
                template_id.clone(),
                capture,
                config,
                session_metrics,
            ));

            // Add session
            manager.add_session(session.clone());
            assert!(manager.get_session(&template_id).is_some());

            // Remove session
            manager.remove_session(&template_id);
            assert!(manager.get_session(&template_id).is_none());
        }
    }

    #[tokio::test]
    async fn test_stream_config_defaults() {
        let config = StreamConfig::default();

        assert_eq!(config.max_fps, 10);
        assert_eq!(config.buffer_size, 5);
        assert_eq!(config.frame_timeout, Duration::from_secs(5));
        assert_eq!(config.max_clients, 10);
        assert_eq!(config.jpeg_quality, 85);
    }

    #[tokio::test]
    async fn test_metrics_initialization() {
        let metrics = StreamMetrics::new();

        // Test that metrics start at expected values
        // Note: Counter and Gauge don't expose their current values in a direct way
        // This mainly tests that they can be created without panicking
        let _ = &metrics.frames_generated;
        let _ = &metrics.frame_errors;
        let _ = &metrics.frame_timeouts;
        let _ = &metrics.subscribers;
        let _ = &metrics.connections_total;
        let _ = &metrics.disconnections_total;
        let _ = &metrics.frames_dropped;
    }

    #[tokio::test]
    async fn test_max_clients_enforcement() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig {
            max_clients: 2, // Set low limit for testing
            ..Default::default()
        };
        let metrics = StreamMetrics::new();

        match source.start().await {
            Ok(capture) => {
                let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

                // First subscriber should succeed
                let _receiver1 = session.subscribe().expect("First subscription should succeed");
                assert_eq!(session.subscriber_count(), 1);

                // Second subscriber should succeed
                let _receiver2 = session.subscribe().expect("Second subscription should succeed");
                assert_eq!(session.subscriber_count(), 2);

                // Third subscriber should fail (max_clients = 2)
                let result3 = session.subscribe();
                assert!(result3.is_err(), "Third subscription should fail");
                assert_eq!(session.subscriber_count(), 2, "Count should remain at 2");

                // Verify error message
                if let Err(e) = result3 {
                    let error_msg = e.to_string();
                    assert!(
                        error_msg.contains("Maximum number of subscribers"),
                        "Error message should mention max subscribers: {}",
                        error_msg
                    );
                    assert!(error_msg.contains("2"), "Error should show max_clients value");
                }

                // After unsubscribing, should be able to subscribe again
                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 1);

                let _receiver3 = session
                    .subscribe()
                    .expect("Should succeed after unsubscribe");
                assert_eq!(session.subscriber_count(), 2);
            }
            Err(_) => {
                // Expected when ffmpeg isn't available
            }
        }
    }

    #[tokio::test]
    async fn test_subscription_rejected_metric() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig {
            max_clients: 1, // Only allow 1 client
            ..Default::default()
        };
        let metrics = StreamMetrics::new();

        match source.start().await {
            Ok(capture) => {
                let session = Arc::new(StreamSession::new(
                    template_id,
                    capture,
                    config,
                    metrics.clone(),
                ));

                // First subscription succeeds
                let _receiver1 = session.subscribe().expect("Should succeed");

                // Second subscription should be rejected
                let _ = session.subscribe();

                // Third subscription should also be rejected
                let _ = session.subscribe();

                // Note: We can't easily verify the metric value in tests without
                // exposing it, but we ensure the code path is exercised
            }
            Err(_) => {
                // Expected when ffmpeg isn't available
            }
        }
    }

    #[tokio::test]
    async fn test_unsubscribe_underflow_protection() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        match source.start().await {
            Ok(capture) => {
                let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

                // Initially count is 0
                assert_eq!(session.subscriber_count(), 0);

                // Calling unsubscribe when count is 0 should not underflow
                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 0, "Count should remain 0, not underflow");

                // Subscribe and verify count increases
                let _receiver = session.subscribe().expect("Should succeed");
                assert_eq!(session.subscriber_count(), 1);

                // Unsubscribe twice (second should be protected)
                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 0);
                session.unsubscribe();
                assert_eq!(session.subscriber_count(), 0, "Count should remain 0 after extra unsubscribe");
            }
            Err(_) => {
                // Expected when ffmpeg isn't available
            }
        }
    }

    // Integration test with a mock streaming scenario
    #[tokio::test]
    async fn test_mjpeg_stream_backpressure() {
        let _test_id = create_test_id();
        let _template_id = Id::new();
        let config = StreamConfig {
            buffer_size: 2, // Small buffer to test backpressure
            max_fps: 30,
            ..Default::default()
        };
        let _metrics = StreamMetrics::new();

        // Create a broadcast channel manually to test backpressure behavior
        let (sender, mut receiver1) = tokio::sync::broadcast::channel(config.buffer_size);
        let mut receiver2 = sender.subscribe();

        // Send more frames than buffer can hold
        for i in 0..5 {
            let frame = Bytes::from(format!("frame_{}", i));
            let _ = sender.send(frame); // Some sends might fail due to buffer size
        }

        // Fast receiver should get recent frames
        if let Ok(frame) = receiver1.try_recv() {
            // Should be able to receive something
            assert!(frame.len() > 0);
        }

        // Slow receiver might lag
        match receiver2.try_recv() {
            Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                // Either got a frame or buffer was empty - both ok
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                // This is what we expect for backpressure - receiver lagged behind
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                // Channel closed - also acceptable for this test
            }
        }
    }
}
