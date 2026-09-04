# Runtime SDLC Admin Panel

## 1. Контур v1

В umbrella-развертывании Admin Panel использует:

| Компонент | Порт | Назначение |
|---|---:|---|
| Admin API | `7771` | Registry, branding, audit, access API. |
| Admin web | `7772` | Браузерный административный интерфейс. |
| PostgreSQL | `7773` | Изолированное хранилище Admin Panel. |
| Central auth | `7701` | Внешний issuer/JWKS, не часть runtime Admin Panel. |

PostgreSQL предназначен для внутренней compose/umbrella-сети. Внешнее опубликование `7773` допустимо только при явном operational решении; application не предполагает прямого доступа браузера к БД.

## 2. Startup sequence

1. Загрузить typed deployment configuration и провалидировать отсутствие пустых обязательных references.
2. Проверить допустимость trusted auth issuer/JWKS origin, audience, CORS origins, URL/CIDR allowlist интеграций и connection к собственной PostgreSQL.
3. Подключиться к PostgreSQL и применить миграции отдельным migration job/процессом; API не должен обслуживать mutations при незавершенной миграции.
4. Инициализировать локальный capability catalog только идемпотентным, versioned seed-механизмом.
5. Создать HTTP client с TLS verification, timeout, redirect policy, response-size и SSRF guards.
6. Инициализировать JWKS cache. Недоступность auth в этот момент не должна уничтожить уже валидный persisted/runtime configuration; защищенные запросы разрешаются только при наличии валидных cached keys согласно policy.
7. Запустить API `7771`, затем web `7772` после доступности API.
8. Mark readiness only when собственная БД, миграции и обязательная локальная конфигурация готовы.

Не выполняются: запросы ко всем сервисам реестра на старте, синхронизация auth storage, push бренда, auto-registration по сети, remote migration/deploy.

## 3. Health probes

| Probe | Путь | Условие `200` | Не проверяет |
|---|---|---|---|
| Liveness | `GET /health/live` | Процесс принимает запрос. | БД, auth, registry services. |
| Readiness | `GET /health/ready` | Своя PostgreSQL доступна, миграции завершены, конфигурация локально валидна. | Доступность каждого сервисного endpoint, CDN/gateway. |

При недоступности central auth API может остаться ready для public runtime branding и диагностических endpoint, но защищенный traffic должен получать безопасный отказ, если нет valid JWKS cache. Этот факт отражается в метрике/операционном статусе, не в публичной выдаче секретных topology details.

## 4. Runtime branding flow

```text
Consumer app                         Admin API :7771
  | use local defaults                         |
  | GET /api/v1/runtime/branding               |
  | If-None-Match: cached-etag                 |
  |-------------------------------------------->|
  | 200 document + ETag + max-age=60            |
  | or 304 Not Modified                         |
  |<--------------------------------------------|
  | validate then apply; failure => keep defaults|
```

Правила:

- Конфигурация читается напрямую с API origin, без CDN/gateway/reverse-proxy cache requirement в v1.
- `Cache-Control: public, max-age=60, must-revalidate`; consumer отправляет `If-None-Match` после истечения TTL/при revalidation.
- API не зависим от реестра сервисов при выдаче бренда и не контактирует с ними на этом пути.
- Consumer не блокирует рендер/критический пользовательский путь: до ответа применяется встроенный default; невалидный/недоступный ответ игнорируется.
- Ротация бренда публикует новую revision и ETag; нет WebSocket, SSE, polling daemon или push.

## 5. Проверки интеграций

Integration check выполняется только по явному запросу authorized оператора и только для approved active declaration. Runtime создает ограниченный исходящий GET по локальному capability catalog.

| Ограничение | Правило |
|---|---|
| Конкурентность | Bounded global и per-service; не запускает unbounded fan-out. |
| Timeout | Connect/read/total timeout фиксированы deployment policy. |
| Redirect | Отключен или строго ограничен разрешенным HTTPS origin. |
| Ответ | Ограничение размера; body не сохраняется. |
| Сеть | DNS/IP allowlist и SSRF blocking. |
| Retries | Нет автоматического retry v1, чтобы не создавать скрытый трафик. |
| Эффект | Только read-only endpoint. |

Неполадка endpoint записывается как `service_check_run`, но не меняет автоматически `active`/`disabled`: это операторское решение с audit event.

## 6. Graceful shutdown

1. Перестать принимать новые HTTP requests.
2. Вернуть readiness `503` для новой маршрутизации трафика.
3. Дождаться активных запросов до ограниченного timeout.
4. Отменить/завершить in-flight integration checks без повторной отправки.
5. Завершить аудит уже подтвержденных транзакций.
6. Закрыть DB pool и HTTP/JWKS clients.
7. Завершить процесс с корректным кодом.

Нельзя оставлять check в неопределенном состоянии: незавершенный run получает terminal outcome `internal_error`/`timeout` с санитизированной причиной либо явно маркируется canceled по будущей модели.

## 7. Конфигурация runtime

Имена переменных и конкретный стек будут зафиксированы при создании кода/compose. Документация фиксирует категории, но намеренно не содержит значений секретов:

| Категория | Примеры назначения | Где не появляется |
|---|---|---|
| Database | URL/учетные данные собственной PostgreSQL. | API, audit, client bundle, docs examples. |
| Auth trust | issuer, audience, JWKS public URL, key cache policy. | Редактируемый UI. |
| HTTP security | CORS, CSP connect origins, request limits. | Runtime branding document. |
| Integration policy | host/CIDR allowlist, timeouts, response limits. | Service declaration от клиента. |
| Observability | log level, metrics exposure, tracing endpoint. | Public API response. |

Startup fail-fast для обязательной DB configuration и insecure production transport. Configuration parse никогда не логирует исходное значение secret.

## 8. Метрики и логи

### Метрики

- HTTP requests/latency/status по route template;
- DB pool/connectivity;
- migration version/readiness;
- JWKS refresh/cache hit/miss/error;
- runtime branding 200/304/404 и latency;
- integration checks outcome/duration/count;
- audit event writes/failures.

Высокая cardinality запрещена: raw URL, JWT subject, ETag hash и request ID не являются labels.

### Логи

Structured log содержит request ID, route, method, status, action, service key и duration. Не содержит Authorization, cookie, secret, token, document сверх разрешенного metadata, response body внешней проверки либо полный sensitive URL.

## 9. Backup и восстановление

Бэкап охватывает собственную PostgreSQL Admin Panel и проверяется restore на чистый изолированный instance. Восстановление не затрагивает central auth или базы потребителей. После restore проверяются migration version, published branding revision, registry integrity и audit append-only policy.

## 10. Не входит в v1

- Высокая доступность/мульти-региональная репликация;
- Redis, очередь, background worker, event bus;
- CDN/gateway для branding;
- auto-discovery сервисов;
- периодический health-poll всех сервисов;
- remote config push и remote command execution.

## References

- `docs/ARCHITECTURE.md`
- `docs/API.md`
- `docs/SECURITY.md`
- `docs/MIGRATIONS.md`
