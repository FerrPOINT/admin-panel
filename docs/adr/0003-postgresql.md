# ADR-0003: PostgreSQL как основная БД

## Status

Accepted

## Context

Admin Panel хранит связанные, проверяемые и аудируемые данные: integrations, capability policy, конфигурационные ревизии, assignments и audit events. Для публикации нужна транзакционная целостность между проверкой версии, сменой active revision и аудитом. Нельзя дублировать identity/credentials из `auth-server` или хранить secrets в config payload.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| SQLite | нулевой операционный порог | недостаточна для multi-instance/конкурентной control plane нагрузки |
| Документная БД | гибкий payload | слабее связи, транзакции и audit query для ядра модели |
| PostgreSQL | ACID, JSONB, индексы, миграции, совместимость | требует backup, monitoring и эксплуатации |

## Decision

Использовать PostgreSQL как единственный persistent source of truth Admin Panel. Нормализованные связи и ограничения используются для ядра домена; `jsonb` разрешён для versioned schema-валидируемого config payload. Schema меняется только версионными SQLx migrations. Секреты не сохраняются: допустима лишь ссылка на внешний secret manager.

## Consequences

- Целостная публикация ревизии, assignments и audit event в одной транзакции.
- Удобные cursor queries и анализ планов для control-plane API.
- Требуются бэкапы, проверка восстановления, миграционная дисциплина и least-privilege DB role.
- Consumers получают проекцию через API, а не прямой доступ к БД.

## Related

- `docs/DATABASE_STANDARDS.md`
- `docs/API_STANDARDS.md`
- `docs/adr/0005-runtime-config-etag.md`
