# Performance

## 1. Цели v1

Admin Panel оптимизируется для предсказуемого control plane, а не для массового data plane. Целевые ориентиры при штатной нагрузке: p95 чтения runtime config <= 200 ms внутри сети, p95 административного API <= 500 ms без внешней зависимости, публикация revision <= 2 s синхронно либо явный `202` с наблюдаемым статусом. Значения подтверждаются нагрузочными измерениями до объявления SLO.

## 2. Backend и БД

- Runtime config возвращается как материализованная безопасная проекция для конкретного consumer; не собирается из неограниченного числа remote вызовов на каждый request.
- `ETag` и `If-None-Match` уменьшают передачу неизменной конфигурации; `304` измеряется отдельно. ETag не отменяет проверку authorization.
- Запросы ограничены pagination, cursor ordering и allowlisted filters. N+1, неограниченные JSON payload, full-table scan в горячем пути и синхронный fan-out запрещены.
- Индексы добавляются после анализа реальных query plans. Connection pool имеет лимит, timeouts и backpressure; долгие внешние действия выполняются через bounded worker/outbox, не внутри HTTP transaction.
- Сжатие и кэш разрешены только для безопасных представлений. Запрещено помещать protected config в shared/public cache.

## 3. Frontend

Маршруты загружаются лениво; крупные таблицы используют server-side pagination и виртуализацию при измеренной необходимости. Debounce применим к поиску, но не скрывает состояние сохранения. React memoization добавляется после профилирования. Bundle budget и регрессии сборки контролируются CI.

Загрузка runtime config ограничена bootstrap + условным GET по ETag. Если сеть недоступна или payload не проходит schema validation, consumer сохраняет последнюю валидную конфигурацию в рамках допустимого TTL либо использует документированный safe default; он не включает неизвестную capability и не пытается выполнять remote command.

## 4. Наблюдаемость и проверка

Измеряются p50/p95/p99 latency, error rate, saturation pool, время публикации, размер response, cache validation hit rate, число fallback и capability denials. Нагрузочные сценарии включают одновременное чтение consumers, конфликт публикаций и недоступность auth/JWKS по правилу fail-closed для новых защищённых запросов.

Перед оптимизацией фиксируются baseline и профиль. Оптимизация не должна обходить audit, authorization, optimistic concurrency или safe defaults.

## Связанные документы

- `docs/API_STANDARDS.md`
- `docs/LOGGING_STANDARDS.md`
- `docs/adr/0005-runtime-config-etag.md`
