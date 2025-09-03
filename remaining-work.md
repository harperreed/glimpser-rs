Remaining Work — Admin CRUD and Templates API

Overview

This document lists follow-ups to fully stabilize admin CRUD (templates, users, API keys) and align API/UX.

High‑priority

- API keys repository (gl_db/src/repositories/api_keys.rs)
  - Implement list_all(limit, offset): SELECT active keys ordered by created_at (limit/offset).
  - Implement delete(id): soft delete (is_active=false, updated_at=now) for consistency with users.
  - (Optional) Implement update_last_used(id) to record API key usage.

- Admin create handlers (gl_web/src/routes/admin.rs)
  - Use JWT user id instead of hardcoded "admin" for create_template and create_api_key.
    - user_id = get_http_auth_user(&req).id
  - Encode permissions as a JSON array string (e.g., ["read","write"]).
  - Switch update_template to PUT (#[put("/templates/{id}")]) to match REST semantics and user-facing routes.

- Admin templates payload shape
  - Accept config as serde_json::Value (not String) for create/update; validate Value and serialize to String for DB.
  - Aligns /api/settings/templates with /api/templates expectations.

- Routing consistency
  - Decide on global trailing slash policy (prefer no trailing slash) and enable NormalizePath accordingly.
  - Remove temporary route aliases/logging after verification.

Medium‑priority

- Tests
  - Add integration tests for /api/settings CRUD happy paths (templates, users, API keys).
  - Add test asserting both /api/templates and /api/templates/ resolve (or enforce one canonical form).

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
