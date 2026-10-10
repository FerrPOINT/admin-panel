# Квалификация Admin на общем кандидате Base

Часть утверждённого пользователем завершения унификации и циклического ревью
перед merge. Согласованный кандидат Base опубликован в PR #164 на SHA
`f2b0ff2993bcdb43415195d7f9fa6d8ca6ff2a94`.

## Изменение

- Закрепить кандидат в активном `.namespace-base-revision`, используемом CI
  и standalone build. Legacy `.base-revision` сохраняет прежнюю роль fallback.
- Обновить Cargo.lock намеренно до сборки: Base messaging закрепляет
  `time 0.3.55`, вместе с ним меняются time-core и time-macros. Production
  продолжает использовать `cargo build --locked`.
- Описать выбор активного pin одинаково с существующими CI и build.py.

Бизнес-правила, API, права, PAT scopes, env/defaults и миграции сохраняются.
Установленные images не переключаются. После merge Base заменить candidate pin
на итоговый merged SHA и повторить проверки точного комплекта.

## Проверки и приёмка

- Rust 1.98.1: fmt, locked clippy/test всего workspace, настоящий PostgreSQL,
  OpenAPI drift/compatibility и production Dockerfile.
- Отдельный MSRV 1.88, domain dependency graph и изолированная сборка.
- Frozen frontend install, typecheck/lint/unit/build, Base package consumer,
  effective themes и browser checks на закреплённом Base.
- Документационные гейты, неизменность lockfiles при сборке и provenance images.
- Общая живая SSO/logout/PAT приёмка и три последовательных полных чистых ревью
  согласованного комплекта. До этих доказательств PR остаётся Draft.

Откат: предыдущий проверенный набор commits/images. Документация Base обновляется
штатным sync после подтверждённых продуктовых проверок.
