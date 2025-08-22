//! ABOUTME: Core types, errors, IDs, and tracing utilities
//! ABOUTME: Foundation crate used by all other glimpser components

pub mod error;
pub mod id;
pub mod telemetry;
pub mod time;

pub use error::{Error, Result};
pub use id::Id;
pub use time::{to_rfc3339, utc_now, MonotonicTimer};

#[cfg(test)]
mod tests {
    use test_support::create_test_id;

    #[test]
    fn test_cross_crate_usage() {
        let test_id = create_test_id();
        assert_eq!(test_id, "test-id-123");
    }
}
