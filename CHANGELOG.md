# Журнал изменений

Все заметные изменения проекта фиксируются в этом файле.

Формат ориентирован на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/), а версии будут следовать [Semantic Versioning](https://semver.org/lang/ru/).

## [Unreleased]

### Fixed
- Подтверждение branding revision actions (#10); service switcher на mobile (#9).
- Управление пользователями: честная граница страниц каталога, скрытие устаревшего списка при ошибке, блокировка форм во время запросов и отдельное состояние повторной отправки письма.
- Журнал аудита: понятный фильтр действий и пользователей, точный `total` для фильтрованной пагинации без пустой последней страницы; увеличены цели checkbox, исправлены контраст и возврат фокуса в диалогах токенов.
### Added
- Workspace overview и audit UX (#8).
- CHANGELOG, CONTRIBUTING, THIRD_PARTY_NOTICES; LICENSE → FerrPOINT Proprietary v1.0.
### Fixed
- .pnpm-store (2888 файлов) выведен из git.
### Added

- Каталог v1.2 (ADR-0007): опциональный `public_ui_url` в декларациях; каталог отдаёт его как `ui_url` (fallback `integration_base_url`) — TLS-фасадные инсталляции сохраняют переключатель сервисов на публичном origin. Миграция `0005`, план `docs/plans/010`.
- Каталог v1.1: `GET /api/v1/runtime/services` отдаёт `ui_url` (capability `ui.render`; `null` для сервисов без UI) и `health` (статус health-worker: `healthy`/`unreachable`/`unknown`) — источник для платформенного переключателя сервисов. Миграция `0004`, план `docs/plans/009-runtime-catalog-health-ui-url.md`, тест `runtime_catalog_v1_1`.

- UI: вход через central auth (login-прокси + `AuthProvider`), мутации сервисов (создание, approve, декларации, disable/retire), страница привязок ролей, живые локальные настройки; README со скриншотами интерфейса.
- API: `POST /api/v1/auth/login`, `GET /api/v1/auth/me`, CRUD `/api/v1/role-bindings`; actor identity мутаций и аудита — реальный central-субъект.
- Опубликован OpenAPI 3.1 контракт (`openapi/openapi.json`, gen-openapi bin, CI drift-gate).

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
