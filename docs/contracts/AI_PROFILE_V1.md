# Контракт AI v1

Статус: частичная серверная реализация. Domain rules, PostgreSQL drafts,
неизменяемые profile revisions, CAS/audit, authenticated settings и bridge
credential/login/account/catalog/operation readback реализованы в исходниках.
Собственные runtime и live UI развёрнуты в SDLC2 2026-10-03. Publication HTTP и
recovery worker развёрнуты; verification producer и inference ещё не подключены.
Планируемые маршруты ниже нельзя
объявлять действующими по этому документу.

## Владение и доступ

Admin хранит registry/settings и immutable publication revisions. AI-runtime
хранит credentials, managed Codex authorization, catalog и verification evidence.
Обмена через общую БД нет. Human endpoints используют текущую Admin bearer policy:
browser SSO mutation разрешена, PAT mutation требует `admin-panel:write`.
Machine API использует отдельные own service credentials/scopes; public branding
runtime не должен открывать AI profile/inference или credential operations.

## Маршруты Admin

Feature требует явного `ADMINP_AI_WORKSPACE=sdlc2`; без него handlers возвращают
`ai_not_configured`. Registry содержит сохранённые drafts и отдельную runtime
проекцию. Без bridge runtime=null и явная ошибка; сохранённая модель не доказывает
подключение. При fresh install миграции Admin применены, собственный runtime
подключён. Verification producer ещё не подключён; `POST .../verify` пока не
создаёт evidence. `PUT /api/v1/ai/selection` реализован, но требует runtime-owned
proof и при его отсутствии отказывает без создания candidate или activation.
Остальные перечисленные handlers реализованы.

| Метод и путь | Содержание |
| --- | --- |
| `GET /api/v1/ai/providers` | Registry metadata, состояние, сохранённые settings и наличие подключения без секретов |
| `PUT /api/v1/ai/providers/{id}` | Settings + `If-Match`, новая draft revision, проверка инвалидируется |
| `PUT /api/v1/ai/providers/{id}/credentials` | OpenRouter write-only key, явно разрешённый endpoint; ответ metadata |
| `POST /api/v1/ai/providers/chatgpt/login` | Начало device-code flow, bounded login ID/code/verification URL |
| `GET /api/v1/ai/providers/chatgpt/account` | Secret-free состояние native ChatGPT account; без refresh по read request |
| `GET /api/v1/ai/providers/chatgpt/login/{login_id}` | pending/completed/expired/cancelled/failed; без OAuth tokens |
| `DELETE /api/v1/ai/providers/chatgpt/login/{login_id}` | Отмена только текущего login attempt |
| `DELETE /api/v1/ai/providers/{id}/connection` | Отзыв own connection, без автоматического fallback |
| `GET /api/v1/ai/providers/{id}/operations/{operation_id}` | Durable credential/logout readback; без fingerprint или секрета |
| `GET /api/v1/ai/providers/{id}/models` | Live catalog + provenance/availability; ошибка не означает пустой каталог |
| `POST /api/v1/ai/providers/{id}/verify` | Проверка точных settings/connection generation и required capabilities |
| `GET /api/v1/ai/selection` | Текущая опубликованная revision либо explicit not-configured |
| `PUT /api/v1/ai/selection` | Publication exact verified settings; `If-Match` current revision |
| `GET /api/v1/ai/publications/{operation_id}` | Own durable pending/published/rejected outcome без secrets |
| `GET /api/v1/runtime/ai` | Authenticated secret-free projection; ETag/If-None-Match |

IDs: `chatgpt`, `openrouter`; неизвестный provider не перенаправляется другому.
Login start и disconnect требуют UUID `Idempotency-Key`. Credentials body
содержит UUID operation_id и write-only credential; пользователь не задаёт URL.
Login status/cancel принимают operation ID. Старый повтор disconnect не отзывает
новое подключение. Unknown logout блокирует новый login до native readback;
перезапуск не повторяет logout и не возобновляет старый device-code request.
Истёкшее либо отозванное подтверждение не восстанавливает authorization.
Registry metadata не содержит key, refresh/access tokens, raw CODEX_HOME или
credentials object. В аудит попадают subject, action, provider и revision,
но не body credentials/login code или full provider errors.

