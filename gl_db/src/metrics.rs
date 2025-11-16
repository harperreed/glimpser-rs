//! ABOUTME: Database connection pool metrics
//! ABOUTME: Provides Prometheus metrics for database pool performance monitoring

use prometheus_client::metrics::{counter::Counter, gauge::Gauge};

/// Metrics for database connection pool operations
#[derive(Debug, Clone, Default)]
pub struct PoolMetrics {
    /// Total number of successful connection acquisitions
    pub connections_acquired: Counter,
    /// Total number of failed connection acquisitions
    pub connections_failed: Counter,
    /// Total number of connection acquisition timeouts
    pub connections_timeout: Counter,
    /// Total number of connection retries
    pub connections_retry: Counter,
    /// Current number of idle connections in pool
    pub connections_idle: Gauge,
    /// Current number of active connections in pool
    pub connections_active: Gauge,
    /// Total number of circuit breaker trips
    pub circuit_breaker_trips: Counter,
    /// Current circuit breaker state (0=closed, 1=open, 2=half-open)
    pub circuit_breaker_state: Gauge,
}

impl PoolMetrics {
    /// Create new pool metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record successful connection acquisition
    pub fn record_acquired(&self) {
        self.connections_acquired.inc();
    }

    /// Record failed connection acquisition
    pub fn record_failed(&self) {
        self.connections_failed.inc();
    }

    /// Record connection acquisition timeout
    pub fn record_timeout(&self) {
        self.connections_timeout.inc();
    }

    /// Record connection retry attempt
    pub fn record_retry(&self) {
        self.connections_retry.inc();
    }

    /// Update idle connections count
    pub fn set_idle(&self, count: i64) {
        self.connections_idle.set(count);
    }

    /// Update active connections count
    pub fn set_active(&self, count: i64) {
        self.connections_active.set(count);
    }

    /// Record circuit breaker trip
    pub fn record_circuit_breaker_trip(&self) {
        self.circuit_breaker_trips.inc();
    }

    /// Set circuit breaker state (0=closed, 1=open, 2=half-open)
    pub fn set_circuit_breaker_state(&self, state: i64) {
        self.circuit_breaker_state.set(state);
    }
}
