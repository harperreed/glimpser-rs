Admin CRUD Audit — Templates, Users, API Keys

Summary

- Scope: Audit why CRUD routes are not working for templates, users, and API keys exposed under `/api/settings`.
- Root causes found:
  - API keys repository methods `list_all`, `delete`, and related are stubbed (no real DB ops).
  - Admin create handlers for templates and API keys hardcode `user_id = "admin"`, violating FK when no such user exists.
  - Admin template update uses POST instead of PUT.
  - Admin template payload expects `config` as string (JSON-as-text) while other template APIs accept JSON objects.
  - Everything is mounted under `/api/settings` with JWT auth; routing is wired correctly.

Routing Topology (Backend)

- File: `gl_web/src/lib.rs`
  - `/api/auth/*`: login
  - `/api/stream/*`: snapshot/stream actions
  - `/api/*`: public endpoints (`/me`, `/streams`, `/alerts`, `/health`)
  - `/api/templates/*`: user templates CRUD
    - `GET /` `GET /{id}` `POST /` `PUT /{id}` `DELETE /{id}` (uses handlers in `gl_web/src/routes/templates.rs`)
  - `/api/settings/*`: admin panel endpoints (focus of this audit)
    - Templates: `GET /templates`, `GET /templates/{id}`, `POST /templates`, `POST /templates/{id}` (should be PUT), `DELETE /templates/{id}`
    - Users: `GET /users`, `GET /users/{id}`, `POST /users`, `DELETE /users/{id}`
    - API Keys: `GET /api-keys`, `POST /api-keys`, `DELETE /api-keys/{id}`
  - Middleware: All `/api/settings` routes are wrapped in `RequireAuth` (JWT) and rate limiting.

Findings

1) API Keys CRUD is effectively disabled

- File: `gl_db/src/repositories/api_keys.rs`
  - `list_all`: returns `Ok(vec![])` (always empty). Comment indicates intent to remove API key system.
  - `delete`: returns `Ok(())` without DB update (no-op).
  - `update_last_used`: no-op.
  - Impact:
    - `GET /api/settings/api-keys` always returns an empty list.
    - `DELETE /api/settings/api-keys/{id}` appears to succeed but doesn’t change the DB.
- Create API key specifics (admin route):
  - File: `gl_web/src/routes/admin.rs`, `create_api_key`
    - Hardcodes `user_id: "admin"` → will violate FK unless a user with id="admin" exists.
    - Sets `permissions: "read,write"` as CSV string, but the field is documented as a JSON array string.

2) Templates (admin) — hardcoded user and method mismatch

- File: `gl_web/src/routes/admin.rs`
  - `create_template`: uses `user_id: "admin"` (FK violation unless user exists).
  - `update_template`: annotated with `#[post("/templates/{id}")]` (should be PUT/PATCH). Inconsistent with `/api/templates` routes which use PUT.
  - Request body types:
    - `CreateTemplateRequestBody` and `UpdateTemplateRequestBody` take `config: String` (expects JSON-as-string) and validate by parsing.
    - Elsewhere (`gl_web/src/routes/templates.rs`), template APIs accept `serde_json::Value` for config. Admin UI would likely send JSON objects.

3) Users (admin)

- File: `gl_web/src/routes/admin.rs`
  - `list_users` uses `UserRepository::list_active()` — OK.
  - `create_user` hashes passwords via Argon2 — OK.
  - `delete_user` calls `UserRepository::delete` which soft-deletes (sets `is_active=false`) — OK.
  - Potential runtime pitfall: timestamps are parsed with `parse_from_rfc3339(...).unwrap()`. Current code writes ISO8601 via `now_iso8601()`, so safe under normal operation.

4) Data model and migrations alignment

- Templates table has `FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE` (file: `gl_db/migrations/003_create_templates_table.sql`).
- Therefore, inserting with a non-existent `user_id` (e.g., "admin") fails.
- Similar FK exists for API keys (`gl_db/migrations/002_create_api_keys_table.sql`).

Concrete Remediations

API Keys Repository (enable real behavior)

- Implement `list_all(limit, offset)` to return active keys:
  - SQL: `SELECT * FROM api_keys WHERE is_active = true ORDER BY created_at DESC LIMIT ?1 OFFSET ?2`.
- Implement `delete(id)` as soft-delete (consistent with users):
  - SQL: `UPDATE api_keys SET is_active = false, updated_at = ?1 WHERE id = ?2`.
- Keep `update_last_used` optional; if kept, set `last_used_at = now`.

Admin Handlers — Stop hardcoding user_id

- Use authenticated user from JWT:
  - `let user = crate::middleware::auth::get_http_auth_user(&req).ok_or(...)`.
  - `user_id: user.id` for both `create_template` and `create_api_key`.
- Normalize API key permissions representation:
  - Store as JSON array string, e.g. `permissions: serde_json::to_string(&["read", "write"])`.

HTTP Methods and Payload Shape

- Change admin template update to PUT for consistency:
  - Update attribute: `#[put("/templates/{id}")]` in `gl_web/src/routes/admin.rs`.
- Consider aligning admin template request/response shape with `/api/templates`:
  - Accept `config: serde_json::Value` in request; validate, then serialize to String for DB.
  - This removes ambiguity and prevents clients from having to stringify JSON.

Verification Plan

1) API keys
   - Create a user and authenticate to get JWT.
   - POST `/api/settings/api-keys` → expect 201; then GET `/api/settings/api-keys` includes the new key.
   - DELETE `/api/settings/api-keys/{id}` → expect 204; GET no longer lists it (or `is_active=false`).

2) Templates (admin)
   - POST `/api/settings/templates` with valid JSON config (as object if payload updated) → expect 201; GET lists it.
   - PUT `/api/settings/templates/{id}` → expect 200 with updates applied.
   - DELETE `/api/settings/templates/{id}` → expect 204 and gone from list.

3) Users (admin)
   - POST `/api/settings/users` → expect 201; GET lists it; DELETE marks inactive and removes from `list_active`.

Observed Frontend Expectations

- File: `frontend/src/app/admin/page.tsx`
  - Lists use:
    - `/api/settings/users`
    - `/api/settings/api-keys`
    - `/api/settings/templates`
  - Deletes use corresponding DELETE routes.
  - No create/edit UI yet for templates/users/api-keys; once added, align payloads with backend changes above.

Why things “weren’t working”

- API keys list and delete never worked because the repository is stubbed. Create also fails if there is no user with id `"admin"`.
- Admin template create fails if there is no user with id `"admin"`. Update requires POST instead of PUT, surprising clients.
- Payload shape mismatch on admin template endpoints can break clients that send JSON objects rather than strings.

Quick Patch Checklist

- [ ] Implement API keys `list_all` and `delete` in `gl_db/src/repositories/api_keys.rs`.
- [ ] In `gl_web/src/routes/admin.rs`:
  - [ ] Use JWT user id instead of hardcoded `"admin"` in `create_template` and `create_api_key`.
  - [ ] Encode permissions as JSON array string.
  - [ ] Change `update_template` to `#[put("/templates/{id}")]`.
  - [ ] (Optional) Switch `config` fields to `serde_json::Value` and serialize internally.
- [ ] Add minimal integration tests for `/api/settings` happy paths.

Notes

- Routing and middleware are correctly mounted; the issues stem from repository stubs and data/HTTP shape inconsistencies.
- Timestamps parse with `.unwrap()` assume ISO8601 in DB; current writers satisfy this.
