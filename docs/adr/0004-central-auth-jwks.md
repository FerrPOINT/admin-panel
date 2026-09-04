# ADR-0004: Центральная аутентификация через JWKS auth-server

## Status

Accepted

## Context

Платформе нужен единый источник identity и credential lifecycle. Разделение между сервисами должно не допустить расхождения пользователей, паролей и сессий, но при этом Admin Panel обязан самостоятельно авторизовать доступ к конфигурационным ресурсам. В v1 нет gateway, который мог бы выступить единственной точкой проверки.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| Локальные пользователи в каждом сервисе | независимый запуск | дублирование identity, паролей и политики сессий |
| Introspection на каждый запрос | всегда свежая сессия | сетевой hot path и доступность auth-server влияет на каждое чтение |
| Auth-server JWT + JWKS | единый issuer, локальная проверка подписи, масштабирование | требует key rotation и строгой claim policy |

## Decision

`auth-server` остаётся единственным владельцем identity, login, session, credential и signing key lifecycle. Он выпускает короткоживущие JWT и публикует JWKS. Admin Panel валидирует `iss`, `aud`, время действия, подпись и допустимые claims по кэшированному JWKS, затем применяет собственную resource/capability authorization policy. Для service-to-service используются отдельные machine identities и минимальные scopes.

Admin Panel не хранит пароли, refresh token, сессии или копию user directory. В audit сохраняется минимальный внешний actor subject reference.

## Consequences

- Нет дублирования source of truth identity между сервисами.
- Проверка токена не создаёт синхронную зависимость от auth-server на каждый request; rotation ключей требует refresh/cache policy.
- Недоступность/невалидность JWKS для нового ключа ведёт к fail-closed, а не к пропуску проверки.
- UI может скрывать недоступные действия, но окончательная авторизация всегда происходит на API.

## Related

- `docs/API_STANDARDS.md`
- `docs/FRONTEND_ARCHITECTURE.md`
- `docs/LOGGING_STANDARDS.md`
