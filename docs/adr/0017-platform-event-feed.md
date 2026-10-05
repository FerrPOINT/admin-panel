# ADR-0017: долговечная безопасная лента межсервисных событий

Дата: 2026-10-02. Статус: принято; пилот реализован и проверен автоматическими сценариями. Рабочий rollout отдельно.

Admin Panel напрямую подключается к общей NATS JetStream шине через Base SDK
`sdlc-messaging`. Библиотека фиксирует inbox и product handler в одной
транзакции; ack отправляется только после commit. Admin владеет таблицами
inbox, quarantine и безопасной проекцией `platform_events` в своей БД.

В пилоте принимается только `platform.cicd.pipeline.finished.v1` от `ci-cd`.
Raw payload, названия проектов, команды и секреты не сохраняются в ленте.
Повтор события подавляет inbox; повтор завершения с другим event id является
конфликтом, не перезаписывает факт и направляется в quarantine через SDK.

Feed хранится 30 суток, processed/quarantined inbox и quarantine — 90 суток.
Очистка ограничена 500 строками на таблицу за проход, pending inbox сохраняется.
List count/items читаются в одном repeatable-read snapshot. HTTP API закрыт
существующей central auth/PAT read scope; public runtime не раскрывает ленту.

Интеграция по умолчанию выключена. Недоступность broker/producer diagnostics
не блокирует остальные функции админки. Статусы источников независимы.
План: [product integration](../../../services-base/docs/plans/2026-10-02-platform-messaging-product-integration.md).

## Изменение профиля 2026-10-03

Исторические сроки этого ADR заменены [ADR-0019](0019-messaging-retention-diagnostics.md):
feed 90 суток, terminal inbox/quarantine 180. Остальные решения сохраняются.
