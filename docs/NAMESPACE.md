# Реестр сквозных Namespace

Admin владеет отдельным Namespace bounded module. Прежний control-plane
integration check и role `manage_bindings` сохраняют свою семантику.
Решение: [ADR 0022](adr/0022-shared-namespace.md).

## API и данные

`GET/POST /api/v1/namespaces` — bounded catalog и original-key создание.
`PATCH /namespaces/{id}` изменяет display properties с expected revision CAS;
slug неизменяем. `/namespaces/{id}/context` возвращает metadata/bindings,
live counters читаются UI непосредственно у Tracker, Wiki и Forge.

`POST /namespaces/{id}/operations` сохраняет durable provision/attach/archive/
restore command до HTTP. `/namespace-operations/{id}` читает original outcome;
`POST /namespace-operations/{id}/reconcile` повторяет исходные owner commands.
Частичный результат остаётся pending с диагностикой, без удаления созданных
ресурсов и без выдачи новых resource UUID. Ready требует трёх owner ACK;
archived требует lifecycle ACK и drain. Restore использует прежние IDs.

Migration 0011 создаёт registry instance, namespaces, operations,
reservations/bindings и audit. ResourceRef уникален глобально; один Namespace
имеет по одному binding каждого kind. Конкурентное присвоение существующего
ресурса получает conflict. CAS действует и в registry, и в local projections.

## Доступ и настройка

`ADMINP_NAMESPACE_INSTANCE_ID` — immutable UUID реестра.
`ADMINP_NAMESPACE_OWNERS` — deployment JSON array из трёх объектов с `kind`,
`instance_id`, fixed `endpoint` и `token_file`. Supported kinds:
`tracker_project`, `wiki_space`, `git_group`. Это owner commands, не proxy.
UI не передаёт owner endpoints или credentials; токены читаются из private files.

Human policy остаётся shared-trusted. Используются существующие service scopes;
`namespace:*` scopes не вводятся. `ADMINP_NAMESPACE_MACHINE_SUBJECTS` и
service-account classification закрывают обычные human routes до mapping.
Человеческий PAT продукта не пересылается соседнему продукту.

`VITE_NAMESPACE_ENABLED=true` включает catalog, picker, wizard и overview.
Wizard либо создаёт ресурс, либо подключает явно выбранный existing UUID.
Responsible subject должен иметь active профиль у Tracker/Wiki; пустые
профили не создаются для обхода ошибки. Team не становится ACL.

Registry outage не останавливает confirmed product resources. Новые bindings
и смена ownership закрываются; ошибки owner counters показываются unavailable.
Раскатывается отдельный exact compatible cohort. Legacy SDK/skills pins,
pdlc-common, соседние stacks и Base-v3 gates сохраняются.
