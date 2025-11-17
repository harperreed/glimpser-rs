# Glimpser-RS: Claude Code Documentation

## Project Overview

**Glimpser-RS** is an enterprise-grade surveillance and monitoring platform built in Rust that captures, analyzes, and summarizes live data from cameras, websites, dashboards, and video streams. This is a high-performance refactoring of a Python-based surveillance platform designed for mission-critical deployments.

**Key Value Propositions:**
- Multi-source monitoring (RTSP cameras, websites, YouTube streams, files)
- AI-powered analysis using OpenAI GPT-4 and CLIP
- Real-time MJPEG/RTSP streaming
- Multi-channel alerting (email, SMS, webhooks, CAP emergency alerts)
- Enterprise job scheduling and automation
- Auto-update system with cryptographic verification

**Current Status:** ~26% complete (45/173 API endpoints implemented)

## Architecture

### Workspace Structure

This is a Rust workspace with 18+ specialized crates organized by domain:

```
glimpser-rs/
├── app/                    # Main binary orchestration and CLI
├── gl_core/               # Foundation types, errors, IDs, tracing
├── gl_config/             # Configuration management with validation
├── gl_db/                 # SQLite database layer with sqlx
├── gl_web/                # Hybrid Axum + Actix-web HTTP server
├── gl_capture/            # Multi-source capture engine (RTSP, FFmpeg, websites)
├── gl_stream/             # MJPEG/RTSP streaming services
├── gl_storage/            # Object storage abstraction (S3/local)
├── gl_scheduler/          # Job scheduling with cron expressions
├── gl_ai/                 # AI client abstraction (OpenAI/stub)
├── gl_vision/             # Motion detection algorithms
├── gl_analysis/           # Analysis pipeline and rule engine
├── gl_notify/             # Multi-channel notification delivery
├── gl_cap/                # CAP emergency alert builder
├── gl_update/             # Auto-update system with signature verification
├── gl_obs/                # Observability and health checks
├── gl_proc/               # External process execution wrapper
├── gl_sched/              # Legacy cron scheduler
└── test_support/          # Shared test utilities
```

### Key Design Patterns

**1. Repository Pattern** (gl_db)
- Each entity has a dedicated repository struct
- All database access goes through repositories
- Connection pooling managed centrally
- Example: `UserRepository`, `StreamRepository`, `SnapshotRepository`

**2. Trait-Based Abstraction**
- `Capturer` trait for all capture sources (RTSP, FFmpeg, websites, etc.)
- `AiClient` trait for online/offline AI implementations
- `Storage` trait for local/S3/Azure backends
- Enables testing, feature-gating, and pluggable implementations

**3. Error Handling**
- All crates export `Result<T>` type alias using `gl_core::Error`
- `thiserror` for structured error types
- Errors propagate up with context using `?` operator
- No panics in production code paths

**4. Configuration Management**
- Environment variables with `GLIMPSER_` prefix
- Strong typing with validation (validator crate)
- Secrets redacted in debug output
- Defaults for development with production auto-detection

**5. Async/Await Throughout**
- Tokio runtime with "full" features
- All I/O operations are async
- Broadcast channels for streaming
- Background tasks use spawned tokio tasks

## Development Practices

### Code Quality Requirements

**Before committing:**
1. Format code: `cargo fmt`
2. Run linter: `cargo clippy --all-features -- -D warnings`
3. Run tests: `cargo test`
4. Check pre-commit hooks pass

**Pre-commit hooks** (`.pre-commit-config.yaml`):
- rustfmt for formatting
- clippy for linting
- Trailing whitespace removal
- Final newline validation
- YAML validation

**CI/CD** (GitHub Actions):
- Format checking (`cargo fmt --check`)
- Linting (`cargo clippy --all-features`)
- Full test suite (`cargo test`)
- Security audit (`cargo audit`, `cargo deny`)
- Documentation generation
- Code coverage reporting

### Testing Guidelines

**Test Organization:**
- Unit tests in same file as implementation (using `#[cfg(test)]`)
- Integration tests in `tests/` directory
- Shared test utilities in `test_support` crate

**Test Requirements:**
- All public APIs must have tests
- Database tests use temporary SQLite databases
- HTTP tests use mock servers (wiremock)
- Async tests use `#[tokio::test]`

