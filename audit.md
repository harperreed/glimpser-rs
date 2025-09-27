# Glimpser Audit

This document summarizes security and performance findings and proposes concrete code-level improvements.

## Top Takeaways

- Strongly parameterize password hashing and use config values.
- Escape any user/content-derived strings rendered into HTML.
- Unify static asset serving and eliminate blocking I/O in Axum.
- Make rate limiting proxy-aware and resilient under load.
- Add cache headers/ETags to image endpoints; avoid chattiness.
- Tighten JWT validation settings and cookie attributes in production.
- Keep SQLite tuned (you did most right); add a few minor tweaks.
- Reduce noisy logging on hot paths (static, polling, streaming).
- Prefer template rendering over string concatenation for views.
- Make rate limiting distributable (Redis) if you scale horizontally.

---

## Security

### Password hashing

- Files: `gl_web/src/auth.rs`, `gl_config/src/lib.rs` (SecurityConfig)
- Issue: Argon2 is called as `Argon2::default()` and ignores configured parameters.
- Risks: Unintended drift from desired memory/time cost; hard to tune.
- Action:
  - Build `argon2::Params` from `SecurityConfig.argon2_params` and instantiate
    `Argon2::new(Algorithm::Argon2id, Version::V0x13, params)`.
  - Enforce reasonable floors: memory >= 19 MiB, time >= 2, parallelism >= 1.

### JWT validation

- Files: `gl_web/src/auth.rs` (JwtAuth), usages in middleware and frontend.
- Issue: `Validation::default()` is used; algorithms/issuer not pinned.
- Risks: Misconfiguration acceptance; weak validation assumptions.
- Action:
  - Use `Validation { algorithms: vec![Algorithm::HS256], validate_exp: true, leeway: 30, iss: Some("glimpser".into()), .. }`.
  - Add `iss` to `Claims` and set in token creation.

### Cookies

- Files: `gl_web/src/routes/auth.rs`, `gl_web/src/frontend.rs`, `gl_config/src/lib.rs`.
- Status: `HttpOnly`, `SameSite=Lax`, `Secure` via config; good.
- Action: Default `SameSite=Strict` for UI flows unless cross-site needed; ensure `GLIMPSER_PRODUCTION=1` in prod to auto-enable secure cookies.

### XSS prevention in server-rendered HTML

- File: `gl_web/src/frontend.rs`.
- Issue: Large HTML strings constructed with `format!` inject values like `s.name` without escaping.
- Action: Prefer Askama templates (already present) for auto-escaping; otherwise, centralize an HTML-escape helper and apply to all interpolations in streams/admin pages.

### Rate limit trust of proxy headers

- File: `gl_web/src/middleware/ratelimit.rs`.
- Issue: Trusts `X-Forwarded-For` and `X-Real-IP` unconditionally.
- Risk: Header spoofing bypasses rate limiting.
- Action: Add a `trusted_proxies` config; only honor forwarded headers when request originates from a trusted proxy. Otherwise, use `peer_addr`.

### CSP tightening

- File: `gl_web/src/routes/static_files.rs`.
- Status: CSP present; allows Tailwind CDN and `'unsafe-inline'` styles.
- Action: Bundle CSS/JS locally, remove external allowances and `'unsafe-inline'`; use nonces or hashes if inline is required.

---

## Performance

### Static assets (Axum path)

- File: `gl_web/src/hybrid_server.rs`.
- Issue: `static_handler` uses blocking `std::fs::read_to_string`, returns ad-hoc responses, logs every request at info.
- Action: Replace with `tower_http::services::ServeDir`/`ServeFile`, add ETag and Cache-Control, move logs to debug.

### Prefer templates over string building

- File: `gl_web/src/frontend.rs`.
- Issue: Big pages built via `format!` are CPU-heavy, harder to cache, and error-prone for escaping.
- Action: Convert Streams, Admin pages to Askama templates; keep small HTMX fragments as templates too.

### Image endpoint caching

- File: `gl_web/src/routes/stream.rs` (`thumbnail`, `snapshot`).
- Issue: No caching headers; repeated calls reload bytes.
- Action: Add ETag (e.g., from last frame timestamp or bytes hash) and `Cache-Control` (short TTL for active streams). Respect `If-None-Match` to return 304.

