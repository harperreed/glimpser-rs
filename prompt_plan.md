# GLIMPSER (RUST) — BUILD BLUEPRINT & TDD PROMPTS

## 0) Ground Rules

* **Cargo workspace** with small, cohesive crates.
* **Fail fast**: typed config, strict linting, compile-time SQL checks, feature flags around heavy deps.
* **Test-first**: unit → integration → e2e; mocks for networks/browsers/ffmpeg.
* **Observability from day 1**: `tracing`, structured logs, metrics.
* **Hardening**: input validation, rate limits, least-privilege, secret hygiene, supply-chain scanning.
* **No orphan code**: every step compiles, tests, and wires into the running binary.

---

## 1) High-Level Blueprint (Phases → Milestones)

### Phase A — Foundations

* **A1**: Repo skeleton (workspace, CI, linting, security gates, Justfile).
* **A2**: Core crate (`gl_core`): error types, ids (ULID), time, feature flags, `tracing` init.
* **A3**: Config crate (`gl_config`): typed config, env/.env, schema validation, secrets policy.
* **A4**: Observability crate (`gl_obs`): tracing layers, Prometheus metrics, health/ready endpoints.

### Phase B — Data & API Skeleton

* **B1**: DB crate (`gl_db`): `sqlx`, migrations, repositories, SQLite first; WAL mode; compile-time SQL.
* **B2**: Domain models & RBAC in `gl_core`.
* **B3**: Web crate (`gl_web`): Actix Web, scopes, typed extractors, JWT/API-key auth; OpenAPI (utoipa).
* **B4**: Minimal endpoints: `/healthz`, `/readyz`, `/version`, `/auth/login`, `/me`.

### Phase C — Capture Engine

* **C1**: Process runner crate (`gl_proc`): `tokio::process::Command`, sandboxing, timeouts, logs.
* **C2**: Capture crate (`gl_capture`): trait-based sources (ffmpeg, yt-dlp, website via Selenium).
* **C3**: Scheduler crate (`gl_sched`): `tokio-cron-scheduler` jobs backed by DB; idempotent runs.
* **C4**: Stream service crate (`gl_stream`): MJPEG (HTTP), RTSP (feature-gated via `gstreamer`).

### Phase D — Notifications & Alerts

* **D1**: Notify crate (`gl_notify`): adapters (SMTP via `lettre`, Twilio via `reqwest`, Webhooks, WebPush).
* **D2**: CAP alerts (`gl_cap`): CAP XML builder/validator (quick-xml); profile presets; audit log.

### Phase E — AI & Analysis

* **E1**: AI crate (`gl_ai`): trait `AiClient`, impl via `reqwest` (OpenAI) with testable stubs.
* **E2**: Vision crate (`gl_vision`): `opencv` + `image`; motion detection baseline; CLIP hooks (feature).
* **E3**: Analysis pipelines: per-template “processors” composed via traits, metrics tagged by template id.

### Phase F — PWA, Updates, Packaging

* **F1**: Static serving of PWA; ETag/Cache headers; CSP.
* **F2**: Updater crate (`gl_update`): sidecar/self-replace strategy; GitHub releases check; signature verify.
* **F3**: Packaging: Docker multi-stage (distroless), systemd service, K8s manifests (optional).

### Phase G — Enterprise Hardening & E2E

* **G1**: Rate limit, input validation, JSON schema, body size limits; `cargo-deny`, `cargo-audit`.
* **G2**: S3/object storage abstraction (`object_store`) for artifacts; pluggable.
* **G3**: E2E: synthetic sources (local mp4, headless browser in CI via Selenium Grid), load tests.
* **G4**: Docs: OpenAPI UI, runbooks, SLOs/alerts, dashboards.

---

## 2) Break Down Again (Milestones → Stories)

### A1 Repo Skeleton

* S1: Create workspace with crates: `app`, `gl_core`, `gl_config`, `gl_obs`, `gl_db`, `gl_web`, `gl_proc`, `gl_capture`, `gl_sched`, `gl_stream`, `gl_notify`, `gl_cap`, `gl_ai`, `gl_vision`, `gl_update`, `test_support`.
* S2: Add `rust-toolchain.toml`, MSRV, `clippy.toml`, `rustfmt.toml`.
* S3: GH Actions: fmt, clippy (deny warnings), test, `cargo-audit`, `cargo-deny`.
* S4: `justfile` + pre-commit hooks (fmt, clippy, tests, deny/audit).

