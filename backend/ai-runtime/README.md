# AI-runtime SDLC2

Частичная серверная реализация; inference и активация провайдера ещё не готовы.

Библиотечный NativeConversation кодирует initial turn из frozen profile/scope:
ведущие system instructions через baseInstructions, остальная история и tool
pairs без потери, последний user ровно один раз. Complete tool-result tail
использует поддержанный пустой turn input; interleaved system, неполные пары и
assistant tail блокируются, а не исправляются синтетическим сообщением.
Codec не разрешает provider dispatch; actual wire/semantic acceptance и numeric
accounting отделены от unit проверки. HTTP admission остаётся закрытым.
Компонент включён в собственный manifest SDLC2 явной миграцией `Migrate -Feature ai`.
Рабочая Admin-консоль `/ai` использует authenticated bridge; отдельный preview
сохраняет fixtures. Подключение владельца и inference ещё не проверены.

## Запуск и сохранность

Runtime не создаёт отсутствующее состояние автоматически. Отдельная команда
`admin-panel-ai-runtime init` создаёт только новое AI-хранилище в пустом каталоге,
не заменяет существующий ключ и не инициализирует БД или ресурсы стенда.

Все переменные ниже задаются deployment-конфигурацией; пути абсолютные:

| Переменная | Назначение |
| --- | --- |
| AI_RUNTIME_WORKSPACE | Сейчас только sdlc2 |
| AI_RUNTIME_LISTEN | Internal listener, без публичного ingress |
| AI_RUNTIME_STATE_DIR | Собственный persistent encrypted vault и owner marker |
| AI_RUNTIME_KEY_FILE | Private 32-byte key вне state directory |
| AI_RUNTIME_CLIENTS_FILE | Private JSON-массив service tokens и scopes |
| AI_RUNTIME_CODEX_BINARY | Собственный проверенный executable |
| AI_RUNTIME_CODEX_VERSION | Точная версия, проверяется перед initialize |
| AI_RUNTIME_CODEX_HOME | Собственный tmpfs для decrypted managed auth |
| AI_RUNTIME_CODEX_WORKDIR | Собственный пустой каталог, без checkout |
| AI_RUNTIME_EXTERNAL_CALLS | Только literal true включает внешние операции; default false |

Client file содержит объекты с `token` и `scopes`. Token — минимум 32 символа,
scopes — `ai:admin`, `ai:catalog`, `ai:infer`. Infer не разрешает credential
operations. Unix permissions ключа и client file — 0600.

Зашифрованный state сохраняется AES-256-GCM с workspace AAD, process lock и
atomic fsync/rename. Ошибка записи не меняет эффективное состояние в памяти.
Ключ не входит в state backup. Не читать или переносить global Codex auth.

## Действующие internal handlers

- GET `/internal/v1/providers` — metadata, capabilities пока не verified.
- PUT `/internal/v1/providers/openrouter/connection` — write-only credential,
  `operation_id`, generation, идемпотентный replay и conflict на другой payload.
- DELETE того же connection — отключение без fallback.
- GET `/internal/v1/providers/{provider}/models` — catalog, не доказательство доступа.
- GET `/internal/v1/providers/{provider}/verifications/{id}` — только own proof
  readback; producer ещё не готов, TTL не продлевается.
- POST `/internal/v1/profiles/registrations` и GET `.../{operation_id}` — durable
  receipt exact revision/draft/generation; без inference и повторной активации.
- GET `/internal/v1/providers/chatgpt/account` — whitelist account metadata.
- POST `/internal/v1/providers/chatgpt/login` — managed device-code login,
  UUID в `Idempotency-Key`; unknown outcome сохраняется, не запускается повторно.
- GET/DELETE `/internal/v1/providers/chatgpt/login/{id}` — lookup/cancel.
- DELETE `/internal/v1/providers/chatgpt/connection` — own logout.
- GET `/health/live` — только жизнеспособность HTTP процесса.

Реальные device login start/status/cancel прошли без входа владельца и inference.
Completion/refresh/logout авторизованного владельца ещё требуют live проверки.
Credential operations не являются verification или publication. Native builtin tool
requests сейчас отклоняются. Library tool-result continuation реализован без
исполнения tools; generic completion gateway и live provider dispatch отсутствуют.
Catalog не подставляет неизвестные лимиты. Restore с external_calls=false не
запускает Codex, OAuth refresh или запросы моделей.