### Rate limiter contention and scale

- File: `gl_web/src/middleware/ratelimit.rs`.
- Issue: Single `Mutex<HashMap<...>>` bottleneck.
- Action: Use a sharded map (e.g., N shards keyed by IP hash) or `dashmap` to reduce lock contention. For multiple instances, move to Redis token bucket.

### Logging noise

- Files: `gl_web/src/hybrid_server.rs`, `gl_web/src/routes/stream.rs`.
- Issue: Info logs on hot paths (static; frequent stream errors) flood logs.
- Action: Downgrade to debug, add sampling for repeated errors, keep warnings for actionable failures.

### SQLite tuning

- File: `gl_db/src/lib.rs`.
- Status: WAL, `synchronous=NORMAL`, `temp_store=memory`, `cache_size` set — good.
- Action: Consider `busy_timeout`, `journal_size_limit`, `mmap_size` pragmas. Ensure compound indexes are present (they are in migrations 016, 023).

### MJPEG streaming resilience

- File: `gl_web/src/routes/stream.rs` (`create_simple_mjpeg_stream`).
- Issue: Broadcast lag yields 500 on error; better to drop frames and continue.
- Action: Treat lag as debug and continue with newest frame to keep stream alive under pressure.

---

## Reliability & Correctness

### Axum/Actix hybrid consistency

- Files: `gl_web/src/lib.rs`, `gl_web/src/hybrid_server.rs`.
- Issue: Hybrid server notes "Axum only for now"; possible split behavior and duplicated logic.
- Action: Either run both stacks cleanly or consolidate. Centralize static handling to one path to avoid drift.

### Input validation coverage

- Files: `gl_web/src/frontend.rs` import handlers.
- Issue: Stream import structure is loosely validated.
- Action: Enforce strict schema and reject unknown keys to prevent runtime config errors.

### Trusted headers config

- Add a `trusted_proxies` list to configuration; use it in auth/ratelimit for header trust decisions.

---

## Build & Deploy

### Dockerfile

- File: `Dockerfile`.
- Issue: Nightly toolchain and Chromium installed in builder stage increases build time and layer size.
- Action: Use stable toolchain unless 2024 features are required; install Chromium only in runtime stage. Consider a separate worker image for capture to isolate browser/ffmpeg from API.

### Runtime hardening

- Ensure `GLIMPSER_PRODUCTION=1` (or `NODE_ENV=production`) is set to auto-enable secure cookies.
- Mount volumes read-only where possible; drop unnecessary capabilities in containers.

---

## Concrete Next Steps (Proposed Patch Set)

1) Parameterize Argon2 in `gl_web/src/auth.rs` using `SecurityConfig.argon2_params`.
2) Harden JWT validation and add issuer; update token creation/claims.
3) Replace Axum `static_handler` with `ServeDir` + caching headers; reduce log level.
4) Add an HTML escape helper and apply where `format!`-built HTML remains; migrate key pages to Askama.
5) Shard rate limiter or switch to `dashmap`; add `trusted_proxies` flag and enforcement.
6) Add ETag/Cache-Control to `thumbnail`/`snapshot` endpoints and honor `If-None-Match`.
7) Downgrade repetitive logs and add sampling for MJPEG stream errors.
8) Optional: add `busy_timeout`/`mmap_size` pragmas to SQLite connect options.

---

## Appendix: Notable Strengths

- Good separation of concerns with repositories and a cache layer in `gl_db`.
- WAL mode and compound indexes aligned with query patterns.
- JWT + HttpOnly cookie approach accommodates image fetches from the browser.
- Askama + Axum groundwork is present for safe server-rendered pages.

## Appendix: References

- Key files reviewed:
  - `gl_web/src/auth.rs`, `gl_web/src/middleware/*`, `gl_web/src/frontend.rs`, `gl_web/src/hybrid_server.rs`, `gl_web/src/routes/*`, `gl_web/src/routing/*`
  - `gl_db/src/lib.rs`, `gl_db/src/repositories/*`, `gl_db/migrations/*`
  - `gl_config/src/lib.rs`
  - `Dockerfile`, `docker-compose*.yml`