### A2 Core

* S1: `Error` enum with `thiserror`.
* S2: `Id` newtype (ULID), time helpers, `Result<T>`.
* S3: `tracing` setup: JSON logs in prod; local pretty.
* S4: Feature flags scaffold (`heavy_opencv`, `rtsp`, `ai_online`).

### A3 Config

* S1: `Config` struct; env + .env loading; serde; defaults.
* S2: Validation (`validator` or custom).
* S3: Secrets redaction in logs.
* S4: Integration test: invalid config fails fast.

### A4 Observability

* S1: Prometheus exporter (`metrics`, `metrics-exporter-prometheus` or `prometheus_client`).
* S2: `/metrics`, `/healthz`, `/readyz` handlers as a small Actix service.
* S3: Tracing layers: env-filter, span fields (template\_id, job\_id).
* S4: Tests for health & metrics scrape.

### B1 Database

* S1: Migrations (`sqlx-cli`), enable SQLite WAL.
* S2: Tables: `users`, `api_keys`, `templates`, `captures`, `jobs`, `alerts`, `events`.
* S3: Repos with compile-time checked queries; transactions.
* S4: Seed & fixtures; test container path for Postgres (optional).

### B3 Web + Auth

* S1: Actix skeleton + scopes; JSON extraction with validation.
* S2: Password hashing (`argon2`), JWT (`jsonwebtoken`), API keys.
* S3: RBAC roles: admin/operator/viewer.
* S4: OpenAPI docs (utoipa) exposed at `/docs`.

### C Capture & Scheduling

* C1: `gl_proc`: resilient process runner, timeouts, stderr->tracing, resource hints.
* C2: `gl_capture`: `CaptureSource` trait; implement `FfmpegSource` (record & snapshot) and `FileSource` (for tests).
* C3: `gl_sched`: cron jobs persisted in DB; dedupe; jitter; manual trigger endpoint.
* C4: `gl_stream`: MJPEG server; `rtsp` behind feature flag (gstreamer).

### D Notifications

* D1: `gl_notify`: `Notifier` trait; SMTP, Twilio, Webhook, WebPush adapters; retry/backoff; circuit breaker.
* D2: `gl_cap`: CAP builder; schema validation; sample profiles; tests against CAP spec samples.

### E Analysis

* E1: `gl_ai`: `AiClient` trait; in-mem stub; `reqwest` impl (OpenAI).
* E2: `gl_vision`: basic motion detection; OpenCV MOG2 behind feature; pure-Rust fallback.
* E3: Pipeline orchestration: run analyzers per capture; emit events; configurable thresholds; metrics.

### F Packaging

* F1: Serve PWA; CSP; gz/brotli.
* F2: Updater: self-replace or sidecar; signature & version checks; rollback.
* F3: Dockerfile (distroless), SBOM, systemd unit.

### G E2E & Hardening

* G1: Rate limit (actix-ratelimit), body limits, JSON schemas; fuzz boundary handlers (arbitrary JSON).
* G2: Object storage abstraction for artifacts.
* G3: E2E suite: spin Actix + SQLite + synthetic streams + mock Twilio/OpenAI; headless Selenium (Grid).
* G4: Dashboards & runbooks.

---

## 3) One More Pass (Stories → Micro-Steps with DoD)

Below are representative “micro-steps” (we’ll push deeper for the early foundation where risk is highest).

### Example: A1 Repo Skeleton (micro)

1. Create workspace, minimal crates compile. **DoD**: `cargo build` ok.
2. Add `justfile` targets: `format`, `lint`, `test`, `ci`. **DoD**: `just lint` passes.
3. CI workflow: cache, fmt, clippy (deny-warnings), tests, `cargo-deny`, `cargo-audit`. **DoD**: CI green on empty main.
4. `test_support` crate with test helpers. **DoD**: a sample test uses helper.

### Example: B3 Auth (micro)

1. Add `/auth/login` with password hashing; store `users` with salted Argon2. **DoD**: unit + integration tests with in-memory SQLite.
2. Issue JWT (HS256) with roles; middleware extracts claims. **DoD**: a protected endpoint returns 401/403 as expected.
3. API key flow (for services). **DoD**: header-based key validated; rate-limited.

### Example: C2 Capture (micro)

