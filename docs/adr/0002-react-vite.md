# ADR-0002: React + Vite для frontend

## Status

Accepted

## Context

Операторам нужен отзывчивый browser UI для просмотра, ревизии и публикации платформенной конфигурации. Приложение self-hosted, не требует SEO/SSR в v1 и не использует CDN/gateway. Нужны совместимые с SDLC React-практики: строгий TypeScript, типизированный API-клиент и browser E2E.

## Alternatives Considered

| Вариант | Плюсы | Минусы |
|---|---|---|
| Vue + Vite | компактный DX | отклонение от существующей React-практики |
| Next.js | SSR и встроенный routing | избыточная server-сложность для закрытого SPA |
| React + Vite | зрелая экосистема, быстрый dev/build, совместимость | routing, state и runtime config нужно собрать явно |

## Decision

Использовать React + TypeScript + Vite как SPA. React Router отвечает за маршруты, TanStack Query -- за server state, Tailwind и локальные shadcn-style primitives -- за UI. Конфигурация загружается через минимальный безопасный bootstrap и защищённый Admin Panel API; секреты не включаются в bundle или public config.

## Consequences

- Быстрый локальный цикл разработки и общий с SDLC frontend stack.
- Нет SSR и CDN-зависимости в v1; статика обслуживается развёрнутым frontend сервисом.
- Нужны явные ErrorBoundary, loading/error/forbidden states и code splitting маршрутов.
- Frontend остаётся клиентом control plane и не становится инструментом удалённого произвольного управления сервисами.

## Related

- `docs/FRONTEND_ARCHITECTURE.md`
- `docs/PERFORMANCE.md`
- `docs/adr/0005-runtime-config-etag.md`
