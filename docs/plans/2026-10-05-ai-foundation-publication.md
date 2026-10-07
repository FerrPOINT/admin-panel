# Публикация AI/messaging основы Admin

## Цель

Подготовить существующую основу и BF-006 вместе. Runtime client должен
принимать только два точных deployment endpoints: `http://ai-backend:8760/`
и совместимое `http://ai-runtime:8760/`. Credentials, index и пользовательские
правки текущего checkout не переносятся в Git и сохраняются на месте.

## Состав

- Messaging consumer, bounded feed/status и graceful workers.
- AI registry, per-model context, CAS, immutable pending revision и outbox.
- Отдельный encrypted ai-runtime, managed authorization, scoped readback,
  библиотечные адаптеры и durable acceptance ledger.
- Утверждённая `/ai` консоль: подключения, каталог, drafts и контекст;
  integration поверх существующей оболочки main.
- Контракты/ADR, миграции 0006–0010 без изменения их существующих identities.
- Точный опубликованный Base SDK SHA как обязательная cross-repo dependency.

## Порядок и проверки

1. Сохранить поведение текущего main: telemetry, request IDs, error envelopes,
   routes и пользовательские flows. Исключить сторонние изменения каталога.
2. Проверить самостоятельность Docker build context и pinned Base dependency.
3. Rust 1.88: fmt, tests, clippy и dedicated PostgreSQL integration.
4. Frontend: frozen install, typecheck, tests, lint, formatting, OpenAPI и build.
5. Браузер: маршрут, Auth boundary, context drafts, ошибки, narrow viewport,
   сохранение изменений при конфликте и уходе со страницы.
6. Повторное review, CI на точном PR head и проверка сохранности workspace.

## Граница готовности

Это публикация основы, а не завершённый AI inference. HTTP admission и
публикация модели остаются заблокированными до реализации producer verification
и реальной capability acceptance. UI честно показывает недоступные действия.
Каталог не доказывает доступ к модели. Платные запросы в этой поставке не
выполняются; ChatGPT/OpenRouter live acceptance требует отдельных credentials.
Нет автоматического fallback или переноса credentials SDLC2 в другой стенд.

Для обычного запуска AI/messaging optional и выключены без deployment config.
Миграции additive, но rollback приложения не удаляет новые таблицы. Уже
применённые миграции не переписываются и не объединяются. Новое размещение
сервиса требует собственной конфигурации/state/key; PR не обновляет runtime
SDLC1 и не переинициализирует рабочие хранилища.

## Совместимость действующей установки

SQLx ledger сохраняется без переписи. Startup принимает только точный SHA-384
LF/CRLF варианта прежнего SQL; остальные изменения по-прежнему блокируют запуск.
См. [ADR 0021](../adr/0021-migration-line-ending-compatibility.md). Исторический
пустой EOF миграции 0007 сохраняется: форматирование SQL не меняет её checksum.

Воспроизводимая интеграция: `python scripts/test_ai_foundation.py`; отдельные БД
проверяют CAS/audit, все десять миграций, consumer/feed, retention и capacity.
Браузерные проверки используют явные OIDC/API fixtures; они не заменяют live SSO
или provider acceptance. SDK transport проверяется отдельными CI scenarios Base.
