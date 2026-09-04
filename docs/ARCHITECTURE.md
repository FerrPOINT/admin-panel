# Архитектура SDLC Admin Panel

## 1. Контекст

Admin Panel — независимый control plane ограниченного назначения. Он централизует платформенную конфигурацию и метаданные интеграций, но сохраняет автономию SDLC-приложений. В v1 нет общего gateway и нет удаленного исполнения: каждый сервис остается владельцем своего домена, API, данных, runtime и секретов.

```text
                    +-------------------------------+
                    | Central auth :7701             |
                    | ES256 issuer + JWKS publisher  |
                    +---------------+---------------+
                                    |
                         access JWT | JWKS fetch/cache
                                    v
+--------------------+      +-------------------------------+
| Admin web :7772    |----->| Admin API :7771               |
| UI only            |      | registry / branding / audit    |
+--------------------+      +---------------+---------------+
                                            |
                                            | local persistence
                                            v
                                  +-------------------+
                                  | PostgreSQL :7773  |
                                  | Admin-owned data  |
                                  +-------------------+
                                            |
          approved, bounded read-only integration checks only
  +----------------+ +------------------+ +-------------+ +----------------+
  | CI-CD          | | task-tracker     | | wiki        | | fleet-control  |
  | owns its API   | | owns its API     | | owns its API| | owns its API   |
  +----------------+ +------------------+ +-------------+ +----------------+

Consumers fetch branding directly from Admin API; no CDN/gateway in v1.
```

## 2. Компоненты и владение

| Компонент | Владелец | Разрешенная ответственность | Явно вне границы |
|---|---|---|---|
| Admin web | Admin Panel | Административные экраны, локальное отображение прав и состояния. | Хранение access token, бизнес-операции внешних сервисов. |
| Admin API | Admin Panel | Реестр, брендинг, RBAC панели, аудит, проверка утвержденных интеграций. | Токены auth, универсальный proxy, task/pipeline/document CRUD. |
| Admin PostgreSQL | Admin Panel | Только таблицы модели Admin Panel. | Таблицы auth и таблицы соседних SDLC-приложений. |
| Central auth | Внешний auth-сервис | Идентичность, подписание ES256 JWT, JWKS. | Реестр, брендинг, роли Admin Panel, audit Admin Panel. |
| Сервисная интеграция | Сам сервис | Свой endpoint и capabilities, contract version, локальная авторизация запроса. | Управление реестром от имени Admin Panel. |
| Потребитель бренда | Каждое приложение | Дефолты, прямой GET, ETag-кеш, валидация документа. | Мутация центрального бренда, зависимость старта от Admin Panel. |

## 3. Логические слои API

Будущая реализация придерживается зависимостей `transport -> application -> domain <- infrastructure`.

- `transport`: HTTP DTO, извлечение claims, request-id, синтаксическая валидация, mapping ошибок.
- `application`: use cases регистрации/одобрения сервиса, публикации бренда, проверки интеграции, выдачи runtime-документа и записи аудита.
- `domain`: правила состояний, allowlist capability, модели revision и инварианты владения.
- `infrastructure`: PostgreSQL repositories, JWKS client/cache, HTTP client проверки endpoint, telemetry.

HTTP handler не пишет напрямую в БД и не выполняет произвольный удаленный запрос. Сервисный слой проверяет роль, состояние записи, capability и допустимый путь операции до вызова repository/HTTP adapter.

## 4. Реестр интеграций

### 4.1. Декларация и одобрение

1. Сервис или уполномоченный оператор подает декларацию по контракту регистрации.
2. Admin API валидирует ключ, endpoint, contract version и capability against local catalog.
3. Измененная декларация получает статус `pending`, даже если предыдущая версия была `active`.
4. `platform_admin` одобряет конкретную ревизию. Только после этого она становится `active`.
5. Admin API может выполнить только согласованную read-only проверку одной из заявленных capabilities.

Endpoint не является универсальным URL-полем для произвольного curl. Он служит базой для заранее описанного маршрута capability из локального каталога. Разрешенные hostname/CIDR и HTTPS-политика устанавливаются deployment-конфигурацией, а не вводом UI.

### 4.2. Каталог capabilities v1