## Settings и publication

`ProviderSettings`: provider, model, context_window_tokens. Default 256000;
agent prerequisite >=64000. Поле UI в тысячах токенов преобразуется ровно x1000.
Context metadata должна относиться к точному model ID, а не общему substring.
Draft response дополнительно содержит `model_contexts`: сохранённые model/budget
для собственного provider/workspace. Сохранение выбранной модели обновляет только
её бюджет в одной транзакции с CAS и аудитом. Новая модель получает UI default
256000; переключение возвращает сохранённый бюджет либо незавершённый локальный
ввод. Локальный ввод не хранится в browser storage. Миграция 0010 сохраняет бюджет
ранее выбранной модели; активации или проверки она не выполняет.

`VerificationEvidence` является внутренним результатом доверенного runtime,
не принимается от browser body. Содержит exact settings, credential_generation,
verified model limits/capabilities, verification ID и validity interval.
Metadata/catalog alone не доказывают успешный inference. Реальная проверка
должна завершиться terminal success и подтвердить tools/JSON/stream/cancel.
Trusted adapter proof дополнительно содержит `draft_revision`. Изменение draft
инвалидирует доказательство даже после возврата прежних model/context. Старые
vault proofs без привязки читаются для диагностики, но не разрешают publication.
TTL verification — 15 минут для публикации. Existing run snapshot не истекает
вместе с publication evidence, но connection/model доступ проверяется на запросах.

Publication транзакционно проверяет current revision, exact settings,
connection generation и свежесть evidence. Сначала создаются immutable candidate
и outbox operation; active pointer сохраняет прежнюю revision. Подтверждение
runtime должно совпадать по operation/profile/generation/adapter/accounting policy.
Только после ACK выполняются CAS active pointer и publication audit одной транзакцией.
PUT body: schema_version=1, operation_id, expected_draft_revision, verification_id
и settings. Evidence/credentials/adapter descriptors в human body запрещены.
Strong If-Match имеет вид `"ai-sdlc2-0"` для новой установки либо текущую revision
из ETag GET selection. Начатая операция возвращает 202 и durable operation ID;
после неопределённого исхода использовать GET publication, без новой операции.
Перед переключением повторно сверяются revision, модель и бюджет текущего draft.
Изменение draft после подготовки candidate блокирует активацию; pending operation
сохраняется для readback и явного завершения reconciliation.
На workspace разрешена одна pending operation; unknown outcome удерживает её
до readback. Definitive rejection сохраняет candidate, его revision не используется
повторно. Повтор завершённой операции возвращает прежний результат.
Stale `If-Match` => 412, сохранённый
draft не теряется. Provider credential operation с unknown outcome сверяется
по operation ID; слепой повтор недопустим.

`RuntimeProfile`: schema_version=1, revision, workspace (`sdlc1`/`sdlc2`), provider,
model, context_window_tokens, verification_id. Новый execution сохраняет копию
профиля; global switch не меняет активный execution. Client не может выбрать
другую model/provider в обход profile. В текущем rollout разрешён только SDLC2.

## Внутренняя регистрация runtime

Новый source contract: `POST /internal/v1/profiles/registrations` принимает
`RevisionRegistration` версии 1 с operation ID, immutable profile, credential
generation, draft_revision и descriptors. Доступ — только own `ai:admin`, без agent `ai:infer`.
Evidence от запроса не принимается: proof уже должен находиться в encrypted vault
runtime и совпадать с текущим подключением и deployed adapter/accounting policy.
Version marker сам по себе не доказывает доступ или корректность accounting.
`GET /internal/v1/providers/{provider}/verifications/{id}` возвращает только
runtime-owned proof этого provider/workspace либо null. Доступ — own `ai:admin`.
Historical readback не продлевает TTL и не заменяет проверку current generation.

