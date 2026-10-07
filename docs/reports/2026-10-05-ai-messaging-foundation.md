# Приёмка AI/messaging foundation и BF-006

Поставка основана на `main` Admin. Зависимость Base закреплена на
`8a00cc9a3ed0732c9ae77140d2694d629ee8082a` из
[Base PR #141](https://github.com/FerrPOINT/services-base/pull/141).
Порядок включения: Base, затем Admin. Канонические checkout, их индексы,
локальные конфиги и действующие runtime этой поставкой не изменяются.

## Исправления и проверенные границы

- BF-006: runtime client принимает только точные deployment endpoints
  `http://ai-backend:8760/` и `http://ai-runtime:8760/`. Другие hosts, ports,
  paths, userinfo, query и fragment отклоняются. Redirect, proxy и retry
  выключены. Service token читается из отдельного защищённого файла.
- Startup сохраняет SQLx validation и принимает только точные SHA-384
  варианты прежнего SQL с LF/CRLF. Исторические checksums не переписываются;
  изменение SQL отклоняется. Проверены все десять миграций на историческом
  смешанном ledger и новой установке, включая повторный запуск.
- Сохранены telemetry, request ID, прежние routes и error envelopes main.
  AI/messaging выключены без явной deployment configuration.
- AI drafts сохраняют контекст: `256` в UI означает `256000` в API.
  CAS conflict не уничтожает локальный draft; уход со страницы требует
  явного решения пользователя. Verification и activation честно закрыты
  до реализации доверенного capability evidence producer.
- Narrow WebKit select исправлен без обрезания страницы; facts используют
  собственную scoped grid вместо отсутствующих global styles.

## Фактические проверки

- Rust 1.88: workspace tests, fmt, clippy с `-D warnings`, генерация OpenAPI.
- Отдельные PostgreSQL БД: migration compatibility, immutable AI revision,
  publication CAS/audit/replay/restart, audit pagination, atomic registry
  patch, event feed pagination/retention и capacity sample.
- Отдельный NATS: семь typed SDK fixtures, настоящая redelivery после
  commit-before-ACK, inbox/feed deduplication и подтверждение всех сообщений.
  API работает через настоящий router и PostgreSQL; central PAT auth —
  явная fixture. Проверены 401/403, pagination/DTO и отказ при недоступной БД.
- Frontend: typecheck, lint/semantic classes, formatting, 93 unit tests,
  OpenAPI drift/compatibility и production build.
- Браузер: 9 cases в Chromium/Firefox/WebKit. Responsive case проверяет
  320, 375, 767, 768, 1279, 1280, 1440, 1920 и 2560 px без горизонтального
  overflow. Проверены defaults, disabled activation, save, stale revision
  и leave guard. Screenshot и metadata включены в screenshot manifest.
- README validator и его 8 tests; pinned Base source verification.
- Временные QA projects очищены по точным manifests без удаления volumes.
  Рабочие БД, credentials провайдеров и Docker socket им не передавались.
  Платных запросов: 0. Workspace `Check` и `Status`: exit 0, 19 своих
  сервисов running/healthy; это только проверка сохранности окружения.

## Что эта поставка не доказывает

HTTP inference admission и trusted verification producer ещё не реализованы;
это основа, а не завершённая интеграция ChatGPT/OpenRouter. Не проверены live
provider login/refresh, доступ к точным model IDs и платный inference.
Браузерные OIDC/API fixtures не заменяют живую SSO приёмку. SDK publisher
fixtures не заменяют полный CI/CD producer/control harness. Полный SDLC,
production capacity, deployment/recovery и двусторонняя приёмка стендов
остаются отдельными этапами. Восемь прежних direct Docker invocations Base
вне нового harness также не объявляются исправленными этой поставкой.

CI и unresolved review threads проверяются на точном опубликованном PR head
перед переводом PR из Draft. Runtime activation и merge не выполняются
в рамках подготовки merge-ready.