| Capability | Назначение | Разрешенный эффект |
|---|---|---|
| `health.read` | Проверка доступности сервиса. | GET фиксированного health-маршрута. |
| `integration.status.read` | Сжатый статус интеграции. | GET фиксированного status-маршрута. |
| `branding.runtime.read` | Декларация, что сервис потребляет центральный брендинг. | Только метаданные совместимости, без конфигурации сервиса. |

Введение новой capability является изменением API/безопасности: требуются документация, OpenAPI, тесты allowlist и архитектурное решение. Capability не может иметь семантику `execute`, `proxy`, `write`, `secret.read` или произвольный путь.

## 5. Аутентификация и авторизация

Защищенный запрос несет bearer access JWT. API получает JWKS центрального auth по настроенному trusted origin, выбирает ключ по `kid` и проверяет строго ES256, issuer, audience, срок действия и обязательные claims. Ключи кешируются с ограниченным TTL согласно JWKS response/локальной политике; неизвестный `kid` вызывает безопасное обновление JWKS, но не обход проверки.

Admin Panel сопоставляет утвержденные auth claims с собственными ролями. Это локальная policy-layer, а не хранилище identity. Панель не выпускает, не продлевает, не отзывает токены и не обращается к БД auth-сервера.

## 6. Runtime-брендинг

### 6.1. Публикация

Бренд хранится как versioned document: черновик валидируется, затем publication atomically создает новую опубликованную revision. ETag детерминированно выводится из опубликованной revision и canonical JSON. Старые revisions остаются для аудита и controlled rollback, но выдача runtime возвращает только текущую опубликованную revision.

### 6.2. Чтение потребителем

```text
Consumer                         Admin API
   | GET /api/v1/runtime/branding  |
   | If-None-Match: "..."          |
   |------------------------------->|
   | 200 + ETag + Cache-Control     |  changed/first load
   | or 304 + Cache-Control         |  unchanged
   |<-------------------------------|
```

- Direct API only: потребитель обращается к origin Admin API напрямую.
- `Cache-Control: public, max-age=60, must-revalidate` — базовая политика v1.
- ETag является strong validator для полного документа.
- На ошибке consumer сохраняет последний валидный результат в своей памяти/локальном хранилище, если это разрешено его политикой, иначе использует compile-time defaults.
- Runtime endpoint не зависит от readiness интегрируемых сервисов и не выполняет к ним вызов.

## 7. Данные и согласованность

PostgreSQL Admin Panel — единственный источник правды для registry, revisions бренда, правил ролей и аудита. Ссылки на внешние субъекты (`actor_subject`, `service_key`) не имеют FK в чужие БД. Внешний сервис удален или недоступен — его реестровая история сохраняется, а последний check отмечается соответствующим статусом.

Каждая mutation выполняется транзакционно вместе с audit event. Конфликт изменения документных ресурсов решается optimistic concurrency (`If-Match` / version). Без совпадения текущего ETag API возвращает `412 Precondition Failed`.

## 8. Надежность

- Admin API запускается только после собственных миграций и доступной собственной БД.
- Недоступность external service не делает Admin API unhealthy; она влияет только на результат конкретной проверки.
- Недоступность central auth/JWKS запрещает новые защищенные запросы при отсутствии валидного cached key; публичный runtime branding продолжает обслуживаться.
- HTTP checks ограничены timeout, response size, redirects, частотой и сетевыми allowlist.
- Никакой очереди, webhook delivery, event bus или фоновой синхронизации сервисов в v1 нет.

## 9. Наблюдаемость

Логи структурированы и включают `request_id`, route, status, actor subject в псевдонимизированной/разрешенной форме, `service_key`, action и latency. Запрещено записывать bearer token, Authorization header, JWKS body целиком, произвольные ответы интеграций, URI с credentials и значения секретов.

Метрики: HTTP latency/status, JWKS cache hit/miss/error, branding 200/304, registry state counts, integration check outcome/duration. URL и полный payload внешнего сервиса не применяются как metric labels.

## 10. Решения v1

- Прямой REST API и PostgreSQL; stack конкретизируется до реализации, но контракт и порты фиксированы уже сейчас.
- OpenAPI становится машинным источником правды после появления backend; `docs/API.md` фиксирует продуктовый контракт до этого.
- CDN/gateway, multi-region replication, remote configuration произвольных полей, push branding и синхронизация в auth-server не входят в v1.

## References

- `docs/TZ.md`
- `docs/DATA_MODEL.md`
- `docs/API.md`
- `docs/RUNTIME.md`
- `docs/SECURITY.md`