## Проверки

Фактический rollout 2026-10-03: собственные state/key/client volumes, UID 10001,
read-only root, tmpfs Codex home/workdir, отсутствие host ports и checkout/socket
mounts проверены. Admin API требует SSO, internal API отвергает запрос без
service token; registry/account/catalog не возвращают секреты. Native model/list
содержит `gpt-6-luna`, что не подтверждает доступ подписки к inference.
Прерванная инициализация продолжает тот же проверенный ключ/staging. Encrypted
state восстановлен отдельно; ключ вне архива, external calls выключены,
непустое назначение отвергается. Это проверка пустого начального vault,
восстановление реальной provider authorization предстоит после подключения.

В backend, Linux Rust 1.88:

```bash
cargo test --locked -p admin-panel-ai-runtime
cargo test --locked -p admin-panel-domain
cargo test --locked -p admin-panel-infra --test ai_profiles -- --ignored
cargo clippy --locked -p admin-panel-ai-runtime --all-targets -- -D warnings
```

Последняя PostgreSQL-проверка требует `AI_REGISTRY_TEST_DATABASE_URL` строго
на БД `sdlc2_ai_registry_test`. Рабочие БД отклоняются. Fixtures не используют
реальные credentials или платный inference. Inspect generated native schema:
`admin-panel-ai-runtime inspect-schema /absolute/generated-schema-directory`.

`Dockerfile.umbrella` собирает broker binary с non-root UID 10001 и официальный
Linux Codex 0.159.0-alpha.12.1 с проверкой distribution SHA-512. Model tool policy
поставляется readonly рядом с binary и сверяется с compiled bytes; Landlock
разрешает чтение только exact policy file. Config layer origins проверяются при
startup. Установка не использует глобальный HOME. Working manifest и resource migration выполнены;
приёмка реального inference и потребителей остаётся следующим этапом.

Подписанный execution grant verifier и encrypted inference journal проверены
Linux unit/clippy и адресно развёрнуты; их дальнейшая интеграция ведётся отдельным
[планом](../../docs/plans/2026-10-03-ai-execution-journal.md). До готовности issuer,
project access и адаптеров admission/dispatch HTTP не открывается. Generic completion
prototype удалён: новый transport обязан использовать durable dispatch intent
и reservation до платного I/O.

Scoped readback source handlers: POST `/internal/v1/inference/{id}/status`,
`/events`, `/cancel`; signed grant только в body, свой `ai:infer` service client
с deployment machine subject. Grant проверяется по optional
`AI_RUNTIME_EXECUTION_TRUST_FILE`: schema_version/workspace/issuer/audience и public
Ed25519 keys. Infer clients без trust либо с `ai:admin` не допускают startup.
Существующие Admin-only clients не требуют нового файла. Наличие readback handlers
не создаёт issuer, project access, dispatch или provider verification.

Доверенный billing receipt выше reservation сохраняет actual cost и durable
`cost_overrun`; все новые paid reservations/dispatch блокируются после restart.
Это не замена проверке актуальных цен и ограничению provider input/output до I/O.

JSON Schema проверяется закреплённой offline библиотекой `jsonschema 0.56.0`:
компиляция до нового admission, exact tool arguments при terminal tool turn,
local refs и formats без внешнего resolver. Невалидный JSON, schema, неизвестный
format и remote ref дают typed failure без private values в ошибке. Final output
validator подготовлен, но live provider terminal transport ещё не подключён.
Этот объём прошёл 46 runtime tests/clippy, собственный Build/Apply и Check/Status;
платных запросов 0.

Readonly native tool policy qualified offline на настоящем закреплённом binary:
только caller tools, JSON/SSE, tool continuation, quota/cancel без retry/fallback.
После коррекции native config projection прошли 80 runtime tests и clippy.
Fixtures не доказывают доступ к подписке. Raw system history SDK отбрасывает;
поддержанный baseInstructions сохраняется как developer instruction. Production
conversation codec, semantic acceptance, numeric framing и physical output reserve
ещё требуются; admission закрыт. [Детали квалификации](../../docs/plans/2026-10-04-native-wire-qualification.md).
