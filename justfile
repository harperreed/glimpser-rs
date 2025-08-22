# Justfile for glimpser project

# Format all code
format:
    cargo fmt --all

# Run linting
lint: format
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run all tests
test:
    cargo test --workspace

# Full CI pipeline
ci: lint test
    @echo "CI checks completed successfully"

# Build all targets
build:
    cargo build --workspace --all-targets

# Clean build artifacts
clean:
    cargo clean

# Check for security vulnerabilities
audit:
    cargo audit

# Check for licensing and supply chain issues
deny:
    cargo deny check
