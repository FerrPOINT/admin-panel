# API Standards

## 1. Границы API

Admin Panel API -- REST/JSON API управления **платформенной конфигурацией**. Базовый namespace: `/api/v1`. Он владеет интеграциями, capability policy, конфигурационными draft/published revisions и audit trail. `auth-server` владеет только identity и выдачей/проверяемыми атрибутами доступа; Admin Panel не создаёт пользователей, пароли или сессии через свой API.

Интеграция с потребителями декларативна: Admin Panel публикует разрешённые значения и capability contract. API не предоставляет endpoint для удалённого исполнения произвольной команды, изменения произвольного файла, вызова произвольного URL или проксирования credentials.

## 2. Базовые правила

- HTTP/1.1 или HTTP/2, JSON UTF-8, `Content-Type: application/json`.
- Имена ресурсов -- множественное число: `/integrations`, `/configuration-revisions`, `/audit-events`.
- Идентификаторы -- UUIDv7 в строковом представлении; время -- ISO 8601 UTC.
- `GET` не меняет состояние; `POST` создаёт ресурс/команду; `PATCH` частично меняет draft; `DELETE` допустим только там, где не нарушает историю.
- Создание возвращает `201 Created` и `Location`; успешное удаление -- `204 No Content`; публикация или откат могут вернуть `202 Accepted`, если операция асинхронна.
- Коллекции используют cursor pagination: `cursor`, `limit` (default 20, max 100), стабильную сортировку и allowlist полей фильтрации/сортировки.

## 3. Ресурсы v1

| Ресурс | Назначение |
|---|---|
| `/integrations` | зарегистрированные потребители и их поддерживаемые capability |
| `/integrations/{id}/capabilities` | декларация поддерживаемых/разрешённых capability |
| `/configuration-revisions` | черновики, опубликованные ревизии и rollback target |
| `/runtime-config` | эффективная безопасная конфигурация для конкретного consumer/context |
| `/audit-events` | неизменяемое представление административного аудита |

Capability -- заранее определённый тип действия/настройки с валидируемой схемой и областью действия, например `feature_flags.read` или `ui.branding.read`. Значения вне зарегистрированной schema или capability не принимаются.

## 4. Авторизация

Защищённые endpoint принимают `Authorization: Bearer <access_token>`. API валидирует подпись и claims по JWKS `auth-server`, а затем применяет локальную policy к роли/permission. Нельзя доверять заголовкам пользователя, данным UI или незаверенным claims. `401` означает отсутствующую/недействительную аутентификацию, `403` -- недостаточные полномочия.

Service-to-service доступ использует отдельный короткоживущий machine identity с минимальным scope. Consumer получает только свою effective configuration и только capability, разрешённые его зарегистрированному контракту.

## 5. Конкурентность, кэширование и идемпотентность

- Чтение revision и runtime config возвращает `ETag`; клиент передаёт `If-None-Match` для чтения и `If-Match` для изменения.
- Несовпадение версии возвращает `412 Precondition Failed`; бизнес-конфликт -- `409 Conflict` с объяснением безопасным для роли клиента.
- Команды публикации и чувствительные `POST` принимают `Idempotency-Key` (UUID), чтобы повторы не создавали повторную ревизию или audit event.
- Ответ runtime config объявляет корректный `Cache-Control`; секреты и приватные поля никогда не попадают в кэшируемое публичное представление.

## 6. Ошибки и наблюдаемость

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Некорректные параметры запроса",
    "details": [{ "field": "capability", "message": "unsupported" }],
    "request_id": "req_..."
  }
}
```

`details` не раскрывает секреты, внутренние адреса или существование недоступных ресурсов. Поддерживаемые коды: `VALIDATION_ERROR`, `UNAUTHENTICATED`, `FORBIDDEN`, `NOT_FOUND`, `CONFLICT`, `PRECONDITION_FAILED`, `RATE_LIMITED`, `INTERNAL_ERROR`. Каждый ответ несёт `X-Request-Id`.

## 7. Контракты

OpenAPI генерируется из Rust handler/DTO и публикуется вместе с релизом; это контракт HTTP, а не единственное место бизнес-правил. Несовместимое изменение требует новой major API version по `docs/API_VERSIONING.md`. Изменение capability schema требует совместимой эволюции или новой capability/versioned contract.

## Связанные документы

- `docs/API_VERSIONING.md`
- `docs/LOGGING_STANDARDS.md`
- `docs/adr/0004-central-auth-jwks.md`
- `docs/adr/0006-service-integration-contract.md`