1. Define `CaptureSource` trait; add `FileSource` that yields frames from local mp4 for tests. **DoD**: integration test streams MJPEG from file.
2. Implement `FfmpegSource` shell-out with resilient process runner. **DoD**: snapshot endpoint returns JPEG.
3. Wire scheduler job → starts/stops `FfmpegSource`. **DoD**: scheduled run emits event row and metrics.

---

## 4) TDD Prompt Pack (Sequential, No Orphans)

**How to use**: Run these prompts in order. Each prompt results in compiling code + tests passing. Each ends with wire-up so nothing is left dangling. (Prompts are pure text; paste into your code-gen LLM.)

---

### Prompt 01 — Initialize Workspace, CI, and Tooling ✅ COMPLETED

```text
You are implementing the initial Rust workspace for "glimpser".

Goal:
- Create a Cargo workspace with crates: app, gl_core, gl_config, gl_obs, gl_db, gl_web, gl_proc, gl_capture, gl_sched, gl_stream, gl_notify, gl_cap, gl_ai, gl_vision, gl_update, test_support.
- Add rust-toolchain.toml (stable) and MSRV policy (e.g., 1.75+).
- Add rustfmt/clippy config to enforce style and deny warnings.
- Add a Justfile with tasks: format, lint, test, ci.
- Add a GitHub Actions workflow that runs fmt, clippy (deny warnings), cargo test, cargo-deny, cargo-audit.

Requirements:
- Each crate compiles with a minimal lib.rs, proper edition = 2021 or 2024 (if available).
- Workspace-level Cargo.toml sets resolver = "2".
- `test_support` has a simple function used by a trivial test in `gl_core` to prove cross-crate testing.
- CI uses caching for Rust builds.

Deliverables:
- Workspace files, CI yaml, Justfile.
- One test that uses `test_support` from `gl_core` and passes.
```

---

### Prompt 02 — Core Types, Errors, and Tracing Bootstrap ✅ COMPLETED

```text
Extend `gl_core` with:
- `Error` enum using `thiserror`, a `Result<T>` alias.
- `Id` newtype backed by ULID (use `ulid` crate), with Display/FromStr/serde.
- Time helpers: utc_now(), to_rfc3339(), monotonic durations.
- Tracing bootstrap function: init_tracing(env: &str, service: &str) that sets JSON logs for prod and pretty logs for dev.

Testing:
- Unit tests for Id parse/serde roundtrips.
- A test that initializes tracing twice should be safe (idempotent or guarded).
- Add doc-comments and examples that compile.

Wire-up:
- Make `app` binary call `gl_core::telemetry::init_tracing()` on startup and log "glimpser starting".
```

---

### Prompt 03 — Typed Config With Validation ✅ COMPLETED

```text
Create crate `gl_config`:

Features:
- `Config` struct with sections: server (host, port), database (path, pool_size, sqlite_wal: bool), security (jwt_secret, argon2_params), features (enable_rtsp, enable_ai), external (twilio, smtp, webhook_base_url), storage (object_store_url, bucket).
- Load from env + optional .env file using `config` crate + `serde`.
- Validation: ensure required fields present; non-empty secrets; ports in range.
- Redact secrets when Debug/Display.

Tests:
- Unit tests for happy-path and failure.
- Integration test: invalid config fails fast and returns a helpful error.

Wire-up:
- `app` loads Config at startup; if invalid, process exits with non-zero.
- `app` prints sanitized config at debug level (secrets redacted).
```

---

### Prompt 04 — Observability Service (Health & Metrics) ✅ COMPLETED

```text
Add crate `gl_obs`:

Implement:
- A small Actix Web service factory that provides routes:
  - GET /healthz -> 200 OK JSON {status:"ok"}
  - GET /readyz -> hooked to a readiness gate (atomic flag, initially true)
  - GET /metrics -> Prometheus scrape using `prometheus_client` (preferred) or `metrics` + exporter
- Tracing middleware that adds request_id, client_ip (best-effort), and span per request.

Tests:
- Integration test using actix-web::test for all three endpoints, asserting content type and sample metrics line exists.
- Unit test for readiness gate toggling.

Wire-up:
- `app` binary spins a small HTTP server on 0.0.0.0:9000 serving only observability routes (port configurable).
```

---

### Prompt 05 — Database: SQLite + Sqlx + Migrations ✅ COMPLETED

