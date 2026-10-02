# Обновление закреплённого Base перед локальным выпуском

План утверждён пользователем в составе финальной локальной поставки SDLC.

## Изменение

Обновить `.base-revision` на принятый Base main
`9408802dfa978cba2f67162a49adca6f65851b01`. Общий UI и SDK не отличаются от
прежнего `c083783`; изменения Base относятся к Central Auth и локальным гейтам.
Публичные API, модель авторизации, миграции и UI Admin не меняются.

## Проверки

- Frozen install, packed Base fingerprint, codegen/OpenAPI и compatibility.
- Semantic lint, lint, typecheck, unit и production build.
- Локальные backend/PG проверки и release build на закреплённом Rust.
- Документационные проверки и отсутствие посторонних файлов в diff.

GitHub Actions для приёмки не требуются. Новый candidate не заменяет работающий
локальный брендинг: rollout выполняется отдельно после проверки совместимости,
с сохранением прежних pins, конфигурации и данных.
