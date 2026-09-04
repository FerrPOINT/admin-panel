# Безопасность SDLC Admin Panel

## 1. Цели и границы доверия

Admin Panel управляет чувствительными метаданными платформы, поэтому защищает три независимые границы:

1. пользователь -> Admin API: проверка central-auth JWT и роли панели;
2. Admin API -> PostgreSQL: доступ только к собственной БД;
3. Admin API -> endpoint интеграции: только одобренная read-only capability по жесткому allowlist.

Панель не является источником identity, secret manager, API gateway или remote execution plane. Нарушение этих границ считается security defect.

## 2. Аутентификация: central auth

- Единственный доверенный issuer — существующий central auth на `7701`.
- Admin API принимает только bearer access JWT, подписанный `ES256`.
- Для каждого токена проверяются signature, `alg`, `kid`, issuer, audience, `exp`, `nbf` (если есть), формат `sub` и обязательные claims политики.
- JWKS берется только с заранее настроенного trusted origin. TLS verification обязательна.
- JWKS cache имеет ограниченный TTL. При неизвестном `kid` допускается однократное controlled refresh; токен не принимается, пока ключ не найден и не проверен.
- Не допускаются `none`, HMAC, algorithm confusion, unsigned token, локальный fallback-token или `alg` из пользовательского ввода.
- Admin Panel не выпускает, не refresh-ит, не отзывает JWT и не сохраняет access/refresh token.

## 3. Авторизация

| Операция | Минимальная роль |
|---|---|
| Просмотр registry, branding revisions, audit | `platform_viewer` |
| Создание declaration/draft, запуск check | `platform_operator` |
| Одобрение/disable/retire сервиса, publish branding, role bindings | `platform_admin` |

- Роль вычисляется на сервере из утвержденных JWT claims и локального `role_bindings`.
- UI скрывает недоступные действия, но не является authorization boundary.
- Все mutations проверяют role в application layer до repository/HTTP adapter.
- `platform_admin` не получает автоматически права предметного администратора CI-CD, task-tracker, wiki или fleet-control.
- Изменение role binding требует audit event и optimistic concurrency.

## 4. Защита реестра и внешних вызовов

### 4.1. Против SSRF и произвольного remote control

- `integration_base_url` допускает только HTTPS absolute origin без userinfo, query, fragment и path.
- Hostname/port/CIDR сверяются с deployment allowlist; localhost, loopback, link-local, multicast, unspecified и private ranges запрещаются, кроме явно утвержденного private deployment policy.
- DNS-resolve проверяется перед соединением и повторно после redirect; DNS rebinding не допускается.
- Redirect отключен либо допускается только однократный HTTPS redirect на allowlisted host; в v1 предпочтительно не следовать redirect.
- Метод и путь берутся только из `capability_catalog`; клиент не передает URL, path, method, headers или body проверки.
- Только approved declaration активного сервиса может быть проверена.
- Capability catalog v1 read-only и не содержит write/execute/proxy/secret operation.
- Таймауты, response-size limit, rate limit и bounded concurrency обязательны.
- В БД/логах сохраняются только итог, HTTP status и санитизированная summary, но не body/headers внешнего ответа.

### 4.2. Входные данные

- DTO применяют strict schema: дополнительные поля запрещены.
- `service_key`, URLs, claim bindings, enum status и цвета валидируются allowlist/regex/schema.
- Все SQL-запросы параметризованы; dynamic SQL и строковая интерполяция запрещены.
- JSONB branding проверяется до сохранения; HTML, CSS, JS, data URL, произвольная вложенность и ключи вне schema отклоняются.
- Ограничение длины применяется ко всем строковым полям, filter/query и idempotency key.

## 5. Runtime branding

Публичный GET бренда не дает права на mutation и содержит только safe presentation fields. Защиты:

- только JSON, `X-Content-Type-Options: nosniff`;
- URL разрешаются только по схеме `https` (или явно утвержденному relative path policy);
- цветовые поля — строго `#RRGGBB`;
- `ETag` строится из canonical document и revision; он не раскрывает секрет или внутренний ID;
- `Cache-Control: public, max-age=60, must-revalidate` ограничивает stale window;
- endpoint не перенаправляет клиента в сторонние origin и не делает external fetch;
- consumer валидирует документ перед применением и всегда имеет собственный fallback default.