```text
Create crate `gl_db` with sqlx (sqlite feature):

Implement:
- Migrations via `sqlx::migrate!()`; include an initial migration to enable WAL, create tables:
  users(id ULID pk, email unique, password_hash, role enum, created_at),
  api_keys(id ULID pk, key_hash, owner_user_id fk, created_at),
  templates(id ULID pk, name, kind enum('ffmpeg','yt','website','file'), config_json, schedule, enabled bool, created_at),
  captures(id ULID pk, template_id fk, started_at, ended_at, status enum, artifact_uri nullable),
  jobs(id ULID pk, kind enum, scheduled_for, status enum, payload_json, created_at, updated_at),
  alerts(id ULID pk, template_id fk, kind enum, content_json, created_at),
  events(id ULID pk, template_id fk, level enum, message, metadata_json, created_at).

- Provide a `Db` struct with pool and methods: `connect(&Config)`, `ping()`, `transaction()`.

- Repository modules for Users and Templates with compile-time checked queries.

Tests:
- Integration tests using temp SQLite file; ensure WAL pragma is set; CRUD ops tested; unique constraints enforced.

Wire-up:
- `app` connects to DB on startup, runs migrations, logs pool size, and fails fast if connect/migrate fails.
```

---

### Prompt 06 — Web Skeleton, Auth (JWT + API Keys), OpenAPI ✅ COMPLETED

```text
Add crate `gl_web` (Actix Web):

Implement:
- Scopes: /api/auth, /api/admin, /api/public.
- Models with `serde` and validation (email, lengths).
- Password hashing with `argon2` (salted) and constant-time comparison.
- Login endpoint returns JWT with role claims.
- API-key middleware: when `x-api-key` is present, authenticate as service principal.
- RBAC guard: role-based access to admin endpoints.
- OpenAPI generation with `utoipa` + serve Swagger UI at /docs.

Tests:
- actix-web integration tests for auth flows (happy/invalid), RBAC 401/403 paths, API-key path.

Wire-up:
- `app` mounts `gl_web` on a configurable port; exposes `/docs`, `/api/auth/login`, `/api/me`, and `/api/templates` (list only).
```

---

### Prompt 07 — Process Runner (`gl_proc`) With Timeouts & Logs ✅ COMPLETED

```text
Create `gl_proc`:

Implement:
- `CommandSpec` { program: PathBuf, args: Vec<String>, env: Vec<(String,String)>, cwd: Option<PathBuf>, timeout: Duration, kill_after: Duration }
- `run(spec)` returns struct { status, stdout (bounded capture), stderr (bounded), duration }.
- Non-blocking using tokio::process.
- Enforce stdout/stderr capture limits; stream stderr lines into tracing as they arrive.
- On timeout, send graceful kill; after kill_after, force kill; collect outcome.

Tests:
- Unit test with a short sleep program; test timeout path; test env/cwd application; test large output truncated.

Wire-up:
- Expose metrics: command_success_total, command_timeout_total, command_duration_seconds histogram.
```

---

### Prompt 08 — Capture Abstractions + FileSource (Test-Only) ✅ COMPLETED

```text
Create `gl_capture`:

Implement:
- Trait `CaptureSource` with async methods: `start(&self) -> CaptureHandle`, `snapshot()` -> Bytes, `stop()`.
- `FileSource` implementation that reads frames from a local mp4 and can produce JPEG snapshots (use `ffmpeg` via `gl_proc` or pure `image` crate if simpler for tests).

Tests:
- Integration test uses `FileSource` to produce a snapshot from a sample mp4 in test fixtures.
- Ensure resources are cleaned up (drop handle stops). Use test-support temp dirs.

Wire-up:
- Add a minimal API endpoint `/api/stream/{template_id}/snapshot` that uses `FileSource` when template.kind == 'file' and returns `image/jpeg`.
```

---

### Prompt 09 — FFmpeg Capture Source ✅ COMPLETED

```text
Extend `gl_capture`:

Implement:
- `FfmpegSource` that builds ffmpeg command with hardware accel flags from Config (vaapi/cuda/qsv optional).
- Start returns a handle that owns child process; snapshot runs an ffmpeg filter to extract a frame to jpeg (or uses a ring buffer of last frames if implemented).
- Map stderr to tracing.

Tests:
- Integration tests guarded with `#[ignore]` when ffmpeg unavailable. Provide mock mode using a dummy program echoing bytes to stdout to simulate frames.
- Unit tests validate command line building from config.

