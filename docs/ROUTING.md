# Маршрутизация SDLC Admin Panel

## 1. Статус документа

Это план маршрутов до UI-кода. Каждый маршрут P0 должен получить отдельный текстовый сценарий, wireframe и явное согласование до реализации. Появление маршрута в таблице не является разрешением создавать страницу без этого этапа.

## 2. Группы маршрутов

| Группа | Auth | Layout | Поведение |
|---|---|---|---|
| Public auth handoff | Нет локальной auth-формы | Минимальный | Редирект к центральному auth или отображение безопасной ошибки callback. |
| Protected app | ES256 JWT + роль панели | `AdminShell` | Основные административные страницы. |
| Forbidden | JWT есть, роли недостаточно | Минимальный | Объяснение без раскрытия policy internals. |
| Not found | Любой | Минимальный | 404, без redirect в защищенные данные. |

## 3. План страниц P0

| Route | Роль | Назначение | Данные | Разрешенные действия | Не должен делать |
|---|---|---|---|---|---|
| `/login` | public | Начать central-auth flow. | Состояние redirect/error. | `Sign in` redirect. | Локальный пароль, register, refresh. |
| `/auth/callback` | public | Принять результат внешнего auth flow на web-клиенте. | Только transient result. | Завершить memory session/redirect. | Логировать token в URL/UI. |
| `/` | viewer+ | Обзор control plane. | Количество сервисов, current branding revision, последние audit events. | Переходы к деталям. | Управлять внешними сервисами. |
| `/services` | viewer+ | Список registry. | `GET /services`; status/owner filters. | Фильтр, перейти в карточку; operator может открыть declaration flow. | Редактировать arbitrary endpoint inline. |
| `/services/new` | operator+ | Подать новую service/declaration. | Capability catalog. | Создать pending declaration с confirmation. | Активировать без admin approval. |
| `/services/:serviceKey` | viewer+ | Карточка сервиса и его границы. | Запись, approved/pending declarations, check history, audit. | Operator: новая declaration/check; admin: approve/disable/retire. | Предметный CRUD сервиса или raw HTTP console. |
| `/branding` | viewer+ | Текущий бренд и revisions. | Published document, revision list. | Operator: create draft; admin: publish. | Вставлять CSS/JS/HTML. |
| `/branding/revisions/:revision` | viewer+ | Детали revision/draft. | Schema-validated document, ETag, audit. | Operator: edit draft; admin: publish. | Изменять published revision in place. |
| `/audit` | viewer+ | Audit trail. | `GET /audit-events` с bounded filters. | Фильтры, пагинация, переход к entity. | Delete/edit audit row. |
| `/access` | admin | Матрица claim-to-role панели. | Role bindings. | Create/remove mapping с confirmation. | Создавать auth users или менять auth storage. |
| `/forbidden` | authenticated | Понятный 403 state. | Требуемая общая роль. | Вернуться в доступный раздел/выйти. | Раскрывать claims/внутренние policy. |
| `*` | any | Not found. | Нет. | На главную только по явному действию. | Автоматически перенаправлять к защищенному dashboard. |

## 4. Предварительные UX-сценарии

### 4.1. Обзор `/`

- Цель: оператор видит границы панели и состояние только тех сущностей, на которые имеет право просмотра.
- Блоки: сервисы по статусу, опубликованный брендинг revision/время, последние audit events, понятное состояние пустой установки.
- Error/loading: skeleton, retry для локального API; отсутствие/ошибка auth ведет к login/forbidden, не к ложному "нет данных".
- Mobile: карточки становятся одной колонкой; таблица аудита не является обязательной на overview.

### 4.2. Реестр `/services` и карточка

- Список: `service_key`, display name, owner team, status, declaration revision, last check outcome/time; запрещены endpoint с credentials и полный внешний diagnostics payload.
- Карточка: отдельные секции "Декларация", "Разрешенные возможности", "Проверки", "Аудит", "Danger zone".
- Mutation: перед approve/disable/retire обязательно confirmation с объяснением, что действие меняет лишь состояние реестра и не отправляет удаленную команду.
- Empty: "сервисов нет" и кнопка declaration доступна только operator+.

### 4.3. Брендинг `/branding`

- Представление current published configuration и revision history без remote image fetch в admin browser до безопасной validation.
- Draft form содержит только fields branding schema; live preview использует локальные sanitized values.
- Publish требует current ETag/confirmation и показывает, что consumers получат обновление в пределах TTL/revalidation.
- Ошибка `412` предлагает reload/compare, а не silent overwrite.

### 4.4. Аудит `/audit`

- По умолчанию: newest first, ограниченное окно, cursor pagination.
- Фильтры: временной диапазон, action, entity type/id, actor subject при наличии права политики.
- Event detail показывает sanitized metadata, request ID и ссылку на доступную entity; raw token/request/response недоступны.

### 4.5. Access `/access`

- Только `platform_admin`.
- Явно маркируется как mapping claims -> roles Admin Panel, не user management.
- Form разрешает только known claim names и одну из трех panel roles.
- Remove требует confirmation, потому что изменение может лишить группу доступа.

## 5. Route guards и переходы

1. Web-клиент проверяет наличие memory-auth state и запрашивает защищенный API.
2. `401` очищает transient client state и направляет на `/login` без сохранения token в URL.
3. `403` ведет на `/forbidden`; маршрут исходного ресурса может сохраняться только как локальный safe return path.
4. `404` на resource показывает route-level not found, не глобальный redirect.
5. Mutation UI отправляет `If-Match` и `Idempotency-Key` по контракту API.
6. После mutation обновляются только связанные query cache entries; никакого polling/command fan-out внешних сервисов.

## 6. Query parameters

| Route | Параметры |
|---|---|
| `/services` | `status`, `owner_team`, `cursor`, `limit` |
| `/audit` | `from`, `to`, `action`, `entity_type`, `entity_id`, `actor_subject`, `cursor`, `limit` |
| `/branding` | `cursor`, `limit` для истории revisions |

Все query parameters валидируются на клиенте для UX и повторно на API; произвольная sort field отсутствует в v1.

## 7. Маршруты API и web

Browser routes принадлежат web на `7772`. REST routes принадлежат API на `7771` и описаны в `docs/API.md`. Web никогда не обращается напрямую к PostgreSQL `7773` и не заменяет service integration endpoint маршрутом собственного UI.

## 8. Условия перед реализацией UI

Перед кодом каждой P0-страницы необходимо приложить к задаче:

1. текст цели и сценариев из данного документа, дополненный data contract;
2. wireframe desktop и mobile;
3. состояния loading, empty, error, 401/403 и optimistic-concurrency conflict;
4. явное пользовательское одобрение wireframe;
5. e2e acceptance scenario и screenshot checklist.

## References

- `docs/TZ.md`
- `docs/API.md`
- `docs/ARCHITECTURE.md`
- `docs/SECURITY.md`
