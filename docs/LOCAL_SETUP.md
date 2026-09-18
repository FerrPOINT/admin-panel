# Локальный запуск

## Назначение

Этот документ описывает текущую локальную топологию Admin Panel. Репозиторий содержит Rust API, React frontend и PostgreSQL; central auth на `7701` остаётся внешней зависимостью identity/JWKS.

## Топология

| Компонент | Repository Compose | Base umbrella | Роль |
|---|---|---|---|
| API Admin Panel | `http://127.0.0.1:7771` | `http://127.0.0.1:7771` | Admin API, runtime APIs, health |
| Frontend Admin Panel | `http://127.0.0.1:7772` | `http://127.0.0.1:7772` | Administrative web UI |
| PostgreSQL | `127.0.0.1:7773` | `127.0.0.1:7773` | Admin-owned persistence |
| Central auth | `http://127.0.0.1:7701` | `http://127.0.0.1:7701` | External identity, ES256 issuer and JWKS |

PostgreSQL is loopback-bound. Do not expose it on a public interface merely to make browser testing easier.

## Prerequisites

- Docker Engine with Compose v2.
- A local checkout of `services-base` adjacent to `admin-panel`; backend and frontend use its shared contracts.
- Private deployment values for database connection and central-auth integration. They belong in an ignored environment file or deployment secret store, never in Git.
- A central auth server when testing authenticated flows. Public readiness and runtime endpoints can still be smoke-tested without a browser login.

## Repository Compose

1. Create a local `.env` from `.env.example` and replace example-only values outside Git.
2. Start the owned stack:

```bash
docker compose -f docker-compose.dev.yml up -d --build
```

3. Verify containers and endpoints:

```bash
docker compose -f docker-compose.dev.yml ps
curl --fail http://127.0.0.1:7771/health/live
curl --fail http://127.0.0.1:7771/health/ready
curl --fail http://127.0.0.1:7772/
```

The development web container proxies `/api/v1/` to `admin-api:7771`; browser requests therefore stay same-origin at `7772`.

## Base Umbrella

From the Base workspace root, start only the Admin Panel application services:

```bash
docker compose -f docker-compose.local.yml up -d --build admin-api admin-web
curl --fail http://127.0.0.1:7771/health/ready
curl --fail http://127.0.0.1:7772/
```

The umbrella injects central JWKS configuration over the private auth network. It does not make Admin Panel responsible for central credentials or signing keys.

## Environment Contract

| Variable | Purpose | Sensitivity |
|---|---|---|
| `ADMINP_DATABASE_URL` | Admin-owned PostgreSQL DSN | Secret |
| `ADMINP_BIND_ADDRESS`, `ADMINP_BIND_PORT` | API listener | Non-secret |
| `ADMINP_MIGRATIONS_DIR` | SQLx migration path in the API image | Non-secret |
| `ADMINP_CORS_ALLOWED_ORIGINS` | Explicit consumer-origin allowlist | Deployment policy |
| `ADMINP_AUTH_JWKS_URI` | Trusted central-auth public JWKS URI | Deployment policy |
| `ADMINP_AUTH_ISSUER` | Expected central access-token issuer | Deployment policy |
| `ADMINP_AUTH_AUDIENCE` | Expected token audience; default `sdlc` | Deployment policy |
| `VITE_API_BASE_URL` | Optional frontend API base URL | Non-secret build setting |
| `VITE_PLATFORM_BRANDING_URL` | Runtime branding source for the shared provider | Non-secret build setting |
| `VITE_PLATFORM_SERVICES_URL` | Runtime catalog source for the shared provider | Non-secret build setting |

The application reads `ADMINP_AUTH_JWKS_URI` and `ADMINP_AUTH_ISSUER` at the server boundary. `ADMINP_AUTH__CENTRAL_*` values are used by the shared central-auth bridge configuration where supplied by deployment; keep both naming surfaces consistent with the selected deployment contract rather than inventing a third variable family.

## Smoke Matrix

| Scenario | Expected result |
|---|---|
| API liveness | `GET /health/live` returns `200`. |
| API readiness | `GET /health/ready` returns `200` only after owned DB and migrations are ready. |
| Web shell | `GET /` on `7772` returns the SPA shell. |
| Runtime branding | Published branding returns ETag and cache policy; no published revision returns a documented safe error. |
| Runtime services | Only active services with approved declaration appear. |
| Auth | Invalid/missing bearer fails closed; unknown role maps to viewer. |
| Role binding | Only panel admin can create/delete local bindings. |

## Troubleshooting

| Symptom | Check | Safe action |
|---|---|---|
| `7771` or `7772` occupied | Identify the owning local container/process | Do not stop a process you do not own; resolve the intended Compose stack first. |
| Readiness fails | `docker compose ... ps`, migration logs and Admin PostgreSQL health | Repair the owned dependency; do not disable readiness. |
| `401` / `403` | Token issuer/audience, JWKS reachability and effective panel role | Fix central-auth configuration or binding policy; never bypass middleware in UI. |
| Runtime branding unavailable | Published revision and API health | Consumer keeps local defaults; do not make frontend startup depend on the runtime endpoint. |
| Integration check fails | Approved declaration state and allowlisted capability | Inspect registry metadata; do not add arbitrary remote URLs or credentials. |

## Cleanup

Stop only the stack you started and retain volumes unless an intentional local-data reset is required:

```bash
docker compose -f docker-compose.dev.yml down
```

Removing volumes deletes local Admin Panel state. Confirm the target is development-only before running any volume-destructive command.
