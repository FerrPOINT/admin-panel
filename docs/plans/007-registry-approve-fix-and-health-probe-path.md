# Plan 007 — Registry approve fix + catalog-driven health probe

Статус 2026-09-11: реализуется. Bug fix объёма Wave 5 N (Base integration).

## Контекст

Регистрация `project-workflow` и `java-agent` в реестре Base (Wave 5 N, решение «both») вскрыла два дефекта v1:

1. `RegistryStore::approve_declaration` / `update_metadata` / `set_status` используют
   `UPDATE ... RETURNING` без колонок `health_status, health_checked_at, health_detail`,
   добавленных миграцией 0003, при `FromRow`-маппинге `RegistryEntryRow` — approve
   любой декларации падает в runtime с `no column found for name: health_status`.
2. `health_worker` хардкодит путь `/health/live`, а capability catalog
   (docs/API.md §6, миграция 0001) фиксирует `health.read → GET /health`.
   Сервисы без `/health/live` (project-workflow) или с auth на нём (java-agent)
   помечаются `unreachable`, хотя контрактный `GET /health` отвечает 200.

## Изменения

- `backend/infra/src/registry.rs`: RETURNING-списки трёх UPDATE дополнены
  health-колонками.
- `backend/infra/src/registry.rs`: новый `health_probe_path()` — чтение
  `fixed_path` активного `health.read` из `capability_catalog` (fallback `/health`).
- `backend/server/src/health_worker.rs`: probe использует catalog-путь вместо
  хардкода `/health/live`.
- Тесты: `backend/infra/tests/registry_returning_columns.rs` (регрессия
  RETURNING-колонок); source-level проверка отсутствия хардкода
  `/health/live` в worker.

## Проверка

- `cargo test --workspace` в rust:1.88 (services-base mount) — green.
- Live: регистрация project-workflow и java-agent (declaration → approve →
  `active`), health-статусы обоих `healthy` после прохода worker.

## Не входит

- Реализация `POST/GET /services/{key}/checks` (documented gap v1, отдельный план).
- Изменение capability catalog или контракта API.
