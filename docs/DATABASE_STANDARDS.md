# Database Standards

## 1. Назначение и владение данными

PostgreSQL -- источник истины для конфигурации платформы, реестра integration contracts, capability policy, ревизий, публикаций и аудита Admin Panel. `auth-server` -- источник истины только для identity, credentials, сессий и ключевого материала подписи. Admin Panel хранит максимум внешний subject identifier и снимок авторизационных атрибутов, нужный для аудита; не копирует пароль, refresh token, credential secret или пользовательский профиль.

Ни один consumer не пишет конфигурационные таблицы напрямую. Изменения проходят через application layer, проверку версии, авторизацию и audit trail.

## 2. Моделирование

- Таблицы и поля -- `snake_case`; первичные ключи -- `uuid` (UUIDv7 генерируется приложением); внешние ключи имеют суффикс `_id`.
- Время хранится как `timestamptz` в UTC: `created_at`, `updated_at`; опубликованные/архивные сущности имеют явные `published_at`, `archived_at` при необходимости.
- Обязательные связи защищены foreign key; для бизнес-статусов используются checked text или PostgreSQL enum при стабилизированной семантике.
- `jsonb` допустим для versioned config payload и schema-валидируемых metadata. Поля, по которым фильтруют, связывают или авторизуют, нормализуются в колонки.
- Каждая конфигурационная ревизия неизменяема после публикации. Draft меняется с optimistic concurrency; публикация сохраняет автора, время и предыдущую ревизию.

## 3. Базовые сущности

| Сущность | Ответственность |
|---|---|
| `integrations` | consumer, owner, lifecycle и contract reference |
| `integration_capabilities` | allowlisted capability и schema/version для integration |
| `config_revisions` | immutable payload, status, checksum, predecessor |
| `config_assignments` | какой consumer получает какую опубликованную ревизию |
| `audit_events` | actor subject, действие, target, request ID, до/после в redacted виде |

Значения конфигурации разделяются на public/runtime-safe и restricted. Секрет не должен храниться в `config_revisions`; при необходимости хранится только ссылка на внешний secret manager, без значения. Публичная проекция строится на сервере allowlist-ом, а не удалением полей на frontend.

## 4. Миграции и целостность

- Схема меняется только версионными SQLx migrations, которые применимы вперёд и проверяются на пустой и существующей БД.
- Одна migration решает одну логическую задачу; destructive change выполняется как expand -> migrate/backfill -> contract после срока совместимости.
- Миграции не редактируются после публикации. Исправление оформляется новой migration.
- Для внешних запросов используются параметризованные SQLx queries; строковая конкатенация SQL и dynamic column names без allowlist запрещены.
- Transaction покрывает единый атомарный переход: проверка revision, изменение состояния, audit event и outbox event при его наличии.

## 5. Индексы, доступ и retention

Индексируются foreign keys, поля выборки активных assignments, `published_at`, cursor-сортировка и `audit_events(occurred_at, id)`. Индекс добавляется по измеренному запросу и объяснённому плану, а не «на всякий случай».

DB role приложения имеет минимальные права и не выполняет DDL в runtime. Backup/restore регулярно проверяются. Audit events не изменяются обычным application path; retention и экспорт определяются отдельной политикой и не удаляются каскадом вместе с configuration revision.

## Связанные документы

- `docs/API_STANDARDS.md`
- `docs/LOGGING_STANDARDS.md`
- `docs/adr/0003-postgresql.md`