`GET /internal/v1/profiles/registrations/{operation_id}` возвращает
`RegistrationStatus`: schema_version, workspace, operation_id и receipt либо null.
Это readback исторического результата, не подтверждение текущей доступности
credentials или модели. Admin сверяет receipt с точным pending candidate и
повторно проверяет current connection generation, draft/TTL перед CAS.
Отозванное подключение даёт `runtime_rejected`, сменившаяся generation —
`runtime_conflict`. Неопределённый native authorization checkpoint удерживает pending.
Restore-mode разрешает readback и запрещает
новые регистрации. Повтор POST не запускает inference и не продлевает evidence.
Recovery worker сначала читает статус операции. Подтверждённое отсутствие receipt
разрешает идемпотентную локальную регистрацию; ошибка связи или protocol сохраняет
pending. Доказанные отказы завершают outbox с неизменяемой причиной
`runtime_rejected`, `runtime_conflict`, `draft_changed`, `verification_expired`
либо `operator_rejected`. Повтор отказа не меняет её и не дублирует audit.
Verification producer и UI публикации ещё не подключены. PUT selection, GET
publication и recovery worker прошли Linux unit/clippy, отдельный PostgreSQL gate
и адресно развёрнуты в SDLC2. Live API отклоняет forged browser proof и отсутствие
runtime verification; active profile остаётся пустым. Fixtures не доказывают
реальный доступ к модели. Native device login start/status/cancel проверены
реальным Codex без входа владельца и без inference.

## Контекст и отказ

### Scoped execution и журнал

Inference admission и dispatch HTTP ещё не открыты. Библиотечный контракт из
[ADR-0020](../adr/0020-scoped-ai-execution-journal.md): service identity отдельно
от signed Fleet grant, exact audience `sdlc2-ai-runtime`, owner/project/root/task/
agent/execution, frozen profile revision и monotonic assignment fencing.
Payload/signature проверяются по deployment trust; request не предоставляет key.
Effective authorization не переживает expiry lease (30 секунд); продление
разрешения сохраняет binding. Сам grant verifier не доказывает, что issuer уже
проверяет central subject/project access: эта интеграция остаётся обязательной.

Encrypted journal связывает request ID с полным payload и machine principal.
Replay даёт readback; изменённый payload или binding отклоняется. Root binding
не меняется для children/Rework/auxiliary. Dispatch intent и paid reservation
фиксируются одной записью до внешнего I/O. Restart переводит неизвестные активные
вызовы в `unknown`, не отправляет их повторно и не освобождает стоимость.
Новая assignment generation не присваивает старый неизвестный run.

Отмена до intent терминальна; после intent сохраняется cancellation request до
reconciled provider receipt. Unknown billing удерживает верхний резерв даже после
подтверждённой остановки; later cost readback не повторяет inference. Stream events
монотонны, exact duplicate append идемпотентен; pagination не пропускает неизвестный
cursor. Public status не содержит request transcript, credentials или registration.
Transcript events выдаются только после exact machine/execution ownership.

Пустые journal maps не добавляются в прежний vault wire shape. Первая штатная
execution admission создаст новые encrypted fields; после этого downgrade на
binary без journal запрещён без отдельного forward/restore решения. Эта admission
сейчас недоступна через HTTP. Tool-result continuation реализован в библиотеке:
parent transcript/tools/schema/reserve наследуются, точные results потребляют
parent только один раз атомарно с новым child request. Builtin/необъявленные tools,
duplicate/missing IDs и unknown parent отклоняются. Adapter terminal proof и
его schema validation ещё требуют подключения; fixtures не являются provider probes.

