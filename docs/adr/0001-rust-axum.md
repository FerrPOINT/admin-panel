# ADR-0001: Rust + Axum для backend

## Status

Accepted

## Context

Admin Panel -- самостоятельный внутренний control plane на портах 7771/7772/7773. Нужен компактный backend для защищённого REST API, OpenAPI-контракта, PostgreSQL и конкурентного чтения runtime config. Стек должен оставаться совместимым с практиками SDLC Rust-сервисов и не требовать gateway/CDN в v1.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| Go + Gin | простая модель развертывания | отдельные conventions от существующих Rust-сервисов |
| Java + Spring Boot | зрелая экосистема | более тяжёлый runtime для компактного control plane |
| Node.js + NestJS | единый язык с frontend | слабее граница типов и иной server-side стек |
| Rust + Axum | async, типобезопасность, совместимость с SDLC | более высокий порог Rust и время компиляции |

## Decision

Использовать Rust edition 2024 и Axum. Application composition строится вокруг явного `AppContext`, HTTP DTO отделены от domain и persistence моделей, OpenAPI выводится из handlers/DTO. PostgreSQL-доступ и внешние JWKS вызовы изолируются в infrastructure adapters.

## Consequences

- Единый с существующими SDLC-сервисами язык, подход к middleware и observability.
- Низкое потребление ресурсов и безопасная конкурентная обработка конфигурации.
- Потребуются Rust review-практики, обязательные fmt/clippy/test и ясные trait boundaries.
- Axum не навязывает DI/ORM: composition и разделение слоёв должны дисциплинированно поддерживаться проектом.

## Related

- `docs/CODE_STYLE.md`
- `docs/API_STANDARDS.md`
- `docs/adr/0003-postgresql.md`
