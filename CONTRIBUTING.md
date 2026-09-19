# Contributing — Admin Panel

## 1. Getting Started

```bash
git clone git@github.com:FerrPOINT/admin-panel.git
cd admin-panel
cp .env.example .env 2>/dev/null || true
```

## 2. Development Setup

Backend (через rust-контейнер, пин флота 1.86/1.88 по rust-version workspace):

```bash
docker run --rm -v "$PWD/backend":/b -w /b rust:1.88-slim-bookworm cargo test
```

Frontend:

```bash
cd frontend
pnpm install
pnpm test
```

Локальный стенд: `docker compose -f docker-compose.dev.yml up --build -d`
(API `7771`, web `7772`, PostgreSQL `7773`).

## 3. Гейты перед PR

- Backend: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test`.
- Frontend: typecheck, tests, lint, build.
- Docs: `python3 scripts/verify_readme.py`.
- CI: лёгкий пайп docs + backend + frontend (см. Base CI_CONVENTION).

## 4. Соглашения

- Контракт первым: OpenAPI-схема из Rust-хендлеров; CI ловит drift.
- AuthZ fail-closed: мутации требуют principal; роли — от central auth.
- Секреты не коммитятся; в примерах и логах — только `[REDACTED]`.
- Доки на русском, код и комментарии на английском.
- Каждый существенный объём работ — план в `docs/plans/` + CHANGELOG-запись.

## 5. Сообщения о безопасности

Только приватно — см. [SECURITY.md](SECURITY.md).