Wire-up:
- Template.kind == 'ffmpeg' routes to this Source; add metrics (frames/sec if feasible, process restarts).
```

---

### Prompt 10 — MJPEG Streaming Service ✅ COMPLETED

```text
Create `gl_stream`:

Implement:
- Actix handler that serves multipart/x-mixed-replace MJPEG stream for a given template.
- Uses an async channel to receive JPEG frames from Source; backpressure-aware; drops frames under load.
- Connection lifecycle: stop stream when client disconnects; update metrics (subscribers gauge).

Tests:
- Integration test that reads N parts from the endpoint and asserts content-type boundaries.
- Backpressure test: producer faster than consumer; ensure bounded queue and drops counted.

Wire-up:
- Add `/api/stream/{template_id}/mjpeg` endpoint and connect to FileSource/FfmpegSource.
```

---

### Prompt 11 — Scheduler (`gl_sched`) With Cron Jobs ✅ COMPLETED

```text
Create `gl_sched`:

Implement:
- Job model: id, kind, schedule (cron), last_run, next_run, jitter_ms.
- Runner uses `tokio-cron-scheduler`; on trigger, writes a row to `jobs`, starts capture or snapshot job depending on template config.
- Ensure idempotency: a job doesn’t run if a prior instance still active (row status check).

Tests:
- Unit tests for cron parsing and next_run calc; integration test with a fast schedule that increments a counter.
- DB test ensures status transitions: pending -> running -> success/failure.

Wire-up:
- Admin endpoints to list jobs, trigger now, pause/resume per job.
```

---

### Prompt 12 — Notifications (`gl_notify`) Adapters ✅ COMPLETED

```text
Create `gl_notify`:

Implement:
- Trait `Notifier { async fn send(&self, msg: Notification) -> Result<()> }` where Notification contains kind, title, body, channels, attachments (URIs).
- Adapters: SMTP (lettre), Twilio SMS (reqwest), Webhook (POST JSON), WebPush (web-push).
- Retry with exponential backoff, jitter; classify retryable vs terminal errors; small in-memory circuit breaker.

Tests:
- Unit tests with mock HTTP server (wiremock) for Twilio/Webhooks success/failure/retry.
- Integration test proving multiple channels are invoked and failures are aggregated.

Wire-up:
- Add `/api/alerts/test` endpoint that triggers a sample notification; RBAC-protected.
```

---

### Prompt 13 — CAP Alerts (`gl_cap`) ✅ COMPLETED

```text
Create `gl_cap`:

Implement:
- Builder for CAP 1.2 messages using `quick-xml`; structs with serde for marshal/unmarshal.
- Profiles for common use (e.g., "Severe Weather Alert"), with validation helpers (required fields, ISO timestamps).
- Optional XSD validation if feasible (feature gate).

Tests:
- Golden-file test: render a CAP example and diff against sample; parse-then-render roundtrip equals normalized canonical form.

Wire-up:
- `gl_notify` can include CAP XML as attachment or body; add endpoint to preview CAP XML for a given template/event.
```

---

### Prompt 14 — Web Templates & Admin CRUD ✅ COMPLETED

```text
Enhance `gl_web`:

Implement:
- Full CRUD for templates: list/create/update/delete; JSON schema validation for `config_json` per kind.
- RBAC: admin only for write operations; operator for read.
- Pagination, filtering; ETag support.

Tests:
- Integration tests for CRUD, optimistic concurrency (If-Match/ETag).
- JSON schema validation tests.

Wire-up:
- `/api/templates` drives everything: capture/stream/schedule consult this table.
```

---

### Prompt 15 — Website Capture via Selenium (`thirtyfour`) ✅ COMPLETED

```text
Extend `gl_capture`:

Implement:
- `WebsiteSource` using `thirtyfour` (async). Supports headless, basic auth, dedicated selectors, stealth flag.
- Snapshot = DOM screenshot; optional element-specific screenshot if selector present.

Tests:
- Abstract `WebDriverClient` behind a trait; provide a mock that returns a synthetic PNG for CI.
- Integration tests behind feature `website_live` to hit real Selenium Grid if env vars present.

