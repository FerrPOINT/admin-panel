# План 010 — public_ui_url в декларациях (ADR-0007, каталог v1.2)

## Контекст
За TLS-фасадом `*.base.localhost:7743` каталог отдаёт `ui_url =
integration_base_url` (loopback), и переключатель сервисов уводит
пользователя с фасада на plain-HTTP адреса. ADR-0007: декларация получает
опциональный `public_ui_url`, каталог предпочитает его.

## Изменения
1. Миграция `0005_declaration_public_ui_url.sql` — `public_ui_url TEXT NULL`.
2. Domain: `Declaration.public_ui_url`, `validate_public_ui_url`
   (http(s)-origin, без credentials/path; пустая строка = нет override).
3. Infra: SELECT/INSERT колонка; backfill не требуется.
4. API: `DeclarationInput.public_ui_url` (serde default), валидация в
   create/patch, каталог v1.2 `ui_url = public_ui_url ?? integration_base_url`.
5. OpenAPI регенерирован; docs/API.md §7.3 обновлён.

## Совместимость
Поле опционально; старые декларации и потребители v1.1/v1.0 не затронуты.

## Проверка
cargo fmt+clippy -D warnings+test (17 ok / 0 FAILED, вкл. 2 новых теста
каталога v1.2); live: TLS-декларации задают `public_ui_url`, каталог и
свитчер остаются на фасаде.
