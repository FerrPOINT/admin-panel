# Frontend Architecture

## 1. Назначение

Admin Panel -- SPA для операторов платформы. Она управляет **платформенной конфигурацией**: реестром интеграций, разрешёнными capability, параметрами UI/runtime и публикацией конфигурационных ревизий. `auth-server` остаётся владельцем только identity: учётных записей, сессий, credential lifecycle и подписанных токенов. Admin Panel не хранит пароли и не реализует собственную базу пользователей.

Стек MVP: React + TypeScript + Vite, React Router, TanStack Query, Tailwind и локальные совместимые shadcn-style primitives. SSR, CDN и gateway не входят в v1.

## 2. Слои и зависимости

```
src/
  app/       # providers, router, bootstrap runtime config
  pages/     # маршруты и композиция экранов
  widgets/   # крупные самостоятельные блоки
  features/  # сценарии изменения конфигурации
  entities/  # интеграция, capability, config revision
  shared/    # api client, ui primitives, lib, config, types
```

Допустимое направление импортов: `app -> pages -> widgets -> features -> entities -> shared`. Нижний слой не импортирует верхний; feature не импортирует внутренности другой feature. Каждый сегмент публикует ограниченный public API через `index.ts`.

## 3. Страницы v1

| Маршрут | Назначение | Источник данных |
|---|---|---|
| `/` | обзор состояния конфигурации и последних публикаций | Admin Panel API |
| `/integrations` | реестр потребителей и интеграционных контрактов | Admin Panel API |
| `/integrations/:id` | capability, scopes, параметры и статус контракта | Admin Panel API |
| `/configuration` | просмотр активной ревизии и безопасных значений по умолчанию | Admin Panel API |
| `/configuration/revisions/:id` | diff, аудит, публикация или откат ревизии | Admin Panel API |
| `/audit` | журнал административных изменений | Admin Panel API |
| `/access-denied` | отсутствие нужной роли без раскрытия конфигурации | локальное состояние |

Страницы identity management в v1 не создаются: они принадлежат `auth-server`.

## 4. Runtime configuration

При загрузке приложение читает минимальный публичный bootstrap (base URL API, issuer, JWKS audience/клиентские параметры, feature flags) и затем получает авторизованную конфигурацию через Admin Panel API. Runtime-конфигурация кэшируется по `ETag`; при `304 Not Modified` сохраняется проверенное значение, при ошибке используются только документированные безопасные defaults.

Публичный bootstrap не содержит секретов, внутренних адресов, service credentials, токенов, полномочий или приватных integration settings. Значение, отсутствующее в конфигурации, не включает рискованное поведение: consumer должен выбрать безопасное default и отразить деградацию в health/diagnostics.

## 5. Состояние и API

- TanStack Query владеет данными API, ключи строятся по ресурсу и revision/version.
- Mutation публикует или меняет черновик через API, инвалидирует затронутые revision/интеграции и показывает `409` как конфликт ревизии, а не перезаписывает данные.
- Zustand допускается только для UI-состояния (навигация, фильтр, несохранённые локальные предпочтения), но не для токенов или серверной конфигурации.
- API-клиент добавляет корреляционный идентификатор, нормализует envelope ошибок и централизованно обрабатывает `401`/`403`.

## 6. Авторизация и UX

Frontend получает access token через согласованный flow `auth-server`; валидность токена проверяет backend. Скрытие кнопки не является авторизацией. На `401` приложение инициирует штатный login/refresh flow, на `403` показывает отсутствие права без деталей закрытого ресурса.

Формы разделяют draft и published state, показывают validation errors, причину конфликта и автора/время текущей ревизии. Опасные изменения требуют явного подтверждения. UI не предлагает «выполнить команду на сервисе», shell, URL произвольного webhook или иной удалённый произвольный контроль.

## 7. UI Shell Contract

Admin Panel follows the Base [UI Shell Standard](https://github.com/FerrPOINT/services-base/blob/main/docs/platform/UI_SHELL_STANDARD.md). `AppShell` owns the shared 264 px/72 px left navigation, 60 px header and full-width right work area. Routes use only `wide`, `reading/form` or `detail-with-aside`; they do not select Admin Panel-specific shell geometry.

- Overview, integration catalog, revision history, audit and health use `wide`.
  Filters and tables keep local overflow inside their own container.
- Integration and revision detail use `detail-with-aside`: a fluid primary column
  with a 320 px contextual rail. Draft/publish forms use `reading/form`, with
  only a 760 px inner column bounded without narrowing catalog or audit routes.
- Expanded desktop sidebar, compact tablet rail and mobile drawer preserve the
  same role-aware navigation order and active state. Drawer behavior includes
  keyboard focus handling, Escape close and focus restoration.
- Service/profile/theme controls stay in the single global header row. Page
  title, revision state, filters and publication actions live below it in
  page-owned header/action rows.
- Shell/layout changes require browser evidence at 375, 1440 and 2560 px:
  active navigation, header alignment, no document overflow and a keyboard
  drawer flow.

## 8. Качество

Тесты: Vitest + Testing Library для features и преобразований, MSW для контрактных ответов, Playwright для входа, просмотра конфигурации, публикации, отказа в доступе и fallback. Маршруты загружаются лениво; длинные таблицы виртуализируются при подтверждённой необходимости.

## Связанные документы

- `docs/API_STANDARDS.md`
- `docs/API_VERSIONING.md`
- `docs/PERFORMANCE.md`
- `docs/adr/0002-react-vite.md`
- `docs/adr/0005-runtime-config-etag.md`
