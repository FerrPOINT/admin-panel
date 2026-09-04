# Architecture Decision Records

## Назначение

ADR фиксируют значимые, долгоживущие архитектурные решения Admin Panel: контекст, альтернативы, принятое решение и последствия. Они хранятся в `docs/adr/`, не удаляются и остаются историей даже после замены.

## Формат

```markdown
# ADR-NNNN: Краткое название

## Status
Proposed | Accepted | Deprecated | Superseded by ADR-NNNN

## Context
## Alternatives Considered
## Decision
## Consequences
## Related
```

Один ADR описывает одно решение. ADR нужен для выбора технологий, границ владения, публичного контракта, модели безопасности или решения, чья замена затронет несколько сервисов. Обычный bug fix, локальный refactoring и текст UI ADR не требуют.

## Активные ADR

| ID | Решение | Status | Дата |
|---|---|---|---|
| ADR-0001 | Rust + Axum для backend | Accepted | 2026-09-04 |
| ADR-0002 | React + Vite для frontend | Accepted | 2026-09-04 |
| ADR-0003 | PostgreSQL как основная БД | Accepted | 2026-09-04 |
| ADR-0004 | Центральная аутентификация через JWKS auth-server | Accepted | 2026-09-04 |
| ADR-0005 | Runtime config с ревизиями и ETag | Accepted | 2026-09-04 |
| ADR-0006 | Декларативный capability-based contract интеграций | Accepted | 2026-09-04 |

## Изменение решения

При замене создаётся новый ADR с новым номером. Старый меняет статус на `Superseded by ADR-NNNN`, а индекс содержит оба решения и ссылку между ними. Новые ADR добавляются в индекс в том же изменении.

## Связанные документы

- `docs/CODE_STYLE.md`
- `docs/API_STANDARDS.md`
- `docs/DATABASE_STANDARDS.md`
- `docs/FRONTEND_ARCHITECTURE.md`