В v1 нет CDN, cache gateway или центральной инъекции HTML/CSS в потребители.

## 6. Transport и browser security

- В production обязателен HTTPS, HSTS и TLS verification для исходящих запросов.
- CORS — строгий allowlist UI origins. Публичный branding endpoint допускает только policy-defined origins; `*` не сочетается с credentials.
- Cookies не используются как auth-механизм Admin API. Если UI хранит access token, он держится только в памяти, не в localStorage/sessionStorage.
- Для browser mutations применяются `Authorization` header, Origin validation и при выбранной cookie-модели отдельная CSRF policy. Cookie fallback не планируется v1.
- Заголовки: `Content-Security-Policy`, `frame-ancestors 'none'`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, `Permissions-Policy: geolocation=(), microphone=(), camera=()`, `X-Content-Type-Options: nosniff`.
- CSP UI запрещает inline script и подключения к неразрешенным origin.

## 7. Секреты и конфигурация

- Секреты передаются только через deployment secret store/env injection; в документации и `.env.example` используются placeholders.
- Запрещено сохранять в БД/API/audit/logs: DB password, signing key, bearer token, JWKS private material, service API key, Authorization header, cookie, секреты pipelines.
- Конфигурация trusted issuer, audience, JWKS URL, DB URL, CORS origin и integration allowlist является deployment configuration, но значения не выдаются через публичные API.
- Ротация secret/JWKS выполняется владельцем central auth/deployment. Панель безопасно обновляет public JWKS по policy, но не владеет ключами.

## 8. Аудит, логи и приватность

- Аудируются: создание/изменение declaration, approve/reject/disable/retire, check trigger/result, branding draft/publish, role binding mutation, authorization deny для чувствительных действий.
- Audit events append-only; изменения пишутся как sanitизированный diff разрешенных полей.
- Логи используют `request_id`, action, role, service_key и HTTP status. Токены и payload внешнего сервиса redacted by design.
- Срок хранения аудита и логов определяется operational policy; удаление отдельных audit records через UI запрещено.
- IP и subject хранятся только если это разрешено политикой privacy/retention развертывания.

## 9. Rate limiting и устойчивость к злоупотреблению

| Класс | Базовое направление |
|---|---|
| Public branding read | Отдельный лимит по IP/origin, не влияющий на admin mutations. |
| Authenticated read | Лимит по subject + IP. |
| Registry/branding mutation | Строгий лимит по subject, idempotency и optimistic concurrency. |
| Integration checks | Низкий лимит per service и global bounded concurrency. |

Конкретные численные значения выбираются нагрузочным тестом перед запуском. `429` возвращает `Retry-After`. Проверка интеграции не может быть использована как сетевой сканер.

## 10. Инциденты

1. Ограничить/отключить скомпрометированную интеграцию через `disabled`; это не отправляет команду сервису.
2. Отозвать/изменить role bindings или доступ в central auth по зоне владения.
3. Сохранить audit correlation и санитизированные логи.
4. Ротировать скомпрометированные deployment secrets у их владельца.
5. Проверить published branding revision; при необходимости опубликовать новую controlled rollback revision.
6. Не удалять аудит и не выполнять удаленные действия через Admin Panel как часть реагирования.

## 11. Проверки до релиза

- Негативные JWT tests: algorithm confusion, неверный issuer/audience, expired/nbf, unknown `kid`.
- RBAC matrix на каждый защищенный endpoint.
- SSRF tests: loopback/private/link-local, DNS rebinding model, redirects, userinfo URL, unexpected port.
- Capability allowlist tests: нельзя передать произвольный method/path/body/header.
- Schema/XSS tests для branding URL/colors/extra fields.
- Audit tests: mutation атомарно создает event без секретов.
- Dependency/container scans, non-root runtime, migration tests на чистой БД.

## References

- `docs/ARCHITECTURE.md`
- `docs/API.md`
- `docs/DATA_MODEL.md`
- `docs/RUNTIME.md`
