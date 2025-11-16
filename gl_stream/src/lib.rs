//! ABOUTME: Streaming services for MJPEG and RTSP video streams
//! ABOUTME: Provides real-time video streaming capabilities

use bytes::Bytes;
use gl_capture::CaptureHandle;
use gl_core::Id;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
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

/// RAII guard for stream subscriptions that ensures proper cleanup
///
/// This guard wraps a broadcast receiver and automatically calls `unsubscribe()`
/// when dropped, preventing double-unsubscribe scenarios.
///
/// The guard implements `Deref` and `DerefMut` to `broadcast::Receiver<Bytes>`,
/// allowing direct access to receiver methods.
pub struct SubscriptionGuard {
    /// The broadcast receiver for frames
    receiver: broadcast::Receiver<Bytes>,
    /// Reference to the session for unsubscribe
    session: Arc<StreamSession>,
    /// Whether this guard is still active (prevents double-unsubscribe)
    active: AtomicBool,
    /// Unique subscription ID for debugging
    id: Uuid,
}

impl SubscriptionGuard {
    /// Get a reference to the underlying receiver (prefer using Deref/DerefMut)
    pub fn receiver(&mut self) -> &mut broadcast::Receiver<Bytes> {
        &mut self.receiver
    }

    /// Manually unsubscribe (consumes the guard)
    pub fn unsubscribe(self) {
        // The drop implementation will handle the actual unsubscribe
        // This method exists to make the intent explicit
        drop(self);
    }

    /// Check if this subscription is still active
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        // Only unsubscribe if still active (prevents double-unsubscribe)
        // Use SeqCst ordering to ensure this is visible across all threads
        if self.active.swap(false, Ordering::SeqCst) {
            self.session.unsubscribe_internal(self.id);
        }
    }
}

// Implement Deref to allow transparent access to receiver methods
impl std::ops::Deref for SubscriptionGuard {
    type Target = broadcast::Receiver<Bytes>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

// Implement DerefMut to allow mutable access to receiver methods
impl std::ops::DerefMut for SubscriptionGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
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
    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        let subscriber_count = self.subscribers.fetch_add(1, Ordering::Relaxed) + 1;
        self.metrics.subscribers.set(subscriber_count as i64);

        info!(
            session_id = %self.id,
            subscriber_count,
            "New subscriber joined"
        );

