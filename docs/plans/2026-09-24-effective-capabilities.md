# Effective capabilities для Admin Panel

## Цель

Сделать `GET /api/v1/auth/me` достоверным источником возможностей интерфейса.
Browser SSO сохраняет одинаковый полный доступ, принятый ADR-0008. Personal
access token получает право на мутации только при точном scope
`admin-panel:write`. Исторические role bindings в central-configured mode
остаются отключёнными.

## Проблема

- API сейчас сообщает `mutate: true` любому токену, который смог прочитать
  `/auth/me`, включая PAT только с `admin-panel:read`.
- API сообщает `manage_bindings: true`, хотя ADR-0008 отключает управление
  локальными bindings в central-configured mode.
- Frontend игнорирует объект `capabilities` и считает любую принятую сессию
  способной изменять данные.
- Services уже умеют скрывать mutation controls, но branding, revisions,
  users и tokens не используют этот контракт.

## Контракт

| Caller | `mutate` | `manage_bindings` |
| --- | --- | --- |
| Browser SSO (`sid` присутствует) | `true` | `false` |
| PAT с `admin-panel:write` | `true` | `false` |
| PAT только с `admin-panel:read` | `false` | `false` |
| Неизвестный или неполный ответ frontend | `false` | `false` |

Серверная авторизация остаётся независимой: `allows_service` проверяет точный
read/write scope каждого запроса. UI только не предлагает действие, которое
заведомо будет отклонено.

## Реализация

1. Сохранить в authenticated caller рассчитанные `can_mutate` и
   `can_manage_bindings`; вернуть их из `/auth/me`.
2. Добавить unit tests для browser, read PAT и write PAT без сетевой проверки
   JWT.
3. Валидировать capabilities в `AuthProvider` по строгому `=== true` и хранить
   их в session; отсутствующие или неверные значения трактовать fail closed.
4. Использовать `canMutate` на branding, revisions, services/detail, users и
   tokens. Read-only режим сохраняет чтение, поиск, pagination, preview и diff,
   но не отображает команды изменения.
5. Добавить frontend unit/E2E regression: read-only session не показывает
   mutation controls и не отправляет write-запросы; browser session сохраняет
   текущий полный workflow.
6. Обновить OpenAPI/документацию только там, где уточняется фактический
   capability contract.

## Проверки

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, OpenAPI generation/check;
- frontend lint, typecheck, unit tests и production build;
- Chromium E2E для authenticated full-access и read-only capability fixtures;
- production candidate route/ergonomic regression без overflow, Axe P1/P2,
  console/page/network errors и неожиданных writes.

## Результат проверки

Пакет проверен 2026-09-24:

- frontend unit: 67/67, Chromium E2E: 23/23;
- lint, typecheck и production build прошли;
- backend fmt, clippy и workspace tests прошли;
- OpenAPI, README/docs и Compose config checks прошли;
- Docker candidate `admin-api`/`admin-web` healthy и отвечает `200`;
- live Central Auth вернул browser session `mutate: true` и
  `manage_bindings: false`;
- live Admin smoke прошёл 2/2, включая 120 route/theme/viewport состояний;
- ergonomic matrix прошла 150/150 без overflow, малых или безымянных controls,
  nested scrollers, runtime errors и write-запросов;
- read-only production-bundle fixture сохранил данные, поиск и compare, скрыл
  mutation controls и не отправил write-запросов.

## Вне объёма

- возврат пользовательской role ladder, отменённой ADR-0008;
- включение UI исторических role bindings;
- изменение Central Auth user/token ownership;
- merge PR #23 или этого пакета без отдельной команды пользователя.
