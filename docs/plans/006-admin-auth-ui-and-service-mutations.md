# План: Admin Panel — Auth UI и мутации сервисов

Дата: 2026-09-05
Статус: утверждён к выполнению
Связано: отчёт аудита 2026-09-05 (P0 «фронт не авторизуется»), C1 (auth backend, `598d8e3`), C6 (OpenAPI)

## Контекст

После включения fail-closed auth (C1) фронт Admin Panel перестал получать данные:
API-клиент не отправляет `Authorization`, экрана логина нет, все gated-запросы дают 401.
Мутации сервисов (approve/disable/retire) в UI отсутствуют; страница role-bindings не сделана.

Границы: central auth (`7701`) — внешний identity-провайдер; Admin Panel не владеет
паролями/сессиями, только валидирует ES256-JWT через JWKS и мапит panel-роли через
`role_bindings` (см. ADR по auth).

## Что делаем

### 1. Backend: login-прокси и /auth/me

- `POST /api/v1/auth/login` `{email, password}` → central `POST /auth/login`;
  при успехе вернуть `{access_token, expires_in, role: panel_role}`.
  Панель не хранит пароли, токен не кукует (SPA хранит в памяти + sessionStorage).
- `GET /api/v1/auth/me` (Bearer) → `{subject, central_role, panel_role}` — источник
  истины о правах для UI (какие кнопки показывать).
- Ошибки central → 401 `INVALID_CREDENTIALS`, без деталей central наружу.
- Тесты: login ok / central 401 / me без токена 401 / me с member-токеном (нет
  биндинга → panel_role none).

### 2. Фронт: AuthProvider + api-client

- `AuthContext`: `{status, token, role, login, logout}`; токен в `sessionStorage`
  (`base.admin.token`), при загрузке — попытка `/auth/me` по сохранлённому токену.
- `api-client`: `Authorization: Bearer` на все запросы кроме runtime/health/login;
  401 → событие logout + редирект на /login; 403 → toast «Недостаточно прав».
- Страница `/login`: email+пароль, ошибка при неверных, редирект back.
- Роутер: защищённые страницы при `!token` → /login; мутации видимы при
  `role >= operator`, bindings-страница при `role = admin`.

### 3. Страница role-bindings (admin)

Таблица bindings + форма создания (claim_name/claim_value/panel_role) через
`POST /api/v1/role-bindings`; ошибки дубликата показываются, не валидируются молча.

### 4. Мутации сервисов

- `/services` — кнопка «Добавить сервис»: форма (service_key, display_name,
  owner_team, base_url, capabilities, contract_version) → POST + PATCH-декларация.
- `/services/:key` — кнопки: Approve (POST /approve, с 422-ошибками), «Новая
  декларация» (PATCH с If-Match), Disable/Retire (PUT status).
- Во всех — optimistic disabled-состояния и понятные ошибки из error-envelope.

### 5. Settings → живые данные

Блоки: auth (issuer/JWKS из конфига, read-only), runtime-кэш (фактические ETag
branding/services из HEAD-запросов), версии (из /health/ready), ссылка на OpenAPI.

### 6. Тесты

- vitest: AuthContext (login/logout/401-logout), api-client (заголовок, 401-flow),
  страницы bindings/services-мутаций (моки react-query).
- e2e: мок `/auth/login`+`/auth/me`; сценарий логин → страницы живые → «Опубликовать»
  брендинг (мок POST) → logout. Плюс негативный: неверный пароль → ошибка.

### 7. Верификация

- Полный backend-gate (docker rust:1.88: fmt/clippy/test) + regen OpenAPI.
- `pnpm test`, `pnpm test:e2e`, `pnpm build`; CI зелёный.
- Live: логин реальным central-юзером на 7772, публикация брендинга rev3, approve
  test-service, проверка audit-событий.

## Не входит

- Собственные пароли/сессии/refresh в Admin Panel (владеет central).
- Изменение central auth-server.
- Health-проверки каталога (отдельный план, F2).
