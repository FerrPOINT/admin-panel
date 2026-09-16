# Plan 008 — Service integration checks (documented v1 contract)

Статус 2026-09-11: реализуется. Закрывает задокументированный в docs/API.md §5.5
контракт `POST/GET /api/v1/services/{service_key}/checks` (Table `service_check_runs`
уже существует с миграции 0001; в v1 не был обвязан endpoint-ами).

## Контракт (docs/API.md)

- `POST /services/{service_key}/checks` `{capability}` — operator+.
  Только для `active` записи и capability активной approved декларации.
  Сервер строит request сам из локального capability catalog
  (`fixed_method` + `fixed_path`) и `integration_base_url`; клиентские
  body/headers/query/path/method не принимаются. Ответ `202` — check run.
- `GET /services/{service_key}/checks` — viewer+, история проверок.

## Реализация

- `RegistryStore`: `list_check_runs(entry_id, limit)`,
  `insert_check_run(...)`, `find_active_declaration(entry_id)`.
- `api`: `run_service_check` — валидация (entry exists → active → capability
  в декларации → capability в каталоге), запуск probe через reqwest с
  host.docker.internal rewrite (как health worker), insert run
  (outcome по HTTP/transport), audit event. `GET` — история.
- OpenAPI paths + DTO; regen `openapi/openapi.json` (CI проверяет актуальность).

## Проверка

- `cargo test --workspace` (rust:1.88, services-base mount) — green.
- `cargo run -p admin-panel-api --bin gen-openapi` == committed json.
- Live: POST check для project-workflow → 202 → GET история → outcome success.

## Не входит

- UI изменений нет (API-контракт); фоновый health worker не меняется.