**Running Tests:**
```bash
cargo test                    # All tests
cargo test -p gl_db           # Specific crate
cargo test -- --nocapture     # With output
cargo test --test '*'         # Integration tests only
```

**Test Support Utilities:**
- `test_support::create_test_db()` - Temporary database with migrations
- `test_support::mock_config()` - Default test configuration
- `test_support::test_user()` - Test user creation

### Common Development Workflows

**Building:**
```bash
cargo build                   # Debug build
cargo build --release         # Production build
cargo build --all-features    # All optional features
cargo build -p gl_web         # Specific crate
```

**Running:**
```bash
# Bootstrap (create admin user)
cargo run --bin glimpser -- bootstrap

# Start server
cargo run --bin glimpser -- start

# With environment config
GLIMPSER_SERVER_PORT=8080 cargo run --bin glimpser -- start
```

**Docker:**
```bash
make build      # Build images
make up         # Start production
make dev        # Start development mode
make logs       # View logs
make health     # Check health
make clean      # Clean Docker resources
make reset      # Rebuild from scratch
```

**Code Generation:**
```bash
# Database migrations
sqlx migrate add <migration_name>
sqlx migrate run

# OpenAPI spec regeneration
# (Automatic via utoipa macros)
```

### Important Conventions

**1. ID Generation:**
- Use `gl_core::Id` (ULID-based) for all entity IDs
- ULIDs are sortable by creation time
- Generated with `Id::new()`

**2. Timestamps:**
- Use `chrono::Utc::now()` for all timestamps
- Store as ISO8601 strings in SQLite
- Database columns: `created_at`, `updated_at`

**3. Configuration:**
- All config in `gl_config::Settings`
- Load from environment with `Settings::new()`
- Validate on load using `validator` crate
- Secret fields use `SecretString` type

**4. Database Queries:**
- Compile-time verification with sqlx macros
- Use `query_as!` for type-safe queries
- Prepared statements for repeated queries
- Transactions for multi-step operations

**5. Error Messages:**
- User-facing: Clear, actionable messages
- Logs: Include context and error chain
- Don't leak sensitive info in errors
- Use `tracing::error!` for structured logging

**6. API Responses:**
- Consistent JSON structure
- Use DTOs (Data Transfer Objects) not internal models
- Include metadata (pagination, timestamps)
- Proper HTTP status codes

## Database Schema

### Core Tables

**users** - Authentication and user management
- `id` (ULID), `username`, `email`, `password_hash` (Argon2)
- `role` (admin/user), `is_active`

**api_keys** - API token authentication
- `id`, `key_hash`, `name`, `user_id`
- `permissions` (JSON array), `is_active`, `last_used_at`

**templates/streams** - Stream configurations
- `id`, `user_id`, `name`, `description`
- `config` (JSON: source type, URL, capture params)
- `is_default`, `last_executed_at`, `last_error`

**snapshots** - Captured images
- `id`, `template_id`, `user_id`, `file_path`
- `width`, `height`, `file_size`, `checksum`
- `perceptual_hash` (for deduplication)
- `captured_at`

**scheduled_jobs** - Cron-based jobs
- `id`, `name`, `job_type`, `schedule` (cron expression)
- `parameters` (JSON), `enabled`, `max_retries`
- `timeout_seconds`, `priority`, `tags`

**job_executions** - Job execution history
- `id`, `job_id`, `status`, `started_at`, `completed_at`
- `duration_ms`, `result` (JSON), `error`
- `retry_count`, `executed_on` (hostname)

**background_snapshot_jobs** - Async snapshot processing
- `id`, `stream_id`, `user_id`, `status`
- `result_path`, `error_message`, `completed_at`

**analysis_events** - AI/motion analysis results
- `id`, `stream_id`, `analysis_type` (motion/ai/manual)
- `confidence`, `result` (JSON), `created_at`

**notification_deliveries** - Notification tracking
- `id`, `alert_id`, `channel`, `recipient`
- `status`, `message`, `error`, `sent_at`

### Migrations

Located in `gl_db/migrations/`:
- 24 migration files (numbered sequentially)
- Applied automatically on startup
- Use `sqlx migrate` CLI for management
- Always test migrations both up and down

## API Architecture

### Hybrid Web Server