Wire-up:
- Template.kind == 'website' uses WebsiteSource; add `/api/stream/{template_id}/snapshot` for websites too.
```

---

### Prompt 16 — yt-dlp Capture ✅ COMPLETED

```text
Extend `gl_capture`:

Implement:
- `YtDlpSource` that shells out to `yt-dlp` to fetch/pipe video; similar to ffmpeg source.
- Handle live stream URLs and VOD; snapshot via ffmpeg frame extraction.

Tests:
- Command-line building tests; mock program to simulate output; timeout handling.

Wire-up:
- Template.kind == 'yt'; register in source factory.
```

---

### Prompt 17 — Analysis: AI Client Stubs & Online Impl ✅

```text
Create `gl_ai`:

Implement:
- `AiClient` trait: `summarize(text)`, `describe_frame(jpeg_bytes)`, `classify_event(...)`.
- `StubClient` (returns canned responses), `OpenAiClient` via reqwest; auth header; timeout; retry.

Tests:
- Unit tests for request serialization; mock server for OpenAI paths.
- Ensure no network calls in CI default (use stub; online behind feature `ai_online`).

Wire-up:
- Config toggles which client to use; default stub in dev/test.
```

---

### Prompt 18 — Vision: Motion Detection (OpenCV Optional) ✅

```text
Create `gl_vision`:

Implement:
- Fallback pure-Rust pixel-diff motion detector on downscaled grayscale frames with configurable threshold.
- Optional OpenCV MOG2 implementation behind `heavy_opencv` feature; choose at runtime based on feature + config.

Tests:
- Unit tests with synthetic frame pairs where motion is known.
- Bench test (criterion) behind feature to compare CPU cost.

Wire-up:
- `gl_capture` can pipe frames through `gl_vision` to raise “motion” events; publish to DB `events` and trigger notifications by rule.
```

---

### Prompt 19 — Analysis Pipeline & Rules ✅

```text
Implement in a new `gl_analysis` module (can live within `gl_capture` or standalone):

- Define `Processor` trait (input: frame/text + context; output: AnalysisEvent).
- Compose processors: motion -> ai.describe_frame -> summarizer.
- Rule engine: YAML/JSON rules attached to template (thresholds, quiet hours, dedupe windows).
- Emit `events` rows and enqueue notifications.

Tests:
- Unit tests for rule evaluation with time windows.
- Integration test simulating a motion burst that dedupes alerts.

Wire-up:
- Admin endpoint to test rules against a sample event.
```

---

### Prompt 20 — Streaming: RTSP (Feature-Gated) ✅

```text
Add RTSP support in `gl_stream` (feature `rtsp`):

- Use `gstreamer` + `gstreamer-rtsp-server` via `gstreamer-rs` (feature).
- Wrap Source frames into RTP pipeline; expose `rtsp://host:port/{template}`.

Tests:
- Smoke test behind feature; skipped in CI by default.

Wire-up:
- Config toggles RTSP on/off; document requirements.
```

---

### Prompt 21 — Notifications: End-to-End Alert Flow ✅ COMPLETED

```text
Glue pieces:

- When `events` row created at level >= configured threshold, produce a `PendingAlert`.
- `gl_notify` consumes and sends over configured channels.
- Store alert delivery results in `alerts` table with per-channel statuses.

Tests:
- Integration test: create an event; verify alert rows and mock channel hits.
- Failure path retries then dead-letter (keep a DLQ table or status=failed with error detail).
```

---

### Prompt 22 — PWA Static Serving + CSP ✅ COMPLETED

```text
In `gl_web`:

- Static file service for PWA assets from a configurable directory; strong caching (ETag, Cache-Control), fallback to index.html.
- Strict CSP with nonces for inline scripts if needed.

Tests:
- Integration tests for ETag revalidation and gzip/brotli (if enabled).

Wire-up:
- `/` serves the PWA; API remains under `/api`.
```

---

### Prompt 23 — Auto-Update (Sidecar or Self-Replace) ✅ COMPLETED

```text
Create `gl_update`:

- Strategy A (safer): sidecar process `gl-update` checks GitHub releases (JSON), verifies signature (ed25519), downloads, swaps symlink atomically, signals main to restart.
- Strategy B (self-replace, feature-gated).
- Rollback on failed health check after N seconds.

Tests:
- Unit tests for signature verify and atomic swap on tmpfs.
- Integration test with mock release server and a fake tarball.

