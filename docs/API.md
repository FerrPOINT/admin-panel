# API v1: SDLC Admin Panel

## 1. Общие правила

- Base URL: `https://{admin-api-origin}/api/v1` (runtime порт umbrella: `7771`).
- Формат: `application/json; charset=utf-8`.
- Версионирование: path-based `/api/v1`.
- Machine-readable source of truth после реализации: OpenAPI, генерируемая из backend DTO/handlers. До реализации этот документ — целевой контракт.
- Все timestamps — RFC 3339 UTC; идентификаторы — UUID.
- Ответы списков ограничены и детерминированно сортированы; по умолчанию `limit=50`, максимум `100`.

## 2. Аутентификация

Защищенные endpoints требуют:

```http
Authorization: Bearer {access-jwt}
```

JWT выпускается существующим central auth на `7701`, подписывается ES256 и проверяется через JWKS. Admin Panel не имеет `/login`, `/refresh`, `/logout`, `/register`, endpoint выдачи токена или storage auth-сервера.

Runtime branding endpoint предназначен для прямого чтения потребителями и в v1 не требует пользовательского bearer token. Его доступность ограничивается origin/network policy deployment-а, а не gateway/CDN.

## 3. Общие ошибки

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Значение не прошло проверку",
    "request_id": "uuid",
    "details": [{"field": "service_key", "reason": "invalid_format"}]
  }
}
```

| HTTP | Код | Когда |
|---|---|---|
| 400 | `BAD_REQUEST` | Некорректный синтаксис запроса. |
| 401 | `UNAUTHENTICATED` | Нет/невалиден JWT или JWKS validation не пройдена. |
| 403 | `FORBIDDEN` | Недостаточна роль панели. |
| 404 | `NOT_FOUND` | Ресурс не существует или скрыт политикой доступа. |
| 409 | `CONFLICT` | Дубликат ключа или недопустимый переход состояния. |
| 412 | `PRECONDITION_FAILED` | Не совпал `If-Match`/ETag текущего ресурса. |
| 422 | `VALIDATION_ERROR` | Семантически невалидные fields/capabilities. |
| 429 | `RATE_LIMITED` | Превышен лимит. |
| 502 | `INTEGRATION_UNAVAILABLE` | Утвержденный endpoint не ответил корректно. |
| 504 | `INTEGRATION_TIMEOUT` | Истек ограниченный timeout проверки. |

Тела ошибок не включают bearer token, секреты, стек, полный внешний URL с параметрами, payload или заголовки интегрируемого сервиса.

## 4. Health

| Метод | Путь | Auth | Назначение |
|---|---|---|---|
| GET | `/health/live` | нет | Процесс жив. |
| GET | `/health/ready` | нет | Собственная БД и миграции готовы. |

Readiness не зависит от доступности сервисов реестра и не делает внешний запрос.

## 5. Реестр сервисов

### 5.1. Список и карточка

| Метод | Путь | Роль | Назначение |
|---|---|---|---|
| GET | `/services` | viewer+ | Список реестра. |
| GET | `/services/{service_key}` | viewer+ | Текущая карточка и активная декларация. |
| POST | `/services` | operator+ | Создать pending запись и первую декларацию. |
| PATCH | `/services/{service_key}` | operator+ | Изменить только display metadata/owner, с `If-Match`. |
| POST | `/services/{service_key}/declarations` | operator+ | Подать новую pending декларацию. |
| POST | `/services/{service_key}/approve` | admin | Одобрить указанную pending декларацию. |
| POST | `/services/{service_key}/disable` | admin | Отключить интеграцию. |
| POST | `/services/{service_key}/retire` | admin | Вывести интеграцию из эксплуатации. |
| POST | `/services/{service_key}/checks` | operator+ | Запустить read-only проверку разрешенной capability. |
| GET | `/services/{service_key}/checks` | viewer+ | История проверок. |

`GET /services` поддерживает `status`, `owner_team`, `cursor`, `limit`. Сортировка: `updated_at DESC, service_key ASC`.

### 5.2. Создание сервиса

```http
POST /api/v1/services
If-Match: "registry-empty-v1"
```

```json
{
  "service_key": "task-tracker",
  "display_name": "Task Tracker",
  "owner_team": "platform",
  "declaration": {
    "declaration_version": 1,
    "integration_base_url": "https://service.example.internal",
    "service_contract_version": "1",
    "capabilities": ["health.read", "integration.status.read"]
  }
}
```

Ответ `201` возвращает `service`, `declaration` со статусом `pending`, ETag карточки и `Location`. Пример не является реальным endpoint или конфигурацией.

Поле `integration_base_url`: HTTPS origin без credentials, query, fragment и path. Разрешенность host/network проверяется сервером. Пользователь не передает endpoint конкретной проверки и не передает HTTP method.

### 5.3. Формат декларации

```json
{
  "id": "uuid",
  "declaration_version": 1,
  "service_contract_version": "1",
  "integration_base_url": "https://service.example.internal",
  "capabilities": ["health.read"],
  "approval_status": "pending",
  "declared_at": "2026-09-04T00:00:00Z"
}
```

Новые endpoint/capabilities передаются только новой декларацией. `PATCH /services/{service_key}` не способен менять `integration_base_url`, capability или статус одобрения.

### 5.4. Одобрение

```http
POST /api/v1/services/task-tracker/approve
If-Match: "service-etag"
```

```json
{ "declaration_id": "uuid" }
```

Успех `200`: указанная декларация получает `approved`, активная ревизия записи обновляется, запись становится `active`, создается audit event. Если декларация не `pending`, ответ `409`.

### 5.5. Проверка интеграции

```http
POST /api/v1/services/task-tracker/checks
```

```json
{ "capability": "health.read" }
```

Проверка доступна только для `active` записи и capability активной approved декларации. Сервер строит request из локального capability catalog и `integration_base_url`; body, headers, query, path или method от клиента не принимаются. Результат `202` возвращает check run; клиент получает итог через `GET /services/{service_key}/checks`.

## 6. Capability catalog

| Метод | Путь | Роль | Назначение |
|---|---|---|---|
| GET | `/capabilities` | viewer+ | Список разрешенных capabilities. |

Catalog в v1 read-only. Изменение набора capabilities выполняется вместе с выпуском API/безопасностного контракта, не через generic CRUD.

## 7. Runtime branding

### 7.1. Получение опубликованного документа

| Метод | Путь | Auth | Назначение |
|---|---|---|---|
| GET | `/runtime/branding` | нет | Прямое чтение текущего опубликованного бренда потребителями. |

Headers запроса:

```http
If-None-Match: "branding-r42-hash"
```

Успешный ответ:

```http
HTTP/1.1 200 OK
ETag: "branding-r42-hash"
Cache-Control: public, max-age=60, must-revalidate
Vary: Origin
Content-Type: application/json
```

```json
{
  "revision": 42,
  "updated_at": "2026-09-04T00:00:00Z",
  "branding": {
    "product_name": "SDLC",
    "product_short_name": "SDLC",
    "logo_url": "https://public.example/logo.svg",
    "favicon_url": "https://public.example/favicon.ico",
    "support_url": "https://public.example/support",
    "primary_color": "#123456",
    "accent_color": "#234567",
    "surface_color": "#f5f5f0"
  }
}
```

При совпадающем ETag — `304 Not Modified` с `ETag` и `Cache-Control`, без тела. Если опубликованной revision нет, API возвращает `404 BRANDING_NOT_PUBLISHED`; потребитель применяет свои дефолты. Не используется CDN, gateway, redirect или webhook.

### 7.2. Администрирование бренда

| Метод | Путь | Роль | Назначение |
|---|---|---|---|
| GET | `/branding/revisions` | viewer+ | Список ревизий. |
| GET | `/branding/revisions/{revision}` | viewer+ | Конкретная ревизия. |
| POST | `/branding/revisions` | operator+ | Создать draft. |
| PATCH | `/branding/revisions/{revision}` | operator+ | Изменить draft с `If-Match`. |
| POST | `/branding/revisions/{revision}/publish` | admin | Атомарно опубликовать draft. |
| POST | `/branding/revisions/{revision}/clone` | operator+ | Создать draft-копию для controlled rollback. |

`PATCH` разрешает только поля утвержденной branding schema. Нельзя передавать CSS, HTML, JS, tokens, URL с credentials или произвольные вложенные ключи. Publication изменяет текущий ETag.

## 8. Роли панели

| Метод | Путь | Роль | Назначение |
|---|---|---|---|
| GET | `/access/role-bindings` | admin | Список mappings claims -> panel role. |
| POST | `/access/role-bindings` | admin | Создать mapping. |
| DELETE | `/access/role-bindings/{id}` | admin | Удалить mapping. |

Запрос принимает только `claim_name`, `claim_value`, `panel_role`. Он не создает пользователя, не меняет central auth и не принимает `sub` как индивидуальную замену централизованной группы без отдельной policy.

## 9. Аудит

| Метод | Путь | Роль | Назначение |
|---|---|---|---|
| GET | `/audit-events` | viewer+ | Фильтруемый неизменяемый след. |

Query: `actor_subject`, `action`, `entity_type`, `entity_id`, `from`, `to`, `cursor`, `limit`. Ответ не содержит чувствительные `metadata`; они предварительно санитизируются при записи.

## 10. Идемпотентность и конкуренция

- `POST /services`, создание declaration, draft и role binding принимает `Idempotency-Key` UUID; повтор с тем же ключом и payload возвращает исходный результат.
- Конкурентно изменяемые ресурсы возвращают ETag в `GET`/mutation response.
- `PATCH`, approve, disable, retire и publish требуют `If-Match`. Отсутствующий/старый precondition дает `428`/`412` соответственно.
- Внутренние background retries v1 отсутствуют; клиент повторяет только безопасные GET либо POST с тем же idempotency key.

## References

- `docs/TZ.md`
- `docs/ARCHITECTURE.md`
- `docs/DATA_MODEL.md`
- `docs/SECURITY.md`
- `docs/RUNTIME.md`
