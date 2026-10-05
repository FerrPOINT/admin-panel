# Contributing — Admin Panel

## 1. Getting Started

```bash
git clone git@github.com:FerrPOINT/admin-panel.git
cd admin-panel
cp .env.example .env 2>/dev/null || true
```

## 2. Development Setup

Backend (Rust 1.88 и соседний Base на SHA из `.base-revision`):

```bash
python3 ../services-base/scripts/verify_base_revision.py --base ../services-base --revision .base-revision
cd backend
cargo test --locked --workspace
cd ..
python3 scripts/test_ai_foundation.py
```

Frontend:

```bash
cd frontend
pnpm install --frozen-lockfile
pnpm test
```

Для полного workspace используйте его штатный helper и профиль. Standalone
Compose описан в `docs/LOCAL_SETUP.md`; не запускайте его рядом с действующим
стендом как дополнительную постоянную группу. Интеграционный harness выше
создаёт собственный временный Compose project и очищает свои контейнеры.

## 3. Гейты перед PR

- Backend: `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo test --locked --workspace`.
- Frontend: typecheck, tests, lint, build.
- Docs: `python3 scripts/verify_readme.py`.
- CI: docs, backend, frontend, minimum-Rust, foundation-integration и Compose config.

## 4. Соглашения

- Контракт первым: OpenAPI-схема из Rust-хендлеров; CI ловит drift.
- AuthZ fail-closed: мутации требуют principal; роли — от central auth.
- Секреты не коммитятся; в примерах и логах — только `[REDACTED]`.
- Доки на русском, код и комментарии на английском.
- Каждый существенный объём работ — план в `docs/plans/` + CHANGELOG-запись.

## 5. Сообщения о безопасности

Только приватно — см. [SECURITY.md](SECURITY.md).
