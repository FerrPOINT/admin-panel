# ADR-0006: Декларативный capability-based contract интеграций

## Status

Accepted

## Context

Admin Panel должен управлять платформенной конфигурацией для CI-CD, task-tracker, wiki, fleet-control и следующих consumers, не превращаясь в удалённый shell/control proxy. Arbitrary remote control повышает blast radius, скрывает зависимости и смешивает ownership сервисов. Каждый consumer должен сохранять собственные доменные данные и иметь безопасное поведение при неполной конфигурации.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| Произвольные HTTP/webhook команды | гибкость без схемы | SSRF, неаудируемая семантика, высокий blast radius |
| Remote shell/agent control | широкая операционная власть | секреты, обход service boundaries, трудно ограничить и тестировать |
| Копирование config в каждый сервис | автономность | дрейф источников истины и сложные rollout/rollback |
| Декларативные contracts + capability | allowlist, schema, аудит, предсказуемый rollout | требуется заранее моделировать поддерживаемые случаи |

## Decision

Интеграция регистрирует consumer, поддерживаемые capability, версию schema, допустимую область действия и безопасные defaults. Admin Panel валидирует configuration against contract и публикует только разрешённую effective projection конкретному consumer. Capability выражает настройку или узкое заявленное действие, но не транспорт произвольной команды.

Consumer сам интерпретирует разрешённую capability в своём domain boundary, не отдаёт Admin Panel внутренние secrets и не принимает неописанные поля. Отсутствующая, неподдерживаемая или невалидная capability даёт безопасный default и диагностическое состояние. Изменения contract/version проходят совместимый rollout по API versioning policy.

Запрещены в v1: remote shell, произвольная команда/скрипт, произвольный URL/webhook, файловый доступ, credential forwarding и endpoint «сделай что угодно на сервисе».

## Consequences

- Чёткое владение: Admin Panel -- платформа config/policy; consumer -- собственный domain/runtime; auth-server -- identity.
- Минимальный privilege, воспроизводимые проверки схемы и понятный audit trail.
- Новая интеграционная возможность требует явного contract/capability design и тестов, а не ad hoc административной кнопки.
- Некоторые срочные операции останутся в собственных операционных интерфейсах сервисов до отдельного ADR и защищённого протокола.

## Related

- `docs/API_STANDARDS.md`
- `docs/API_VERSIONING.md`
- `docs/FRONTEND_ARCHITECTURE.md`
- `docs/adr/0004-central-auth-jwks.md`
- `docs/adr/0005-runtime-config-etag.md`
