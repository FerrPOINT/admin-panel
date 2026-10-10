# ADR 0022: сквозной Namespace и узкие owner-команды

Дата: 2026-10-08. Статус: принят по прямому поручению пользователя.

## Контекст

Tracker Project, Wiki Space и Forge repositories не имеют общей идентичности.
Admin уже владеет центральным каталогом, но его прежний integration check
contract разрешает только read-only checks. Namespace требует отдельного
ограниченного provisioning/lifecycle протокола.

## Решение

Admin хранит Namespace и уникальные reservations/bindings. Продукт хранит
локальную проекцию с generation и operation ID. Внешний ресурс идентифицируется
парой instance/resource UUID и kind. Namespace identity — registry instance/UUID.
Slug неизменяем; display name изменяется с CAS. Ready требует три owner ACK.

Отдельный `/api/v1/namespaces` API не использует legacy role `manage_bindings`.
Owner API допускает только ensure/attach, readback и lifecycle для конкретного
ресурса. Deployment задаёт endpoints, instances и token files; URL/credential
не принимаются от человека. Ограниченные машинные subjects регистрируются до
human mapping; service read/write scopes не становятся глобальным namespace ACL.
HTTP client lifecycle-managed, redirects отключены, timeout/body ограничены.

Reservation и operation сохраняются до HTTP. Повтор использует исходный ключ,
сверяет payload и owner readback; чужой ресурс не присваивается. При частичном
результате данные не удаляются. Reconcile повторяет те же owner-команды.

Archive сначала блокирует новые writes/starts в локальных проекциях, затем
ожидает ACK/drain. Restore возвращает прежние IDs и не снимает независимый архив.
Продукты запускаются без Admin; обычные active операции используют локальную
подтверждённую проекцию. Новые bindings/ownership changes требуют Admin.

## Альтернативы

Tracker Project как корень отвергнут: каталог и Wiki/Git не принадлежат Tracker.
Универсальный proxy, shared DB и distributed transaction отвергнуты: нарушают
владение и автономность. Сопоставление по именам отвергнуто из-за неоднозначности.

## Последствия

Расширяется только namespace capability; прежние check capabilities остаются
read-only. Нужны additive migrations, old-route admission, explicit backfill и
совместимый rollback cohort до активации. Purge отсутствует в v1. Контракты SDLC
v1 не меняются; execution context и его activation — отдельная версия/gate.
