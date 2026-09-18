# Admin Panel README Reference Implementation Plan

> **Статус 2026-09-18:** active execution plan for the Base README migration wave. This is documentation and validation work; it does not change product domain, API or runtime behavior.

**Goal:** Turn `admin-panel/README.md` into the second Base reference implementation: a source-accurate command-center entry point with an owned banner, current evidence and a CI-enforced structural validator.

**Architecture:** Keep the current Rust/Axum, React/Vite and PostgreSQL product topology untouched. Derive public claims from backend routes, Compose configuration, current screenshots and existing architecture docs. Add a repository-local standard-library README validator because a cross-repository shared package is intentionally deferred until multiple repositories prove identical requirements.

**Tech Stack:** Markdown, SVG, Python standard library `unittest`, GitHub Actions, Rust 1.88, pnpm, existing Playwright evidence.

---

## Task 1: Establish README validator with TDD

**Files:**
- Create: `scripts/tests/test_verify_readme.py`
- Create: `scripts/verify_readme.py`
- Modify: `.github/workflows/ci.yml`

1. Write focused failing tests for valid local links/images/header anchors and for missing link/image/anchor, unresolved placeholder, local filesystem path and CI badge without workflow.
2. Run `python3 -m unittest scripts.tests.test_verify_readme -v`; confirm failure because the implementation is absent.
3. Implement only `RMD001`-`RMD006` and `RMD009` from `services-base/docs/README_VALIDATION.md`.
4. Re-run focused tests; then run `python3 scripts/verify_readme.py` against the real README.
5. Add validator test and validator commands as an independent CI job. Do not weaken existing backend, frontend, Compose or E2E jobs.

## Task 2: Build owned command-center hero and source-accurate README

**Files:**
- Create: `docs/assets/admin-panel-banner.svg`
- Modify: `README.md`

1. Use an owned midnight/cobalt/electric-blue SVG hero with title, short role and accessible title/description metadata.
2. Build explicit navigation anchors and a compact proof strip, including a verified CI badge only for `.github/workflows/ci.yml` on `main`.
3. Derive capabilities and routes from `backend/api/src/lib.rs`, ports from Compose and UI routes from `frontend/src/app/router.tsx`.
4. State the ownership boundary: external central auth owns identities/tokens; Admin Panel owns branding revisions, service registry, role bindings and audit data.
5. Present selected existing desktop screenshots as full-width named evidence blocks and retain existing mobile evidence with its honest compact-table note.
6. Keep secrets out of examples. Link canonical architecture, API, security and local setup documents.

## Task 3: Repair stale control-plane documentation discovered by the fact audit

**Files:**
- Modify: `docs/API.md`
- Review: `docs/ARCHITECTURE.md`, `docs/LOCAL_SETUP.md`, `docs/ROUTING.md`, `docs/RUNTIME.md`

1. Verify that the API reference documents the current verified `user_id` role-binding boundary from the Rust middleware.
2. Regenerate `openapi/openapi.json` from `gen-openapi` and verify the CI-normalized generated JSON before release.
3. Preserve ownership boundaries and do not invent setup values or credentials.
4. Preserve current runtime/Compose docs when review finds them source-accurate.

## Task 4: Verification and release

**Commands:**

```bash
python3 -m unittest scripts.tests.test_verify_readme
python3 scripts/verify_readme.py
cd frontend && pnpm install --frozen-lockfile && pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm exec playwright test --project=chromium
# Rust: run fmt, clippy, test and regenerate/compare OpenAPI through the repository's documented container path.
docker compose -f docker-compose.dev.yml config -q
docker compose -f /opt/dev/sdlc/docker-compose.local.yml ps admin-api admin-web admin-postgres
```

Review local image paths, anchors, badge workflow target, screenshot dimensions and rendered Markdown at desktop/mobile width. Commit only related files, push `main`, then wait for hosted CI to finish green.
