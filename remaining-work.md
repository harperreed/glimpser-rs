Remaining Work — Admin CRUD and Streams API

Overview

This document lists follow-ups to fully stabilize admin CRUD (streams, users, API keys) and align API/UX.

High‑priority

- API keys repository (gl_db/src/repositories/api_keys.rs)
  - [x] Implement list_all(limit, offset): SELECT active keys ordered by created_at (limit/offset).
  - [x] Implement delete(id): soft delete (is_active=false, updated_at=now) for consistency with users.
  - [x] (Optional) Implement update_last_used(id) to record API key usage.

- Admin create handlers (gl_web/src/routes/admin.rs)
  - [x] Use JWT user id instead of hardcoded "admin" for create_stream and create_api_key.
    - user_id = get_http_auth_user(&req).id
  - [x] Encode permissions as a JSON array string (e.g., ["read","write"]).
  - [x] Switch update_stream to PUT (#[put("/streams/{id}")]) to match REST semantics and user-facing routes.

- ✅ COMPLETED: Renamed Templates → Streams (API/UI)
  - [x] Added /api/streams CRUD (templates kept for backward compatibility).
  - [x] Expose /api/settings/streams in admin; UI migrated.
  - [x] Added StreamRepository (view-backed) and migrated all routes.
  - [x] Migrated all references to use streams terminology.

- ✅ COMPLETED: Enhanced Streaming Architecture (Real-Time)
  - [x] Implemented persistent capture tasks with broadcast channels.
  - [x] Added memory-first snapshot serving (10-50ms vs 3-5s response times).
  - [x] Created /api/streams/{id}/recent-snapshots endpoint for hover animations.
  - [x] Enhanced MJPEG streaming with real-time broadcast capabilities.
  - [x] Added duration limits and graceful shutdown to capture loops.

- Admin streams payload shape
  - [x] Accept config as serde_json::Value (not String) for create/update; validate Value and serialize to String for DB.
  - [x] Aligns /api/settings/streams with /api/streams expectations.

- Routing consistency
  - [x] Decide on global trailing slash policy (prefer no trailing slash) and enable NormalizePath accordingly.
  - [x] Remove temporary route aliases/logging after verification.

Medium‑priority

- Tests
  - Add integration tests for /api/settings CRUD happy paths (streams, users, API keys).
  - Add test asserting both /api/streams and /api/streams/ resolve (or enforce one canonical form).

- Frontend (admin UI)
  - Add create/edit UI for templates/users/api keys and wire to endpoints.
  - Ensure template payloads send config as JSON object once backend accepts Value.
  - Display API key value only on creation; use key_hash thereafter.

- Error handling / serialization
  - Avoid unwrap() on timestamp parsing; handle malformed data gracefully.
  - Standardize error response format (ErrorResponse vs ProblemDetails) per endpoint category.

Low‑priority / cleanup

- Remove any remaining references to API key auth in middleware (already simplified to JWT only).
- Consider removing /api/settings POST /templates/{id} route once PUT is in place to reduce confusion.
- Review body size overrides and rate limit configs per route.

Operational checks

- Confirm backend binds to the port used by Next rewrite (default 8080).
- Verify startup route log shows expected endpoints, then remove the verbose log when stable.

Notes

- Current fixes include: supporting /api/templates without trailing slash and logging unmatched API routes.
- See auth-audit.md for detailed root-cause analysis and SQL suggestions.
