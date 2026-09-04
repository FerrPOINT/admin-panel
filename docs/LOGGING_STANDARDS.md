# Logging Standards

## 1. Цели

Логи помогают восстановить ход административного запроса, диагностировать интеграцию и измерять SLO, не раскрывая конфигурационные секреты или identity data. Логирование не заменяет неизменяемый `audit_events`: audit отвечает на «кто изменил что», технический лог -- на «как выполнился запрос».

## 2. Формат и обязательные поля

Backend пишет структурированные JSON-логи через `tracing`. Каждый request log включает: `timestamp`, `level`, `service`, `environment`, `request_id`, `method`, `route`, `status`, `duration_ms`. При наличии допустимы `trace_id`, `actor_subject_id` (псевдонимизированный/минимальный), `integration_id`, `capability`, `config_revision_id`, `error_code`.

Frontend не пишет access token или payload config в console. Клиентские ошибки отправляются только через одобренный telemetry transport с `request_id`, route, release и redacted diagnostic code.

## 3. Уровни

| Уровень | Использование |
|---|---|
| `ERROR` | запрос не выполнен, нужна реакция или разбор инцидента |
| `WARN` | контролируемая деградация, конфликт revision, fallback к safe default |
| `INFO` | границы жизненного цикла, публикация revision, результат запроса |
| `DEBUG` | ограниченная диагностика в non-production без чувствительных данных |
| `TRACE` | локальная разработка; в production выключен по умолчанию |

Не логируются `Authorization`, cookie, JWT, password, ключи, secret value, полный runtime config, PII сверх минимально нужного subject reference и сырые тела запросов. Redaction выполняется до сериализации; маскирование в sink недостаточно.

## 4. Audit и события безопасности

Успешная и отклонённая попытка опубликовать/откатить config, изменить capability или contract создаёт audit event с actor, действием, target, результатом, request/trace ID и redacted diff. Лог безопасности не должен давать читателю возможность восстановить существование закрытой интеграции или содержимое секрета.

Внешние ошибки записываются как классифицированный код и безопасный контекст. Стек, SQL и адрес внутренней инфраструктуры не возвращаются клиенту; их запись ограничивается защищённым sink.

## 5. Эксплуатация

Сохраняются метрики запросов, latency, error rate, cache hit/304, config publication и capability-denied. Корреляция строится на `request_id`/`trace_id`, которые передаются в API-ответе. Уровень логирования меняется временно, scoped и с ограниченным сроком.

## Связанные документы

- `docs/API_STANDARDS.md`
- `docs/DATABASE_STANDARDS.md`
- `docs/PERFORMANCE.md`
