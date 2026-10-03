# Согласование инструментов сборки

## Объём

Объединить Docker-обновления Rust 1.98.1 и Node 26.10.0 с согласованными
release CI и документацией. Установить pnpm 10.28.1 напрямую через npm,
поскольку новый Node image не содержит вызываемый Dockerfile Corepack.
MSRV 1.88.0, Base SHA и lockfiles сохраняются. PM-реализация не входит в PR.

## Проверка

- Rust release: fmt, strict Clippy, locked workspace tests, реальные PostgreSQL
  audit regressions, OpenAPI drift и сборка backend images.
- Отдельно MSRV 1.88.0: locked workspace/all-targets check.
- Node 26.10.0 / pnpm 10.28.1: frozen install, lint, format, typecheck,
  tests, build, OpenAPI drift/compatibility, packed Base consumer и effective
  themes; сборка обоих frontend Dockerfile.
- Проверить неизменность lockfiles и точный соседний Base checkout.

## Поставка и откат

Самостоятельный PR в main связывает два dependency PR и устраняет дефекты
совместимости до их поставки. Готовность зависит от выполненных проверок.
Принятые runtime images и данные не меняются; откат инструментов возвращает
предыдущий проверенный commit/image набор.
