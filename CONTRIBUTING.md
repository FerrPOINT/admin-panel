# Contributing — Admin Panel

## 1. Getting Started

```bash
git clone git@github.com:FerrPOINT/admin-panel.git
cd admin-panel
cp .env.example .env 2>/dev/null || true
```

## 2. Development Setup

Backend: Rust 1.98.1 для release; отдельный MSRV gate — 1.88.0.
Проверенный Base checkout должен находиться рядом, см. [BASE_INTEGRATION](docs/BASE_INTEGRATION.md).

```bash
cd backend
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Frontend: Node 26.10.0 / pnpm 10.28.1 (из корня продукта):

```bash
cd frontend
pnpm install --frozen-lockfile
pnpm test
```

Постоянный локальный стенд запускается из корня workspace через `start-local.ps1`.
Контейнерные проверки выполняются только через отдельный временный Compose
project с task/purpose labels, собственными ресурсами и cleanup в finally;
правила принадлежат [Base LOCAL_GROUPS](https://github.com/FerrPOINT/services-base/blob/main/deploy/LOCAL_GROUPS.md).

## 3. Гейты перед PR

- Backend: `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`.
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
