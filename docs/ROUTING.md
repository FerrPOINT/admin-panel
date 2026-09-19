# Маршрутизация Base Admin Panel

## Статус документа

Документ описывает текущие browser routes и их security boundaries. Маршрут не является разрешением расширять API или создавать remote-control flow: changes проходят review вместе с route-level state, authorization и browser coverage.

## Группы маршрутов

| Группа | Auth | Layout | Поведение |
|---|---|---|---|
| Login | Public | Minimal | Запускает Central Auth Authorization Code + PKCE; token не попадает в URL или persistent browser storage. |
| Protected app | Central bearer | `AdminShell` | Основные administrative pages для любого активного пользователя. |
| Not found | Any | Minimal | 404 without redirect into protected data. |

## Текущие страницы

| Route | Access | Назначение | Разрешённые действия | Не делает |
|---|---|---|---|---|
| `/login` | Public | Вход через Central Auth | Начать Authorization Code + PKCE | Local password form, registration, token storage |
| `/sso/callback` | Public callback | Завершить PKCE | Принять code и открыть исходный маршрут | Показывать token в URL/storage |
| `/` | Viewer+ | Control-plane overview | Переходы к доступным разделам | Управление внешними сервисами |
| `/branding` | Viewer+ | Published branding и revisions | Operator creates drafts; operator publishes under current API policy | Arbitrary CSS/JS/HTML |
| `/services` | Viewer+ | Service registry | Operator creates and updates declarations | Inline arbitrary endpoint edit |
| `/services/:serviceKey` | Viewer+ | Service declaration and checks | Operator runs allowed check; operator approves/disables/retires under current API policy | Raw HTTP console or product CRUD |
| `/audit` | Viewer+ | Append-only audit trail | Filter and inspect sanitised records | Edit/delete audit row |
| `/runtime` | Viewer+ | Runtime branding/catalog observability | Read current safe projections | Modify product runtime |
| `/settings` | Viewer+ | Local panel settings view | Read supported UI settings | Edit central auth identity |
| `/users` | Authenticated | Центральные пользователи | Create, rename, resend setup, disable/restore | Store password or directory copy |
| `/tokens` | Authenticated | Личные API-токены | Create scoped token, show once, revoke | Store raw token secret |
| `*` | Any | Not found | Explicit navigation back | Automatic redirect to protected overview |

## Responsive behavior

Desktop displays the persistent sidebar and dense operational tables. At
`375×812`, navigation collapses behind the menu, tables use mobile-safe layouts,
and user/token actions remain keyboard accessible without horizontal overflow.

## Route guard rules

1. Browser keeps access token in memory only and asks protected API for effective capabilities.
2. `401` clears transient client state and navigates to `/login`; no token is retained in URL.
3. `403` renders a forbidden state rather than hiding a server authorization failure.
4. `404` renders a route-level not-found state.
5. UI visibility is convenience, never the authorization boundary; Admin API verifies every protected action.
6. Mutation views surface API precondition/conflict responses and never silently overwrite server state.

## References

- [API](API.md)
- [Architecture](ARCHITECTURE.md)
- [Security](SECURITY.md)
- [Frontend architecture](FRONTEND_ARCHITECTURE.md)
