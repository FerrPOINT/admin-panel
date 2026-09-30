# Готовность Admin Panel

## Объём

Закрыть A02 аудита: `/health/ready` остаётся status-only endpoint. Frontend nginx
проксирует точный путь к Admin API, экран настроек не пытается читать пустое 200
как JSON. Учётные записи, роли, migrations и backend API не меняются.

## Состояния

- Pending: «Проверка…», повторная проверка заблокирована.
- HTTP 200: «Готова».
- HTTP 503: «Не готова».
- Сетевая ошибка, другой HTTP error или HTML SPA fallback: «Недоступна».
- При повторной проверке и ошибке предыдущий успех не отображается.

## Приёмка

Unit-проверки пустого 200, 503, network failure, retry, HTML fallback и pending.
Локальные frontend lint/typecheck/unit/build. Live-проверка через production
nginx, без подмены API, на mobile/desktop и во всех темах. Недоступность backend
проверяется только в отдельном QA-проекте, не на пользовательском стенде.

## Результат

- Frontend: 72 unit-теста, lint/semantic lint, typecheck и production build.
- Backend: полный fmt/clippy/test gate, 29 tests с реальным PostgreSQL.
- Production nginx: пустой HTTP 200 корректно отображается как «Готова».
- Live Chromium без mock API: 375/1920/2560 px, light/gray/dark, full-page
  screenshots, без overflow, console errors и serious/critical axe.
- Остановка только `admin-postgres` из изолированного QA Compose-проекта
  возвращает HTTP 503 и «Не готова»; после восстановления retry показывает
  «Готова». Контейнер проверяется по точным project/service labels и ID,
  восстановление выполняется в finally. Пользовательские volumes не удаляются.
