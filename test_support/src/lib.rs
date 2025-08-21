//! ABOUTME: Shared testing utilities and helper functions
//! ABOUTME: Common test fixtures and mocks for all crates

/// Simple test helper function to demonstrate cross-crate testing
pub fn create_test_id() -> String {
    "test-id-123".to_string()
}

/// Helper for creating temporary directories in tests
pub fn temp_dir_path() -> std::path::PathBuf {
    std::env::temp_dir().join("glimpser-test")
}