OpenRouter terminal decoder в исходниках проверяет assembled final JSON и tool
arguments по offline schemas, finish/usage/DONE и отсутствие truncation.
Private reasoning/reasoning_details сохраняются отдельным encrypted sidecar и
возвращаются провайдеру при continuation в исходном порядке. Client DTO/events
этих полей не содержат; их байты входят в проверку wire context до reservation.
OpenRouter adapter/accounting descriptors v2 инвалидируют прежнее evidence.
Production HTTPS transport OpenRouter реализован в библиотеке и развёрнут в
SDLC2 2026-10-04. Opaque одноразовый dispatch permit требует durable intent и
paid reservation до POST; redirects/proxy/retries отключены. SSE сверяет generation
ID header/body и terminal decoder. Scoped heartbeat продлевает только тот же run;
истечение lease прекращает ожидание I/O без объявления provider cancellation.
Encrypted generation correlation позволяет GET readback стоимости без повторного
POST. Неизвестный результат сохраняет резерв; metadata stop при потерянном output
не превращается в successful result. Admission HTTP, trusted pricing/framing,
verification producer и terminal-proof orchestration ещё не подключены.
Локальные HTTP fixtures не доказывают реальную совместимость провайдера.

Развёрнутая версия OpenRouter descriptors v3 связывает paid estimate и
`provider.max_price` в финальном wire. Точные decimal USD/million token caps
выводятся из server-owned nanodollars/token, request fee cap=0; параметры входят
в wire context до reservation/intent. Trusted pricing/framing producer и live
подтверждение поддержки ceilings по-прежнему обязательны; каталог не является
верхней оценкой цены endpoint. Полный gate 115 tests/clippy, own Build/Apply и
live negative admission QA прошли; реальные upstream caps ещё не проверены.

Native descriptor v2 развёрнут: закреплённый Codex требует
`features.view_image=false` и обязательного config/read подтверждения вместе
с остальными отключёнными features. `tools.view_image` этим binary игнорируется
и не используется. Offline probe не является verification capabilities.

Подтверждённый billing receipt выше reservation сохраняет actual cost и
`cost_overrun` в encrypted ledger. `cost_ceiling_exceeded` event сохраняется вместе
с provider outcome. Новые paid reservations и dispatch блокируются, включая
подготовленные операции. Restart не снимает блокировку. Это не доказательство
невозможности overbilling у провайдера: подтверждение цен/limits до I/O обязательно.

Следующий internal source contract — POST `/internal/v1/inference/{request_id}/status`,
`/events`, `/cancel`. Status/cancel body — strict signed `GrantEnvelope`; events —
`{grant, from_sequence, limit}`, limit 1..200. Grant не помещается в URL. Caller
machine subject берётся из own service credential; `ai:infer` без subject либо
совмещённый с `ai:admin` не допускается. Optional deployment
`AI_RUNTIME_EXECUTION_TRUST_FILE` содержит schema_version=1, workspace=sdlc2,
issuer=sdlc2-fleet-control, audience=sdlc2-ai-runtime и map key ID → Ed25519 public
key hex. Infer clients без trust блокируют startup. Не создаёт Fleet issuer,
project access или разрешение dispatch; legacy Admin-only clients совместимы.

### Общий бюджет платной приёмки

`GET /api/v1/ai/budget` требует действующую central session. Admin получает
агрегаты только через `GET /internal/v1/budget` собственного ai-runtime с
`ai:admin`; infer/catalog credentials не дают этот доступ. Ответы не кешируются.
Недоступный runtime или некорректный projection возвращают 503/502, без
подмены остатка на $30 или zero ledger. Readback не выполняет provider calls.

Version 1 содержит `schema_version`, `workspace`, `currency=USD`,
`limit_microdollars`, `settled_microdollars`, `reserved_microdollars`,
`uncertain_microdollars`, `available_microdollars`, `unsettled_requests`,
`uncertain_requests`, `blocked_reason`. Все денежные значения — canonical decimal
strings в microdollars, лимит `"30000000"`; операция, prompt, цены отдельных
запросов и credentials не возвращаются.

`reserved` включает Reserved/Dispatched/Uncertain; `uncertain` — его подмножество,
а не дополнительное списание. Settled/CostOverrun учитывают подтверждённую actual
стоимость. CancelledBeforeDispatch не удерживает деньги. Остаток равен
max(0, limit − settled − reserved). Unknown outcome удерживает reserve до
provider reconciliation; expiry pricing или logout не освобождают его.

`blocked_reason` = null, `ai_acceptance_budget_exhausted` либо
`provider_cost_exceeds_reservation`. При overrun блокировка остаётся даже при
положительном остатке. ChatGPT subscription quota учитывается отдельно;
данный readback не утверждает её доступность и не сбрасывает общий $30 ledger.

