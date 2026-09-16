# План 009 — Каталог v1.1: health + ui_url в runtime/services

## Контекст
Переключение между приложениями флота (ServiceSwitcher в @sdlc/ui) строится на
`GET /api/v1/runtime/services`. Сервисы без UI (java-agent) сейчас ведут на
API-порт; живой health-статус в каталоге отсутствует.

## Изменения
1. Миграция `0004_catalog_ui_render.sql`:
   - `ui.render` в capability_catalog (GET /);
   - data-migration: активные декларации UI-сервисов (admin-panel, ci-cd,
     fleet-control, project-workflow, task-tracker, wiki) получают `ui.render`.
2. `runtime_services` (api): в выдаче добавлены
   - `ui_url` — integration_base_url при capability `ui.render`, иначе `null`;
   - `health` — health_status из реестра, деградация `"unknown"`.
3. OpenAPI перегенерирован (18 paths).
4. Регрессионный тест `server/tests/runtime_catalog_v1_1.rs`.

## Совместимость
Поля только добавляются; ETag-версия каталога не меняет схему кэширования.
Потребители v1.0 (текущие свитчеры) игнорируют новые поля.

## Проверка
cargo fmt --check, clippy -D warnings, cargo test --workspace (17 ok),
live: 7 сервисов, 6 с ui_url, java-agent null, health healthy.
