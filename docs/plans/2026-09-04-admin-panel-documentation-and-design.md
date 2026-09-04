# Admin Panel: Documentation And Design-First Plan

> **For Hermes:** Implement this plan task-by-task. First complete and review
> the documentation; do not create frontend implementation before the user
> explicitly approves the UI wireframes.

**Goal:** Establish `FerrPOINT/admin-panel` as a standalone SDLC control-plane
application that centrally configures branding, a service integration registry,
and supported platform settings for all fleet applications.

**Architecture:** Admin Panel owns its own Rust/Axum backend, React frontend,
and PostgreSQL database. It validates central ES256/JWKS authentication but does
not own identity or tokens. Each registered product exposes an explicit,
versioned integration capability contract; Admin Panel manages only declared
platform settings, never arbitrary remote administration. Consumers keep safe
built-in defaults and fetch public runtime configuration via a direct,
ETag-versioned API with a 60-second TTL.

**Tech Stack:** Rust 1.88 / Axum / SQLx / PostgreSQL 17; React 19 / Vite /
Tailwind 4; `@sdlc/ui`; central auth JWKS; Docker Compose umbrella ports
`7771` API, `7772` web, `7773` PostgreSQL.

---

## Constraints And Decisions

- Repository: `https://github.com/FerrPOINT/admin-panel`; source checkout:
  `/opt/dev/sdlc/admin-panel`.
- This is a first-class fleet application, not a module inside auth-server or a
  sidecar database.
- `auth-server` is identity, roles, signed tokens and JWKS only. It never stores
  branding, service catalog, config revisions or admin-panel audit data.
- Service integration is opt-in and declarative. A service registration contains
  public UI URL, an authenticated integration base URL, declared capabilities,
  health endpoint, status and version. There is no generic proxy or arbitrary
  remote write endpoint.
- Runtime config is public-safe metadata only: title/logo/colors/radius and
  enabled service navigation. No credentials, health diagnostics, upstream
  addresses, permission maps or private configuration can escape it.
- Consumers retain immutable local CSS defaults. Admin Panel downtime must not
  prevent a product frontend from rendering or operating.
- V1 has no CDN, proxy gateway, CSS asset publisher or SSE. It uses direct
  cross-origin `GET /api/v1/runtime-config`, strict CORS, `ETag`,
  `If-None-Match`, client-side 60-second revalidation, and optional last-valid
  local storage snapshot.
- Service catalog links use configured public frontend URLs in local v1; future
  same-origin gateway aliases are deliberately deferred.

## Deliverables

1. Complete documentation parity and governance artifacts in `admin-panel`.
2. Accepted UI page map and wireframes in `docs/UI_UX.md`.
3. A committed architecture/implementation plan that can be executed after UI
   approval.
4. No app code, Compose changes, migrations, or frontend implementation before
   approval of the documented screens.

## Page Inventory Requiring Approval

| Route | Purpose | Authoritative data |
|---|---|---|
| `/` | Operational overview: current revision, registered services, integration state, recent config changes | Admin Panel API |
| `/branding` | Edit title/logo/colors/radius with live preview and optimistic revision save | `platform_config` |
| `/services` | Register, order, enable and inspect product integrations | `platform_services` |
| `/services/:key` | Capability contract, endpoint validation, compatibility and health history | service registry + observations |
| `/revisions` | View immutable published config revisions; compare and roll back by creating a new revision | `config_revisions` + audit |
| `/audit` | Filtered audit trail of actor, action, before/after revision and result | audit log |
| `/runtime` | Read-only runtime manifest inspector, headers/ETag and consumer defaults contract | runtime endpoint |
| `/settings` | Admin Panel local operational settings only; no duplicate product settings | Admin Panel API |
| `/login`, `/register` | Existing central-auth entry flow, if product-local routes are required | central auth |

## Documentation Tasks

### Task 1: Baseline and Documentation Tree

**Files:** root governance files; `docs/`; `docs/adr/`.

1. Clone and inspect the empty explicit GitHub repository.
2. Use task-tracker documentation as a coverage reference, not copied domain
   content.
3. Define the new application's ownership, API boundary, data model, runtime,
   security, cache, deployment and UI contracts.
4. Check every document against the decisions above; remove statements that
   imply auth-server ownership, CDN, a gateway prerequisite or arbitrary remote
   service control.
5. Commit the documentation set only after cross-links and terminology checks.

### Task 2: UI Approval Gate

**Files:** `docs/UI_UX.md`, this plan.

1. Verify every page has an exact purpose, data source, empty/loading/error
   state and role boundary.
2. Present wireframes to the user and wait for explicit approval.
3. Do not create React page components, CSS, mocks or screenshots before that
   approval.

### Task 3: Backend Implementation After Approval

**Files:** `backend/`, `openapi/`, tests, migrations.

1. Write a failing test for each contract first.
2. Create typed domain models and immutable SQLx migrations for configuration,
   services, revisions, audit and health observations.
3. Implement public runtime read API with CORS allowlist, ETag/304 and strict
   response projection.
4. Implement administrator write APIs with central ES256 validation, role
   enforcement, `If-Match` conflict handling, audit rows and atomic revision
   publish.
5. Add a narrow service capability client that can test only declared endpoint
   operations; credentials remain encrypted/secret-backed and never appear in
   runtime JSON or logs.
6. Generate and commit the OpenAPI specification; run focused then full tests.

### Task 4: Frontend Implementation After Approval

**Files:** `frontend/`, frontend tests.

1. Build the approved routes using existing SDLC patterns and `@sdlc/ui`.
2. Implement real loading/empty/error states; do not add mock KPIs or synthetic
   records.
3. Use revision-aware saves: display a conflict state when server returns 412.
4. Add branded preview using the same CSS token application contract consumers
   use.
5. Test desktop and mobile service catalog, branding preview, revision conflict,
   capability state and audit filtering.

### Task 5: Fleet Integration

**Files:** `services-base/frontend`, four existing product frontends, umbrella
Compose, each product configuration.

1. Add `PlatformProvider` and `ServiceSwitcher` to `@sdlc/ui` with defaults.
2. Add direct runtime-config URL to consumer config; keep each consumer default
   working when it is unset/unreachable.
3. Register CI-CD, Task Tracker, Wiki and Fleet Control with minimal declared
   capabilities. Do not turn products into mutually dependent control planes.
4. Add each consumer's CORS origin to Admin Panel configuration and test 304,
   failure fallback, catalog navigation and no-secret response projection.

### Task 6: Proof and Delivery

1. Full Rust tests, lint, format, dependency checks and OpenAPI drift gate.
2. Frontend typecheck, lint, unit tests, build, responsive browser tests.
3. Local Compose health checks on `7771`, `7772`, `7773`; CORS/ETag/304 smoke.
4. Deterministic evidence seed, desktop `1920x1080` and mobile `375x812`
   screenshots in `docs/screenshots/` plus README references.
5. Commit and push only after the repository is internally clean and all checks
   are real and green.
