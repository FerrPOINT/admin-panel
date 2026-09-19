# API v1: Base Admin Panel

`openapi/openapi.json` — машиночитаемый контракт, генерируемый из Rust handlers. CI сравнивает свежую генерацию с committed file; этот документ — операторская карта, а не дублирующая схема.

## Базовые адреса

| Развёртывание | API | Web |
|---|---|---|
| Base umbrella | `http://127.0.0.1:7771` | `http://127.0.0.1:7772` |
| Repository Compose | `http://127.0.0.1:7771` | `http://127.0.0.1:7772` |

Все versioned endpoints имеют префикс `/api/v1` и используют JSON.

## Модель доступа

| Класс | Аутентификация | Фактическая policy |
|---|---|---|
| Health | Нет | `GET /health/live`, `GET /health/ready` |
| Public runtime | Нет | Только safe published branding и active catalog |
| Session | ES256 bearer через configured central JWKS | `GET /auth/me` |
| Protected API | Valid central bearer + service scope | Registry, branding, checks, audit, users и tokens |

UI выполняет Central Auth Authorization Code + PKCE через публичный issuer.
`POST /auth/login` является legacy proxy и возвращает `410 SSO_REQUIRED` при
настроенном Central Auth. Панель не хранит credentials, browser sessions или
token secrets. Все активные вошедшие люди имеют одинаковые права; локальные
role-binding mutations в central mode возвращают `410 ROLES_DISABLED`.

## Маршруты

### Health и runtime

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/health/live` | Liveness процесса. |
| `GET` | `/health/ready` | Readiness собственной database и migrations. |
| `GET` | `/api/v1/runtime/branding` | Published safe branding document; поддерживает `If-None-Match`, возвращает ETag и `Cache-Control`. |
| `GET` | `/api/v1/runtime/services` | Active services с approved declarations для navigation. |

Runtime endpoints не содержат credentials, private integration configuration или audit history. Если branding revision не опубликована, branding отвечает documented not-found error, а consumers сохраняют свои defaults.

### Session

| Метод | Путь | Назначение |
|---|---|---|
| `POST` | `/api/v1/auth/login` | Проксировать credentials в central auth и вернуть validated panel session. |
| `GET` | `/api/v1/auth/me` | Вернуть validated subject, effective panel role и UI capabilities. |

### Пользователи и личные токены

| Метод | Путь | Назначение |
|---|---|---|
| `GET`, `POST` | `/api/v1/users` | Список/search или создание pending account в Central Auth. |
| `PATCH` | `/api/v1/users/{id}` | Изменить отображаемое имя; email действующей учётки не меняется. |
| `POST` | `/api/v1/users/{id}/status` | Отключить или восстановить; отключение отзывает sessions и personal tokens. |
| `POST` | `/api/v1/users/{id}/password-link` | Отменить прежнюю и отправить новую одноразовую setup-link. |
| `GET`, `POST` | `/api/v1/tokens` | Список metadata или выпуск scoped personal token; secret возвращается один раз. |
| `DELETE` | `/api/v1/tokens/{id}` | Отозвать personal token. |

Admin Panel проксирует эти операции в Central Auth с bearer пользователя и не
сохраняет password setup links, browser sessions, raw token secrets или копию
user directory.

### Registry, branding и audit

| Метод | Путь | Назначение |
|---|---|---|
| `GET`, `POST` | `/api/v1/services` | Получить список или создать registry entry. |
| `GET`, `PATCH` | `/api/v1/services/{service_key}` | Прочитать или изменить разрешённые entry metadata. |
| `POST` | `/api/v1/services/{service_key}/approve` | Одобрить pending declaration. |
| `POST` | `/api/v1/services/{service_key}/disable` | Отключить service entry. |
| `POST` | `/api/v1/services/{service_key}/retire` | Retire service entry. |
| `GET`, `POST` | `/api/v1/services/{service_key}/checks` | Прочитать или запустить bounded declared-capability check. |
| `GET`, `POST` | `/api/v1/branding/revisions` | Получить revisions или создать draft. |
| `POST` | `/api/v1/branding/revisions/{revision}/publish` | Опубликовать draft revision. |
| `POST` | `/api/v1/branding/revisions/{revision}/withdraw` | Withdraw revision. |
| `GET` | `/api/v1/audit-events` | Прочитать sanitised append-only audit events для covered actions. |

Covered actions: `service.approved`, `service.checked`, `branding.published` и `branding.withdrawn`. Registry create/update/status и role-binding mutations пока не создают complete audit evidence; это known closure gap, а не обещание API.

### Legacy role bindings

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/role-bindings` | Прочитать исторические записи для совместимости. |
| `POST` | `/api/v1/role-bindings` | Legacy only; `410 ROLES_DISABLED` в central mode. |
| `DELETE` | `/api/v1/role-bindings/{id}` | Legacy only; `410 ROLES_DISABLED` в central mode. |

Новые пользовательские роли не назначаются. Поля и записи сохраняются один
релиз только для совместимости исторических данных.

## Ошибки

API возвращает structured JSON errors. Типовые результаты: `401` для отсутствующего/невалидного bearer token, `403` для недостаточной panel role, `404` для отсутствующего resource, `409` для invalid state/conflict и `412` для stale optimistic-concurrency precondition. Error responses исключают bearer tokens, passwords, private signing material и external response bodies.

## Граница интеграций

Registry check — не generic remote request. Caller указывает allowed capability, а server code выводит method и fixed route из local catalog и проверяет только active approved declaration. Arbitrary remote URL, method, path, headers и body не являются API surface.

## References

- [OpenAPI 3.1](../openapi/openapi.json)
- [Architecture](ARCHITECTURE.md)
- [Security](SECURITY.md)
- [Runtime](RUNTIME.md)