Wire-up:
- Admin endpoint to “check now” and “apply update”; protected by RBAC.
```

---

### Prompt 24 — Rate Limiting, Body Limits, Input Validation ✅ COMPLETED

```text
Hardening in `gl_web`:

- Add actix-ratelimit for IP + API key buckets.
- Global JSON body size limit and per-endpoint overrides.
- Validation errors return RFC 7807 Problem Details (content-type application/problem+json).

Tests:
- Rate limit test with burst traffic.
- Oversized body returns 413.
- Invalid payload returns structured errors.
```

---

### Prompt 25 — Object Storage Abstraction

```text
Add storage abstraction:

- Use `object_store` crate with backends (S3, local fs). Store artifacts from captures and snapshots.
- URIs like `s3://bucket/path.jpg` or `file:///var/data/...`.
- Streaming upload with retries and MD5/etag validation.

Tests:
- Unit tests for URI parsing and write/read roundtrip (local fs).
- Mock S3 in tests with a local server (minio) behind feature.

Wire-up:
- Capture artifacts now stored via Storage; DB stores URIs.
```

---

### Prompt 26 — E2E Test Harness

```text
Create `e2e` tests:

- Spin up: app (HTTP), SQLite temp DB, synthetic FileSource templates, mock Twilio/OpenAI, observability server.
- Scenario: create template -> schedule job -> produce event -> send notification -> fetch metrics -> assert invariants.

Tools:
- Use `testcontainers` (optional) for Selenium Grid when website feature is enabled.

Deliverables:
- A single `cargo test --package app --test e2e_smoke` that runs end-to-end.
```

---

### Prompt 27 — OpenAPI Parity Guard

```text
Add a test that:
- Generates OpenAPI spec via utoipa at runtime.
- Compares against a golden file checked into repo; diff must be approved to avoid breaking the frontend contract.

Include:
- Script to update golden when intentional changes occur.
```

---

### Prompt 28 — Security & Supply Chain Gates

```text
Strengthen CI:

- `cargo audit` fail on vulnerable crates.
- `cargo deny` policies: multiple versions, yanked crates, unknown licenses blocked.
- Add `gitleaks` for secret scanning; pre-commit hook.

Deliverables:
- CI workflow updates; a test PR should fail if a secret-like string appears in code.
```

---

### Prompt 29 — Dashboards & Runbooks

```text
Add docs/ with:
- Grafana dashboards JSON for request rates, job durations, alert failures, ffmpeg restarts.
- Runbooks: incident steps for “no snapshots”, “alert storm”, “update rollback”.

Wire-up:
- Link dashboards from `/docs` page.
```

---

### Prompt 30 — Release Packaging

```text
Finalize packaging:

- Multi-stage Dockerfile: build (with sccache), then distroless runtime; pass in UID/GID; set read-only FS; mount /data for DB/artifacts.
- SBOM generation (syft) in CI; attach to releases.
- systemd unit with sandboxing (ProtectSystem=strict, PrivateTmp, NoNewPrivileges).

Deliverables:
- `docker build` works; container starts and serves health endpoints.
```

---

## 5) Right-Sizing Review (Why this is safe & forward-moving)

* **Each prompt compiles and tests** something real; no future promises, no dangling code.
* **Heavy/unstable deps are feature-gated** (OpenCV, RTSP, live Selenium, online AI).
* **Networked integrations are mockable** so CI is deterministic.
* **Observability exists from day one**, easing triage and performance work later.
* **Security gates block regression** (deny/audit/gitleaks).
* **OpenAPI golden test** prevents frontend breakage.
* **E2E harness** validates real workflows end-to-end before shipping.

If you want an even finer grain for the earliest prompts (A1–B1), you can split them further (e.g., Prompt 01a: workspace only; 01b: CI; 01c: Justfile). But for most teams, the 30 prompts above are the sweet spot: small enough to be safe, big enough to matter.

---

## 6) Bonus: Minimal Acceptance Checklist (per PR)

* [ ] `cargo fmt` + `clippy` (no warnings)
* [ ] Unit tests added & passing
* [ ] Integration/E2E updated if behavior changed
* [ ] Metrics/logging added for new paths
* [ ] Public API documented and appears in OpenAPI
* [ ] Security review: inputs validated, secrets not logged
* [ ] Migration is backward-compatible or includes upgrade notes
* [ ] Docs updated (README, runbooks, dashboards)
