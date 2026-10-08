# Сквозной Namespace PDLC

Поручение пользователя от 2026-10-08: реализовать проверенный план N0–N8.
Namespace связывает один Tracker Project, один основной Wiki Space и одну
Forge Git Group. В группе допускаются несколько hosted/external repositories.

## Владение и реализация

Admin владеет каталогом, reservations, bindings, CAS, operations и audit.
Продукты владеют ресурсами и локальными подтверждёнными проекциями. Идентичность
включает instance ID; имя, URL и короткий ключ идентичностью не являются.
Люди работают в общем доверенном пространстве; owner-only approvals и машинные
границы не расширяются. Решение: [ADR 0022](../adr/0022-shared-namespace.md).

Используются существующие Rust/Axum, PostgreSQL/SQLx, React/TanStack Query и
Base shell. Альтернатива с универсальным gateway или общей продуктовой БД
отклонена: она нарушает владение и делает обычную работу зависимой от Admin.

## Пакеты и gate

| Пакет | Работа | Зависимости |
| --- | --- | --- |
| N0 | Owner contracts, ADR, PR reconciliation, credential/lifecycle policy | — |
| N1 | Admin registry, durable operations, reservation, CAS, audit, reconcile | N0 |
| N2 | Tracker bindings/guards, permanent counter, bounded catalog/context | N1 |
| N3 | Wiki binding, typed TaskRef, immutable revision/document links | N1/N2 |
| N4 | Forge groups/catalog IDs, aliases, PR/CI mapping, admission | N1 |
| N5 | Picker, wizard, overview, deep links, unavailable states | N2–N4 |
| N6 | Fleet/Workflow context; execution activation remains independent | N2/N4 |
| N7 | Explicit backfill, backup rehearsal, compatible rollback cohort | N0–N6 |
| N8 | Integrated functional acceptance and fresh installed evidence | N1–N7 |

Во время разработки — scoped checks; общий build/test/OpenAPI/CI и IAB
375/1920/2560 после интегрированного milestone. Source, tests, image IDs,
served hashes и пользовательская приёмка учитываются отдельно.

## Миграция и сохранность

Только schema expansion в canonical migrations. Не переписывать исторические
checksums, pins, IDs, dossiers/revisions или clone URLs. Соответствия ресурсов
задаются явно после inventory/backup. Не выводить их из одинаковых имён.
Неизвестные legacy refs остаются с диагностикой. После активации rollback только
на cohort с bindings/counter/guards; отключение UI не отключает guards.
Запрещены down migrations с потерей новых данных, prune/reset и пустые замены.
Сохраняются pdlc1/pdlc-common и соседние stacks; upstream Hermes не меняется.

## PR reconciliation

Проверены исходные heads: Tracker #114 `357caa7`, Forge #87 `fc3e107`,
Wiki #59 `6f8338f` (conflicting), Fleet #59 `aaddfbe`, Workflow #90 `1139871`.
Tracker #114, Forge #87/#89 и Fleet #59 интегрированы в task-owned ветки с сохранением
истории и исходных heads. Namespace не зависит от включения автономного
SDLC; runtime_ready/dispatch_allowed не повышаются. Draft counter должен
использовать тот же allocator, что обычные Issues; one-project PM ownership
нельзя подменять бизнес-Namespace. Forge #89 сохраняет owned-workspace recovery
из #87 и использует Compose runner; устаревший cleanup вызов согласован с ними.
Forge #88 остаётся отдельным расширением deployment после неизменяемой 0039.
Механизм Wiki #59 адаптирован в Namespace-кандидате: подтверждённое имя обновляет
profile по central subject, machine principal определяется раньше. Legacy SDK pin
сохранён; его смена не нужна для имеющегося try_token_with_name. Включены три
PostgreSQL regression-сценария из exact head #59. Исходный PR пока conflicting.
Workflow #97 остаётся установленной baseline.

## Обязательные проверки

Concurrency/replay/payload conflict, lost ACK/restart/original-key reconcile;
одинаковые имена/slug в разных namespaces; Admin outage; readonly/revoked PAT
и machine denial; archive через legacy API/Git/PR/schedule; restore прежних IDs
без снятия независимого архива документов; counter после purge; заполненные
migrations/aliases/PR numbers; compatible rollback.

Сквозной сценарий: Namespace → три ACK → Task → опубликованная Wiki revision →
два hosted repos → push/PR/CI нужного repo → external registration → restart →
archive/restore, без API-моков. До fresh evidence N8 не считается завершённым.


## Доказательства реализации (2026-10-09, работа продолжается)

- Ветки `feat/shared-namespace-20261008`: Base, Admin, Tracker, Wiki, Forge, Fleet и Workflow.
- Scoped Rust compilation, проверки типов UI и focused tests учитываются отдельно от общего gate.
- Восстановлены заполненные snapshots шести БД в PostgreSQL без сети и постоянных volumes. Аддитивные migrations прошли; Workflow сохраняет schema `project_workflow` и baseline 0006.
- 32 конкурентных номера Tracker уникальны и последовательны. Archive закрывает legacy writes/counter; restore сохраняет IDs. Удаление managed Project остаётся закрытым.
- Wiki запрещает изменение и удаление опубликованной revision; независимый архив документа переживает Namespace restore.
- Forge сохраняет одинаковые repository slugs в разных группах; архив закрывает новые CI starts и разрешает terminal drain уже работающего job.
- Потерянная projection закрывает writes в Tracker, Wiki и Forge; managed marker нельзя снять обычным обновлением.
- Private evidence: `.local/reports/namespace-rehearsal-20261009-004921/evidence.json`. Проверены strict projection constraints всех трёх owners, исторические PR/aliases и legacy dossiers. QA использует отдельный Compose project. Это репетиция миграции, не runtime acceptance.
- Последний readback PR: Forge #88 `56f1217` (четыре checks успешны), #89 `955bbde`; Wiki #59 `6f8338f` остаётся conflicting. Их исходные checks не подтверждают общий кандидат Namespace.
- Совместимый SDK закреплён отдельными `.namespace-base-revision` на Base `0b51e41206f0e783f5c51bd03625240bf97cba06`; legacy pins сохранены. Exact tracked export проверен, Base Rust/UI gate пройден.
- Восстановлены и прочитаны все восемь PostgreSQL backups в изолированном Compose; файловые архивы четырёх volumes извлечены и проверены. Эти volumes сейчас пусты. Evidence: `.local/reports/namespace-backup-restore-20261009-011002/evidence.json`.
- Workflow прошёл 56 real-PostgreSQL integration tests с актуальной 0007. Общий Rust consumer gate и полный Workflow coverage продолжаются.
- Установленные images остаются прежними. Publication consumer cohort, rollout/rollback и IAB-приёмка ещё не завершены.