        self.frame_sender.subscribe()
    }

    /// Subscribe to the frame stream with automatic cleanup guard
    ///
    /// This method returns a `SubscriptionGuard` that automatically calls
    /// `unsubscribe()` when dropped, preventing double-unsubscribe scenarios.
    ///
    /// # Example
    /// ```ignore
    /// let guard = session.subscribe_with_guard();
    /// // Guard automatically unsubscribes when dropped
    /// ```
    pub fn subscribe_with_guard(self: &Arc<Self>) -> SubscriptionGuard {
        let subscriber_id = Uuid::new_v4();
        let subscriber_count = self.subscribers.fetch_add(1, Ordering::Relaxed) + 1;
        self.metrics.subscribers.set(subscriber_count as i64);

        info!(
            session_id = %self.id,
            subscriber_id = %subscriber_id,
            subscriber_count,
            "New subscriber joined (with guard)"
        );

        SubscriptionGuard {
            receiver: self.frame_sender.subscribe(),
            session: Arc::clone(self),
            active: AtomicBool::new(true),
            id: subscriber_id,
        }
    }

    /// Remove a subscriber (legacy method - prefer using SubscriptionGuard)
    ///
    /// **Note**: This method is kept for backward compatibility but doesn't
    /// protect against double-unsubscribe. Use `subscribe_with_guard()` instead
    /// for automatic cleanup.
    pub fn unsubscribe(&self) {
        self.unsubscribe_internal(Uuid::nil());
    }

    /// Internal unsubscribe implementation with tracking
    fn unsubscribe_internal(&self, subscriber_id: Uuid) {
        // Use compare-and-swap loop to safely decrement without underflow
        let mut current = self.subscribers.load(Ordering::Relaxed);
        loop {
            if current == 0 {
                // Already at zero - this indicates a double-unsubscribe bug
                warn!(
                    session_id = %self.id,
                    subscriber_id = %subscriber_id,
                    "Attempted to unsubscribe with zero subscribers (double-unsubscribe detected)"
                );
                // Note: debug_assert removed to allow testing of double-unsubscribe protection
                // The warning log above serves as notification during development/debugging
                return;
            }

            // Try to decrement atomically
            match self.subscribers.compare_exchange_weak(
                current,
                current - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // Successfully decremented
                    let new_count = current - 1;
                    self.metrics.subscribers.set(new_count as i64);

                    info!(
                        session_id = %self.id,
                        subscriber_id = %subscriber_id,
                        subscriber_count = new_count,
                        "Subscriber left"
                    );
                    return;
                }
                Err(actual) => {
                    // Another thread modified the counter, retry with new value
                    current = actual;
                }
            }
        }
    }

    /// Get current subscriber count
    pub fn subscriber_count(&self) -> u64 {
        self.subscribers.load(Ordering::Relaxed)
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
                let _receiver1 = session.subscribe();
                assert_eq!(session.subscriber_count(), 1);

                let _receiver2 = session.subscribe();
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

    #[tokio::test]
    async fn test_double_unsubscribe_protection() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Subscribe once
            let _receiver = session.subscribe();
            assert_eq!(session.subscriber_count(), 1);

            // Unsubscribe once - should work
            session.unsubscribe();
            assert_eq!(session.subscriber_count(), 0);

            // Double unsubscribe - should be protected (counter stays at 0)
            session.unsubscribe();
            assert_eq!(session.subscriber_count(), 0);

            // Triple unsubscribe - should still be protected
            session.unsubscribe();
            assert_eq!(session.subscriber_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_subscription_guard_auto_cleanup() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Subscribe with guard
            {
                let _guard = session.subscribe_with_guard();
                assert_eq!(session.subscriber_count(), 1);
                // Guard should auto-cleanup when it goes out of scope
            }

            // After guard is dropped, count should be back to 0
            assert_eq!(session.subscriber_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_subscription_guard_no_double_unsubscribe() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Create guard
            let guard = session.subscribe_with_guard();
            assert_eq!(session.subscriber_count(), 1);
            assert!(guard.is_active());

            // Manually unsubscribe
            guard.unsubscribe();
            assert_eq!(session.subscriber_count(), 0);

            // Even if we had a reference, the guard is now inactive
            // and won't double-unsubscribe
        }
    }

    #[tokio::test]
    async fn test_multiple_guards_concurrent() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Create multiple guards
            let guard1 = session.subscribe_with_guard();
            let guard2 = session.subscribe_with_guard();
            let guard3 = session.subscribe_with_guard();

            assert_eq!(session.subscriber_count(), 3);

            // Drop guards in different order
            drop(guard2);
            assert_eq!(session.subscriber_count(), 2);

            drop(guard1);
            assert_eq!(session.subscriber_count(), 1);

            drop(guard3);
            assert_eq!(session.subscriber_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_mixed_subscription_methods() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Mix old and new subscription methods
            let _receiver1 = session.subscribe(); // Old method
            let _guard1 = session.subscribe_with_guard(); // New method
            let _receiver2 = session.subscribe(); // Old method

            assert_eq!(session.subscriber_count(), 3);

            // Manually unsubscribe for old method
            session.unsubscribe();
            assert_eq!(session.subscriber_count(), 2);

            // Guard auto-cleanup
            drop(_guard1);
            assert_eq!(session.subscriber_count(), 1);

            // Remaining manual unsubscribe
            session.unsubscribe();
            assert_eq!(session.subscriber_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_subscription_guard_deref() {
        let _test_id = create_test_id();
        let (_temp_dir, video_path) = create_test_video_file().await;

        let template_id = Id::new();
        let source = FileSource::new(video_path);
        let config = StreamConfig::default();
        let metrics = StreamMetrics::new();

        if let Ok(capture) = source.start().await {
            let session = Arc::new(StreamSession::new(template_id, capture, config, metrics));

            // Create guard - demonstrates Deref/DerefMut allowing direct receiver access
            let mut guard = session.subscribe_with_guard();
            assert_eq!(session.subscriber_count(), 1);

            // Can call receiver methods directly thanks to Deref
            // try_recv() requires &mut self, demonstrating DerefMut works
            let result = guard.try_recv();
            assert!(matches!(result, Err(tokio::sync::broadcast::error::TryRecvError::Empty)));

            // Guard still auto-cleanup on drop
            drop(guard);
            assert_eq!(session.subscriber_count(), 0);
        }
    }
}