### Контекст отдельного запроса

User budget отделён от native model limit. Вход учитывает system/instructions,
history, tools/schema и приложения, а не только последний user message.
Reserved output >0 и <= model output limit; input+reserve <= configured budget
<= confirmed physical model limit. Unknown limit => not ready, без 256K fallback.
Увеличившийся server window не меняет configured budget; уменьшившийся limit
вызывает revalidation error. Нельзя считать chars/4 точным tokenizer для всех
моделей: перед deployment адаптер фиксирует tokenizer/accounting policy и запас.

Agent history разрешено компактировать с сохранением обязательного контекста
и tool-call/output pairs. Workflow assignment/evidence не обрезаются. Over-budget
возвращается как `required_context_exceeds_budget`, а не successful empty reply.
Provider mismatch, expired/replaced credentials, unavailable model/quota,
unsupported tool protocol и stream без terminal success — typed errors.
Нет fallback на OpenAI API billing, другой model/provider или соседний runtime.

## Обязательные проверки реализации

ChatGPT native child запускается только при mandatory Linux filesystem isolation
ABI >=3. `native_filesystem_isolation_unavailable` означает невозможность подготовки
или применения policy, либо неподдержанную платформу; подключение не считается
runtime_available, запуск без защиты не выполняется. Policy не открывает broker
key/state/clients, checkout, общий /tmp или /proc для чтения. Native temp/cache
находятся в managed home. Public CA/DNS и exact pinned executable разрешены.
Filesystem restrictions не являются доказательством egress isolation или полной
совместимости JSON/tools/stream/cancel. Adapter descriptor
`codex-app-server-0.159.0-alpha.12.1-v5-readonly-tool-policy` требует свежего evidence.
Deployment задаёт 40 explicit disabled feature overrides и skip_host_skill_discovery.
Raw config/read не считается доказательством effective tools: pinned CLI сохраняет
unified_exec=true даже при false. До фактической проверки toolset и совместимости
capability evidence не выдаётся. Будущий thread/start обязан явно задавать
allowProviderModelFallback=false. Cloud-only ThreadResume.history не используется.

Закреплённая model-owned tool policy поставляется readonly рядом с executable;
runtime проверяет exact bytes, отсутствие links/write permissions и exact config
path. `native_tool_policy_mismatch` блокирует native startup. Каталог сохраняет
exact `gpt-6-luna` и original advertised metadata; ограничиваются только tools.
`agents.enabled` и user-input/update-plan controls отключены отдельно. Actual wire
toolset проверяется отдельно от config/read; candidate fixture подтвердил только
caller tool в functions namespace, без встроенных exec/clock/collaboration.
Readonly production gate и real account/model/capabilities остаются отдельными.
Catalog numbers не являются подтверждённым physical limit или output reserve.

Native initial-turn codec передаёт только ведущие system messages в
thread/start.baseInstructions, сохраняя текст и порядок; Codex выдаёт их на wire
как developer instructions. Это документированное преобразование, не сохранение
literal system role. Поздний system message отклоняется без hoisting. Остальная
история сохраняет developer/user/assistant и завершённые tool-call/result пары.
Последний user message передаётся один раз через turn/start. Complete tool-result
tail продолжается через inject_items и поддержанный пустой turn input, без
синтетического user сообщения. Неполная пара и assistant tail отклоняются.
Configured model_context_window передаётся через поддержанный thread config.
Отсутствие подтверждённого native output bound не разрешает уменьшить reserve.
Codec не создаёт evidence или inference authority; semantic acceptance,
численное accounting и production continuation/dispatch остаются отдельными gate.

Auth/scopes и own issuer; readonly PAT; credential/log redaction; exact evidence
и stale revision; no model/provider override; default/small-context/output budget;
native metadata overwrite; model switching/reasoning replay; JSON/tool arguments;
stream partial error/cancel; quota и paid-call reservation; crash/connection
reconciliation; restart persistence; own backup/restore без внешних вызовов.