**Axum** (Server-side rendering):
- HTMX-based frontend at `/`
- Server-rendered templates (Askama)
- Routes: `/`, `/streams`, `/settings`, `/admin`

**Actix-web** (REST API):
- JSON API at `/api/*`
- OpenAPI/Swagger documentation at `/swagger-ui/`
- Prometheus metrics at `/metrics`
- Health checks at `/health`, `/healthz`

**Ports:**
- Main server: `8185` (configurable via `GLIMPSER_SERVER_PORT`)
- Observability: `9000` (metrics, health checks)

### API Endpoint Categories

**Authentication** (`/api/auth/*`):
- Login, signup, logout, current user

**Streams** (`/api/streams/*`):
- CRUD operations, pagination, import/export

**Stream Operations** (`/api/stream/{id}/*`):
- Snapshot capture (sync/async)
- Thumbnail retrieval
- Recent snapshots
- MJPEG streaming
- Start/stop capture

**Admin** (`/api/admin/*`):
- User management
- API key management
- Stream administration
- Update management

**AI** (`/api/ai/*`):
- Content summarization
- Frame description
- Event classification

**Alerts** (`/api/alerts/*`):
- Alert CRUD operations

### Middleware Stack

1. **Request Logging** - tracing with request IDs
2. **Authentication** - JWT validation (secure cookies)
3. **Rate Limiting** - IP-based, configurable limits
4. **Body Size Limits** - Route-specific overrides
5. **CORS** - Configurable origins
6. **Error Handling** - Consistent JSON error responses

## Configuration

### Required Environment Variables

```bash
# Minimum for development
GLIMPSER_SECURITY_JWT_SECRET=<32+ character secret>
GLIMPSER_DATABASE_PATH=glimpser.db

# Recommended for development
GLIMPSER_SERVER_PORT=8185
GLIMPSER_SERVER_HOST=127.0.0.1
RUST_LOG=debug,sqlx=warn
```

### Production Requirements

```bash
# Security
GLIMPSER_SECURITY_JWT_SECRET=<strong-random-secret>
GLIMPSER_SECURITY_SECURE_COOKIES=true  # Requires HTTPS

# Database
GLIMPSER_DATABASE_PATH=/data/glimpser.db
GLIMPSER_DATABASE_POOL_SIZE=20
GLIMPSER_DATABASE_SQLITE_WAL=true

# Storage
GLIMPSER_STORAGE_ARTIFACTS_DIR=/data/artifacts
# Optional: S3 configuration
GLIMPSER_STORAGE_OBJECT_STORE_URL=s3://bucket-name
GLIMPSER_STORAGE_BUCKET=glimpser-media

# AI (optional)
GLIMPSER_AI_USE_ONLINE=true
GLIMPSER_AI_API_KEY=<openai-key>
GLIMPSER_AI_MODEL=gpt-4

# External services (optional)
GLIMPSER_EXTERNAL_SMTP_HOST=smtp.example.com
GLIMPSER_EXTERNAL_SMTP_PORT=587
GLIMPSER_EXTERNAL_SMTP_USERNAME=<email>
GLIMPSER_EXTERNAL_SMTP_PASSWORD=<password>
```

### Feature Gates

Some dependencies are feature-gated:
- `opencv` - Motion detection with OpenCV (requires system libs)
- `cuda` - Hardware acceleration (requires NVIDIA runtime)
- `rtsp` - RTSP streaming via GStreamer (requires GStreamer)

Build with all features: `cargo build --all-features`

## Current Implementation Status

### Completed ✅

**Core Infrastructure:**
- Rust workspace setup with 18+ crates
- SQLite database with 24 migrations
- JWT authentication with Argon2 password hashing
- Hybrid Axum + Actix-web server
- Rate limiting and body size limits
- HTMX frontend with SSR
- Docker support with multi-stage builds
- Health checks and graceful shutdown

**Stream Management:**
- CRUD operations with pagination
- ETag-based caching
- Configuration validation
- Multi-source support (RTSP, FFmpeg, websites, YouTube, files)
- Thumbnail generation
- Import/export functionality

**Real-Time Streaming:**
- MJPEG streaming with broadcast channels
- Memory-first snapshot serving (10-50ms latency)
- Multi-consumer support
- Connection metrics

