# План документации выделения AI Hub

Дата: 2026-10-09. Пользователь поручил документы/design, не runtime migration.
Источник текущих экранов /ai и API: AI_PROFILE_V1, исходный frontend и own ai-runtime.
Карта назначения каждого поля/operation/storage опубликована в AI Hub ADMIN_HANDOFF_V1.
Admin после принятого cutover сохраняет status/service link. /ai — контекстный переход
в Hub; settings/credentials/model context/publication/budget editor больше не Admin.
До этой вехи текущие маршруты, migrations, данные, authorization и runtime сохраняются.
Проверки: ссылка/handoff source SHA, owner boundaries, UI redirect preserves Namespace;
реальные backup/import/grants/financial/cutover/rollback gates выполняются позднее.

Повторная документационная проверка: target signed transport, exact revision UUID и cost/tariff history описаны в Hub; opaque proof привязан к target authorization/config, old proof только history. Обновление /ai runtime или выдача новых service grants не выполняются.

READY-01–05 target documentation resolved: no future-stage proof prerequisite, no fake pricing reference, full financial/context snapshots and strictly scoped service transport. Consumer/native/SQL/backup/financial/cutover acceptance remains future S7.

Поставка повторного ревью 2026-10-10: Hub
`b266e158b33e6cc70e02a9df3ba1d21c74b983af`; ссылка на source-bound отчёт добавлена
в extraction contract. Публикация документации не переключает Admin /ai или consumer.
