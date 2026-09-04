# ADR-0005: Runtime config с ревизиями и ETag

## Status

Accepted

## Context

Consumers платформы должны получать актуальную конфигурацию без постоянного polling полного payload и без источников истины, дублирующих Admin Panel. Одновременно конфигурация может изменяться администраторами параллельно, а недоступность control plane не должна включать опасное поведение. В public config нельзя отдавать secrets.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| Конфигурация в env каждого consumer | простое чтение | дублирование, дрейф и сложная публикация |
| Polling полного JSON без версий | быстро начать | лишний трафик, нет защиты от lost update |
| Push/config agent | быстрые обновления | дополнительный runtime и протокол для v1 |
| Revisioned API + ETag | HTTP-совместимость, conditional GET, аудит и concurrency | consumer должен реализовать cache/fallback policy |

## Decision

Admin Panel хранит immutable published configuration revisions и отдаёт безопасную effective projection через runtime config API. Ответ содержит стабильный `revision_id`, `schema_version` и `ETag`. Consumer выполняет conditional GET с `If-None-Match`; административная запись использует `If-Match` и получает `412` при stale revision. Публикация создаёт audit event.

Consumer валидирует схему, использует последнюю валидную конфигурацию только в установленный TTL либо безопасные defaults. Отсутствующее/неизвестное значение не включает capability и не запускает удалённое действие. Секреты, токены и private settings исключены из runtime projection.

## Consequences

- Однозначный источник истины и наблюдаемая история конфигурации.
- Уменьшение трафика и нагрузки на неизменных чтениях через `304 Not Modified`.
- Нужны schema evolution, cache TTL, conflict UX и измерение fallback/ETag hit rate.
- ETag защищает представление и конкурентную запись, но не является authorization artifact.

## Related

- `docs/API_STANDARDS.md`
- `docs/API_VERSIONING.md`
- `docs/PERFORMANCE.md`
- `docs/adr/0006-service-integration-contract.md`