**Job Scheduling:**
- Cron-based scheduling
- Job persistence and execution history
- Retry mechanisms
- Background snapshot jobs
- Perceptual hash deduplication

**Notifications:**
- Multi-channel delivery (SMTP, webhooks, push)
- Circuit breaker pattern
- Delivery tracking

### In Progress / Planned ⏳

**AI Integration:**
- Complete GPT-4 content analysis pipeline
- CLIP image classification
- Advanced motion detection with OpenCV
- Automated alert generation from AI analysis

**Enterprise Features:**
- CAP emergency alert generation
- Auto-update system activation
- Role-based access control (RBAC)
- Enhanced API key permissions

**Advanced Capture:**
- Browser automation (Selenium/thirtyfour)
- Hardware acceleration (CUDA/VAAPI)
- Concurrent multi-source processing at scale
- JavaScript execution in website captures

**Observability:**
- Comprehensive Prometheus metrics
- Performance dashboards
- Alerting rules
- Distributed tracing integration

**Progressive Web App:**
- Service worker for offline support
- Enhanced push notifications
- App manifest

## System Dependencies

### Required

```bash
# Debian/Ubuntu
sudo apt-get install -y \
    chromium chromium-driver \
    ffmpeg yt-dlp \
    pkg-config libssl-dev
```

### Optional (Feature-Gated)

```bash
# For OpenCV motion detection
sudo apt-get install -y \
    libopencv-dev clang libclang-dev

# For RTSP streaming
sudo apt-get install -y \
    libglib2.0-dev \
    libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev \
    libgstreamer-plugins-bad1.0-dev \
    libgstreamer-plugins-good1.0-dev \
    libgstrtspserver-1.0-dev

# For hardware acceleration
# NVIDIA CUDA runtime (version-specific)
```

## Important Files and Directories

**Configuration:**
- `Cargo.toml` - Workspace dependencies
- `rustfmt.toml` - Code formatting rules
- `clippy.toml` - Linter configuration
- `deny.toml` - Dependency restrictions
- `rust-toolchain.toml` - Toolchain specification
- `.pre-commit-config.yaml` - Pre-commit hooks

**CI/CD:**
- `.github/workflows/ci.yml` - Main CI pipeline
- `.github/workflows/rust-audit.yml` - Security audit
- `.github/workflows/rust-fmt.yml` - Format check
- `.github/workflows/coverage.yml` - Code coverage

**Database:**
- `gl_db/migrations/` - SQLx migrations (24 files)
- `data/glimpser.db` - Runtime SQLite database

**Static Assets:**
- `static/` - Frontend assets (CSS, JS, images)

**Documentation:**
- `rust_spec.md` - Rust project specification
- `performance.md` - Performance analysis
- `remaining-work.md` - Work tracking
- `code-quality-review.md` - Code quality reports
- `audit.md` - Security audit findings

## Common Tasks for Claude

### Adding a New API Endpoint

1. Define request/response DTOs in `gl_web/src/dto/`
2. Add handler function in `gl_web/src/handlers/`
3. Register route in appropriate router (`gl_web/src/routes/`)
4. Add OpenAPI documentation with `#[utoipa::path]`
5. Update OpenAPI spec in `gl_web/src/lib.rs`
6. Write integration tests in `gl_web/tests/`
7. Update this documentation

### Adding a New Database Table

1. Create migration: `sqlx migrate add <name>`
2. Write SQL in generated file
3. Add repository struct in `gl_db/src/repositories/`
4. Implement CRUD methods using `query_as!`
5. Export repository from `gl_db/src/lib.rs`
6. Write tests in repository file
7. Update schema documentation in this file

### Adding a New Capture Source

1. Implement `Capturer` trait in `gl_capture/src/`
2. Add configuration struct in `gl_config/src/`
3. Register in capture source enum
4. Add tests with mock data
5. Update API to expose new source type
6. Document in user-facing docs

### Adding a New Notification Channel

1. Implement delivery logic in `gl_notify/src/channels/`
2. Add configuration in `gl_config/src/external.rs`
3. Add channel to `NotificationChannel` enum
4. Implement retry logic and error handling
5. Add delivery tracking
6. Write integration tests with mock server

## Security Considerations

**Authentication:**
- Passwords hashed with Argon2 (OWASP recommended parameters)
- JWT tokens with configurable expiry
- Secure cookie settings for production (HTTPS-only, SameSite)
- API keys hashed before storage

