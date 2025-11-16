//! ABOUTME: Metrics collection for streaming services
//! ABOUTME: Provides Prometheus metrics for stream performance monitoring

use prometheus_client::metrics::{counter::Counter, gauge::Gauge};

/// Metrics for streaming operations
#[derive(Debug, Clone, Default)]
pub struct StreamMetrics {
    /// Total number of frames generated
    pub frames_generated: Counter,
    /// Total number of frame generation errors
    pub frame_errors: Counter,
    /// Total number of frame generation timeouts
    pub frame_timeouts: Counter,
    /// Current number of active subscribers
    pub subscribers: Gauge,
    /// Total number of client connections
    pub connections_total: Counter,
    /// Total number of client disconnections
    pub disconnections_total: Counter,
    /// Total number of dropped frames (backpressure)
    pub frames_dropped: Counter,
    /// Total number of sequence gaps detected across all subscribers
    pub sequence_gaps_total: Counter,
}

impl StreamMetrics {
    /// Create new streaming metrics
    pub fn new() -> Self {
        Self::default()
    }
}
