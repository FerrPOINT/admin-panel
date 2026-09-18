# Runtime Base Admin Panel

## Контур

В Base umbrella Admin Panel использует:

| Компонент | Порт | Назначение |
|---|---:|---|
| Admin API | `7771` | Registry, branding, audit, access API. |
| Admin web | `7772` | Браузерный административный интерфейс. |
| PostgreSQL | `7773` | Изолированное хранилище Admin Panel. |
| Central auth | `7701` | Внешний issuer/JWKS, не часть runtime Admin Panel. |

PostgreSQL публикуется на loopback для локальной разработки. Приложение не предполагает прямого browser-доступа к БД.

## Startup и readiness

1. API читает required `ADMINP_DATABASE_URL`, listener и auth-trust configuration.
2. API подключается к собственной PostgreSQL и применяет SQLx migrations при startup.
3. После успешного database setup запускаются API `7771` и web `7772`.
4. `GET /health/live` отвечает при работающем процессе; `GET /health/ready` проверяет owned database.

Readiness не зависит от доступности сервисов registry или central auth. Public runtime branding продолжает обслуживаться из собственной базы; protected requests fail closed, если central validation token не проходит.

## Локальная сверка каталога

В Base umbrella `ADMINP_BOOTSTRAP_SERVICES` декларирует локальный сервисный
каталог. При старте API создаёт отсутствующие записи и сверяет только
declaration с маркером `local-bootstrap` или историческим `bootstrap`; активная
операторская declaration никогда не перезаписывается. Declaration содержит
только service origin, необязательный public UI origin и фиксированные
capability; в ней нет credentials, произвольных путей или request data.

Admin API подключается к product networks только для этих объявленных
read-only health probes. Runtime catalog остаётся безопасной read-only
проекцией: он не является service discovery, remote execution или каналом
распространения секретов.

## Runtime branding

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

- `Cache-Control: public, max-age=60, must-revalidate` и ETag ограничивают stale window.
- API не контактирует с registry services на пути выдачи branding.
- Consumer сначала использует встроенный default; недоступный/невалидный ответ не блокирует render, auth или primary product flow.
- Нет CDN, WebSocket, SSE или push delivery requirement.

## Integration checks

Integration check выполняется по явному запросу authorized operator и только для active approved declaration. Caller выбирает capability, а server берёт fixed method и path из локального catalog. Внешний request остаётся read-only; arbitrary URL, method, headers и body не принимаются.

Failure check сохраняет sanitised result и не меняет registry state автоматически. Disable или retire — явное операторское действие с audit.

## Configuration boundary

| Категория | В deployment configuration | Никогда не в public API/README evidence |
|---|---|---|
| Database | DSN собственной PostgreSQL | Password/DSN secret |
| Auth trust | Issuer, audience, public JWKS URL, central login URL | Private signing key и bearer token |
| Browser policy | CORS allowed origins | Consumer credentials |
| Registry policy | Approved service declaration и capability catalog | Arbitrary remote request data |
| Observability | `RUST_LOG` and deployment sink | Secret values and raw external responses |

Names and safe defaults: `.env.example` and [Local Setup](LOCAL_SETUP.md). Configuration parsing does not log secret source values.

## Logs, metrics и backup

- Server emits JSON tracing and request-level route/status data; credentials, tokens and external response bodies are excluded.
- The current application exposes runtime health contracts. Prometheus-specific metrics endpoint is not part of the committed Admin Panel API contract.
- Backup/restore scope is Admin Panel PostgreSQL only. Restore never changes central auth or product databases; verify migration state, branding revision, registry integrity and audit records afterwards.

## Not in scope

- Multi-region replication or HA;
- Redis/queue/event bus;
- auto-discovery or periodic polling of all services;
- remote config push, deploy, migration or command execution;
- central auth storage synchronization.

## References

- [Architecture](ARCHITECTURE.md)
- [API](API.md)
- [Security](SECURITY.md)
- [Local setup](LOCAL_SETUP.md)
