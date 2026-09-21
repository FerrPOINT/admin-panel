# Карточка сервиса: detail-layout и проверки интеграции

## Цель

Привести `/services/:serviceKey` к документированному `detail-with-aside` и
сделать доступным уже существующий bounded flow проверок интеграции. Страница
должна использовать wide desktop без растягивания форм, сохранять понятный
линейный порядок на mobile и не добавлять произвольный remote-control UI.

## Роли и источники данных

- Наблюдатель читает карточку, активный контракт, историю деклараций и историю
  проверок.
- Оператор и администратор дополнительно отправляют декларацию, одобряют
  pending-декларацию, запускают разрешённую capability-проверку и подтверждают
  отключение или вывод сервиса из эксплуатации.
- Карточка и декларации: `GET /api/v1/services/:serviceKey`.
- История проверок: `GET /api/v1/services/:serviceKey/checks`, не более 50
  записей по текущему backend-контракту.
- Запуск проверки: `POST /api/v1/services/:serviceKey/checks` только с ключом
  capability активной approved-декларации. URL, method, headers и body сервер
  строит из локального allowlist; UI их не принимает.
- Права и mutation-доступ остаются в `useAuth`; backend остаётся границей
  авторизации.

## Компоновка и сценарии

1. Заголовок и возврат в каталог занимают полную ширину рабочей области.
2. На wide desktop основная fluid-колонка содержит активный контракт, проверки,
   форму новой декларации и историю. Справа расположен sticky contextual rail
   шириной `320 px` со статусом, metadata и допустимыми действиями.
3. На mobile rail идёт сразу после заголовка, до подробных данных, чтобы статус
   и основные команды не оказывались после длинной истории.
4. Форма сохраняет ограниченную ширину полей, но не ограничивает ширину всей
   страницы. Длинные URL, capability и subject безопасно переносятся.
5. Проверки разделяют initial loading, error/retry, empty, pending, mutation
   error и success. Повторная загрузка истории не скрывает уже видимые строки.
6. Запуск недоступен без active approved declaration, при неактивном сервисе,
   для viewer и во время запроса. Выбираются только известные capability из
   активной декларации.

## Проверка

- Unit: exact POST capability, pending/error/success, retry истории, viewer
  read-only и существующие declaration/status сценарии.
- `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm build`.
- Browser с API-моками: active/read-only/error/empty states, точный POST,
  keyboard flow и 375/768/1440/2560 px в light/dark.
- Измерить document overflow, ширину rail `320 px`, цели действий от `40 px`,
  доступные имена controls и отсутствие неожиданных console/API ошибок.
- Снимки сохранить в `docs/assets/screens/2026-09-21-service-detail/`.
