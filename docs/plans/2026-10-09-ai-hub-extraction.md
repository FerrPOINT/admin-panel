# План документации выделения AI Hub

Дата: 2026-10-09. Пользователь поручил документы/design, не runtime migration.
Источник текущих экранов /ai и API: AI_PROFILE_V1, исходный frontend и own ai-runtime.
Карта назначения каждого поля/operation/storage опубликована в AI Hub ADMIN_HANDOFF_V1.
Admin после принятого cutover сохраняет status/service link. /ai — контекстный переход
в Hub; settings/credentials/model context/publication/budget editor больше не Admin.
До этой вехи текущие маршруты, migrations, данные, authorization и runtime сохраняются.
Проверки: ссылка/handoff source SHA, owner boundaries, UI redirect preserves Namespace;
реальные backup/import/grants/financial/cutover/rollback gates выполняются позднее.
