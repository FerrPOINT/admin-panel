# Миграции базы данных SDLC Admin Panel

## 1. Назначение

Миграции управляют только PostgreSQL-схемой Admin Panel на `7773`. Они не подключаются к storage central auth на `7701`, не создают таблицы в CI-CD/task-tracker/wiki/fleet-control и не изменяют их схему.

Выбор конкретного migration framework и расположения исходников будет зафиксирован вместе с backend stack. Независимо от инструмента применяются правила этого документа.

## 2. Начальная схема v1

Первая миграционная серия должна создать в зависимости:

1. `capability_catalog`;
2. `service_registry_entries`;
3. `service_declarations`;
4. `service_check_runs`;
5. `branding_revisions`;
6. `role_bindings`;
7. `audit_events`;
8. индексы, constraints и append-only policy аудита.

Полная целевая схема, типы, индексы и инварианты приведены в `docs/DATA_MODEL.md`. Миграции являются фактическим источником правды после появления кода; документ обновляется в том же изменении.

## 3. Правила именования и порядка

- Каждая миграция имеет неизменяемый уникальный идентификатор и упорядоченный префикс UTC timestamp/sequence согласно выбранному framework.
- Описание — lowercase snake_case, например `create_service_registry`.
- Одна миграция решает одну согласованную schema-задачу; не смешивает схему, секреты, предметные данные внешнего сервиса и несвязанный refactor.
- Изменение уже примененной migration запрещено. Исправление выполняется следующей migration.
- Новая migration обязана зависеть от текущей единственной head revision; ветвление цепочки до отдельного согласованного merge migration запрещено.
- Миграции повторяемы только через framework history table, но их DDL не должен создавать непредсказуемую runtime-логику.

## 4. Безопасные изменения схемы

### 4.1. Добавление

- Новую колонку на живой таблице сначала добавлять nullable либо с безопасным server default.
- Сначала развернуть compatibility-код, затем backfill bounded batches, затем в отдельной migration добавить `NOT NULL`/constraint после проверки.
- Уникальные и search-индексы на больших production таблицах строить non-blocking способом, который поддерживает выбранный PostgreSQL/framework.
- Foreign key добавлять `NOT VALID`, исправлять сироты, затем `VALIDATE CONSTRAINT`, если это необходимо для снижения lock времени.

### 4.2. Изменение и удаление

- В production не полагаться на destructive `down`. Использовать compensating forward migration.
- Нельзя физически удалять registry history, branding revision и audit events ради rollback.
- Поле, которое перестает читаться, проходит expand/contract: сначала dual-read/compatibility, затем migration удаления после окна совместимости.
- `service_key` не переименовывается скрыто: это стабильная integration identity. Нужна явная migration стратегии с redirect/mapping и audit, одобренная ADR.

## 5. Инварианты, которые закрепляются схемой

| Инвариант | Механизм |
|---|---|
| Один `service_key` на запись | UNIQUE constraint. |
| Service активен только с approved declaration | FK/check constraint плюс transactional application validation. |
| Одна published branding revision | Partial UNIQUE index `WHERE state = 'published'`. |
| Revision и ETag уникальны | UNIQUE constraints. |
| Audit append-only | Отсутствие прикладных update/delete routes; DB role grants/trigger policy по выбранному deployment. |
| Capability check ссылается на известную capability | FK `service_check_runs.capability_key` -> catalog. |
| Нет внешних DB dependencies | Нет cross-database FK, FDW и migration calls к чужим DB. |

SQL check не заменяет business validation: JSON capability/branding document и cross-row approval проверяются приложением в транзакции.

## 6. Seed-данные

Начальный capability catalog является versioned product reference data:

- `health.read`;
- `integration.status.read`;
- `branding.runtime.read`.

Seed должен быть идемпотентен, не принимать values из runtime UI и не включать endpoint, token, password, private host или secret. Изменение catalog — security/API изменение, требующее миграции/кода/тестов/документации, а не ручной SQL-правки production.

Стартовая published branding revision не обязательна: пока ее нет, runtime endpoint возвращает controlled `404`, а consumers используют свои дефолты. Миграция не должна записывать организационные названия, logo URL или любые реальные deployment values.

## 7. Применение

### Local и CI

1. Поднять изолированную PostgreSQL для Admin Panel.
2. Применить все migrations на чистой БД.
3. Проверить schema/invariants и seed catalog.
4. Запустить API integration tests, включая publish/ETag, registry transition и audit atomarity.
5. Проверить upgrade с предыдущей production-like schema, если migration не первая.

### Production

1. Сделать проверенный backup собственной Admin Panel PostgreSQL.
2. Проверить текущую migration version, lock/statement timeout и свободное место.
3. Применить migrations отдельным job/шагом deployment, не из browser/UI.
4. Дождаться завершения и проверить `health/ready`.
5. Проверить published branding revision, registry count, ограничения схемы и отсутствие orphan rows.
6. При проблеме применить заранее подготовленную compensating migration или откатить приложение к совместимой версии; не править историческую migration и не выполнять destructive reset.

## 8. Тесты миграций

Каждое изменение модели требует как минимум:

- fresh install test: все migrations применяются к пустой PostgreSQL;
- upgrade test: предыдущая schema обновляется без потери разрешенных history данных;
- rollback/compensation test при destructive-risk изменении;
- constraint test для уникальности service key, single published branding, FK capability и active declaration;
- transaction test: mutation создает domain change и audit event вместе;
- secret scan: fixtures, SQL и logs не содержат реальных секретов.

Для больших таблиц дополнительно нужны performance/lock evaluation и rehearsal с representative copy. До этого не объявлять migration production-safe.

## 9. Запрещенные операции

- `DROP DATABASE`, reset/clean, ручное удаление audit/revision history в production.
- Подключение migration к БД auth или соседнего SDLC сервиса.
- Сохранение secret/token/password в seed, SQL comment, default, fixture или audit backfill.
- Непроверенный raw SQL, который строится из пользовательского ввода.
- Автоматическое добавление service registry entries по network scan.

## 10. Документирование изменения

PR/задача с migration должна обновить при необходимости:

- `docs/DATA_MODEL.md` — новая фактическая схема и инварианты;
- `docs/API.md` — изменившийся контракт/etag/status;
- `docs/ARCHITECTURE.md` — новое решение/граница;
- `docs/SECURITY.md` — изменение SSRF, audit или secret boundary;
- новый ADR при долговременном архитектурном выборе.

## References

- `docs/DATA_MODEL.md`
- `docs/ARCHITECTURE.md`
- `docs/API.md`
- `docs/SECURITY.md`
- `docs/RUNTIME.md`