**Input Validation:**
- All user input validated using `validator` crate
- SQL injection prevented by sqlx parameterized queries
- XSS protection via Content Security Policy
- Path traversal prevented in file operations

**Rate Limiting:**
- IP-based rate limiting on all endpoints
- Configurable limits per route
- DOS protection for expensive operations

**Dependencies:**
- Regular security audits with `cargo audit`
- License checking with `cargo deny`
- Minimal dependency tree
- No known critical vulnerabilities

**Secrets Management:**
- Environment variables for secrets
- Redacted in logs and debug output
- Never committed to repository
- Separate secrets for dev/staging/prod

## Performance Characteristics

**Database:**
- SQLite with WAL mode for concurrent reads
- Connection pooling (configurable size)
- LRU cache for frequent queries
- Perceptual hashing for deduplication

**Streaming:**
- Memory-first snapshot serving (10-50ms latency)
- Broadcast channels for multi-consumer efficiency
- Backpressure handling
- Connection metrics tracking

**Concurrency:**
- Tokio async runtime with work-stealing scheduler
- Thousands of concurrent streams supported
- Non-blocking I/O throughout
- Bounded channels prevent memory exhaustion

**Resource Usage:**
- Minimal: 512MB RAM, 2 cores
- Recommended: 2GB+ RAM, 4+ cores
- Disk: Variable based on snapshot retention

## Troubleshooting

**Build Issues:**
- Ensure nightly toolchain: `rustup default nightly`
- Check system dependencies installed
- Clear target directory: `cargo clean`
- Update dependencies: `cargo update`

**Database Issues:**
- Check migrations applied: `sqlx migrate info`
- Verify database file permissions
- Check disk space
- Enable WAL mode for concurrency

**Runtime Issues:**
- Check environment variables set correctly
- Verify external services reachable (SMTP, S3, OpenAI)
- Review logs: `RUST_LOG=debug cargo run`
- Check health endpoint: `curl http://localhost:8185/health`

**Docker Issues:**
- Check volume mounts for persistence
- Verify port mappings
- Review container logs: `make logs`
- Rebuild images: `make reset`

## Resources

**Rust Documentation:**
- Tokio: https://tokio.rs/
- Actix-web: https://actix.rs/
- Axum: https://github.com/tokio-rs/axum
- sqlx: https://github.com/launchbadge/sqlx

**Project Documentation:**
- OpenAPI spec: http://localhost:8185/swagger-ui/
- Prometheus metrics: http://localhost:9000/metrics
- Health check: http://localhost:8185/health

**Development:**
- Pre-commit hooks: https://pre-commit.com/
- Conventional commits: https://www.conventionalcommits.org/

## Working with Claude Code

**When implementing features:**
1. Understand the crate structure (workspace-based)
2. Follow existing patterns (repositories, traits, error handling)
3. Add comprehensive tests
4. Update OpenAPI documentation
5. Run full test suite before committing
6. Update this documentation if adding major features

**When fixing bugs:**
1. Write a failing test first
2. Fix the bug
3. Ensure test passes
4. Check for similar issues in codebase
5. Add regression test if applicable

**When refactoring:**
1. Ensure tests pass before starting
2. Make small, incremental changes
3. Run tests frequently
4. Update documentation
5. Verify no performance regression

**Code review checklist:**
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Error handling comprehensive
- [ ] No unwrap() in production code
- [ ] Logging added for debugging
- [ ] Security implications considered
- [ ] Performance impact assessed
- [ ] OpenAPI spec updated (if API change)

## Git Workflow

**Branch:** `claude/write-readme-01De8vMS9Wwy2k9MRcMteANG`

**Commit Messages:**
- Follow conventional commits format
- Examples: "feat:", "fix:", "docs:", "refactor:", "test:", "chore:"
- Be descriptive but concise
- Reference issue numbers when applicable

**Before Pushing:**
```bash
cargo fmt                           # Format code
cargo clippy --all-features         # Lint
cargo test                          # Run tests
git add .                           # Stage changes
git commit -m "feat: description"   # Commit
git push -u origin <branch-name>    # Push
```

---

*This documentation is maintained alongside the codebase. Update it when making significant changes to architecture, patterns, or workflows.*
