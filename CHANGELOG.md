# Журнал изменений

## [Unreleased]

- Namespace cohort: стабильные refs, локальные binding projections и lifecycle guards; аддитивные migrations, совместимый rollback и отдельные execution v2 gates. Runtime-приёмка ещё не завершена.
  — AI/messaging foundation

- Добавлены AI registry/runtime, утверждённая консоль и messaging consumer. Внутренний клиент принимает canonical ai-backend и совместимый ai-runtime endpoint (BF-006). Реальный inference остаётся закрыт.

Все заметные изменения проекта фиксируются в этом файле.

Формат ориентирован на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/), а версии будут следовать [Semantic Versioning](https://semver.org/lang/ru/).

## [Unreleased]

- Namespace cohort: стабильные refs, локальные binding projections и lifecycle guards; аддитивные migrations, совместимый rollback и отдельные execution v2 gates. Runtime-приёмка ещё не завершена.

- Инструменты Docker и release CI согласованы: Rust 1.98.1, Node 26.10.0,
  pnpm 10.28.1. Frontend Docker устанавливает pnpm явно и не требует
  отсутствующий Corepack; отдельная проверка MSRV остаётся на Rust 1.88.0.

- Явный вход через SDLC возобновляет SSO после отменённого logout, включая
  браузеры без Navigation API; автоматические попытки сохраняют защиту PKCE.
- Повторная попытка входа убирает предыдущее сообщение об ошибке; реальные
  отказы Auth остаются видимыми и не смешиваются с отменой навигации.
- Тестовый JSDOM использует nwsapi 2.2.28 с защитой от рекурсивной проверки
  `:fullscreen`; полный frontend-набор выполняется одним worker без
  увеличения timeout. Production-зависимости не изменены.

- Каталог сервисов при выпуске личного API-токена загружается из общего
  Central Auth контракта; product-specific scopes больше не зашиты во frontend.

### Fixed

- Создание центрального пользователя записывает его UUID в `central_user.created`,
  чтобы аудит был связан с учёткой. Невалидный успешный ответ Central Auth
  возвращает 502 без ложного success audit; исходные ошибки upstream сохраняются.
- Допустимые названия PAT длиной до 100 символов переносятся внутри строки
  списка и не растягивают страницу на мобильном экране.
- Подтверждение отзыва PAT использует ConfirmDialog Base: отмена и Escape
  заблокированы во время запроса; ошибка доступна для retry и очищается для
  другого токена. Focus возвращается на исходную кнопку либо стабильную кнопку
  создания после успешного отзыва и обновления списка.
- Шапка использует общий `PlatformHeader` на всю ширину экрана; боковая
  навигация начинается под ней. Аккаунт и выход находятся в одном меню,
  email больше не дублируется в sidebar; drawer закрывается при переходе
  на desktop. Runtime-каталог и модель авторизации не изменены.
- Карточка сервиса использует общий contextual split из Base: правый rail
  320 px от 1024 px и линейный порядок контент/контекст на узких экранах.
- Escape и отмена подтверждения отключения/вывода сервиса возвращают фокус
  к исходной кнопке действия.
- Экран локальных настроек читает status-only readiness endpoint через production nginx: пустое 200 означает готовность, 503 и ошибка сети различаются, повторная проверка не показывает устаревший успех.
- `/auth/me` теперь сообщает фактические возможности: browser SSO сохраняет
  полный доступ ADR-0008, read-only PAT не получает mutation controls, а
  отключённые legacy role bindings не показываются как доступные.
- Каталог сервисов: поиск, фильтр и страницы по 20 записей; ошибки загрузки больше не показывают устаревшие строки, а создание сервиса и новой декларации сохраняет черновик и блокирует поля на время запроса. Управляющие кнопки в каталоге и карточке получили цель 40 px; Admin Playwright smoke обновлён под OIDC, историческая роль убрана из локальных настроек.
- Формы сервиса сохраняют `ui.render` и публичный URL при создании новой декларации, чтобы одобрение не скрывало UI-сервис из платформенного переключателя.
- PATCH сервиса и декларации атомарен, не даёт ложный 412 после записи и учитывает публичный URL в уникальности декларации.
- Подтверждение branding revision actions (#10); service switcher на mobile (#9).
- Управление пользователями: честная граница страниц каталога, скрытие устаревшего списка при ошибке, блокировка форм во время запросов и отдельное состояние повторной отправки письма.
- Журнал аудита: понятный фильтр действий и пользователей, точный `total` для фильтрованной пагинации без пустой последней страницы; увеличены цели checkbox, исправлены контраст и возврат фокуса в диалогах токенов.

### Added

- В форме личных API-токенов доступны продуктовые scopes
  `service-pulse:read/write`; Service Pulse при этом не добавляется в системный
  каталог сервисов.
- Workspace overview и audit UX (#8).
- CHANGELOG, CONTRIBUTING, THIRD_PARTY_NOTICES; LICENSE → FerrPOINT Proprietary v1.0.

### Fixed

- .pnpm-store (2888 файлов) выведен из git.
- Журнал аудита скрывает устаревшие события и счётчик при повторной загрузке или ошибке; фильтры и страница сохраняются для retry.

### Added

- Каталог v1.2 (ADR-0007): опциональный `public_ui_url` в декларациях; каталог отдаёт его как `ui_url` (fallback `integration_base_url`) — TLS-фасадные инсталляции сохраняют переключатель сервисов на публичном origin. Миграция `0005`, план `docs/plans/010`.
- Каталог v1.1: `GET /api/v1/runtime/services` отдаёт `ui_url` (capability `ui.render`; `null` для сервисов без UI) и `health` (статус health-worker: `healthy`/`unreachable`/`unknown`) — источник для платформенного переключателя сервисов. Миграция `0004`, план `docs/plans/009-runtime-catalog-health-ui-url.md`, тест `runtime_catalog_v1_1`.

- UI: вход через central auth (login-прокси + `AuthProvider`), мутации сервисов (создание, approve, декларации, disable/retire), страница привязок ролей, живые локальные настройки; README со скриншотами интерфейса.
- API: `POST /api/v1/auth/login`, `GET /api/v1/auth/me`, CRUD `/api/v1/role-bindings`; actor identity мутаций и аудита — реальный central-субъект.
- Опубликован OpenAPI 3.1 контракт (`openapi/openapi.json`, gen-openapi bin, CI drift-gate).

### Fixed

- Брендинг: после ошибки публикации повтор использует уже созданный черновик; форма блокируется на время запроса, а смена значений явно начинает новую ревизию.

## [1.0.0] — 2026-09-05

### Added

- Backend v1 (Rust workspace: api/app/domain/infra/shared/server/migration): health live/ready,
  branding revisions (draft→publish, ETag/If-Match), service registry CRUD с approvals
  и версионированием, role bindings, audit events, миграции 0001–0002.
- Публичный runtime-контракт для продуктов платформы: `GET /api/v1/runtime/branding`
  (ETag, max-age=60, 304) и `GET /api/v1/runtime/services` (каталог активных сервисов).
- Central auth: JWKS-проверка bearer-токенов (auth-server 7701), fail-closed middleware;
  мутации — PlatformOperator+, role-bindings/статусы — PlatformAdmin; локальные
  role_bindings как мост при отсутствии роли в central token.
- Frontend v1 (React 19 + @sdlc/ui): overview, branding, revisions, services,
  audit, runtime, settings; ServiceSwitcher в сайдбаре.
- Docker: umbrella-сборка (context /opt/dev/sdlc), зонтик 7771/7772/7773.
- CI: fmt/clippy/test backend, lint/typecheck/test/build frontend, compose-config gate.

### Security

- Мутации admin API без валидного central-токена отклоняются (401/403).
- Опубликована политика ответственного раскрытия уязвимостей.

## Документация AI Hub — 2026-10-09

Описано будущее выделение раздела /ai; текущие endpoints/runtime/data не переключены.

## 2026-10-09 — Уточнение handoff AI Hub

Target consumer signing/revocation и currency-aware pricing согласованы с Hub `45508cb53430c2f85d11a45b973672da81d36e84`. Текущий /ai и ai-runtime сохраняются до отдельного cutover; документационный status не равен main/runtime integration.
