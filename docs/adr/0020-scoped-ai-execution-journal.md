# ADR-0020: scoped machine grants и encrypted inference journal

Дата: 2026-10-03. Принято для реализации SDLC2; полная приёмка ожидается.

## Решение

Native conversation codec кодирует initial turn без I/O и не выдаёт dispatch
authority. Ведущий system prefix переносится в поддержанный baseInstructions;
на wire Codex это developer role. Interleaved system отклоняется, не переносится
через историю. Last-user input передаётся один раз; исторические tool pairs
сохраняют call ID и отдельный functions namespace. Complete last-tool history
поддерживает пустой turn input без синтетического user сообщения. Durable callback,
numeric native accounting и реальные semantics требуют отдельной приёмки.
Подробности: [план codec](../plans/2026-10-04-native-conversation-codec.md).

Не расширять обычный пользовательский JWT audience `sdlc` до provider access.
Машинный сервис предъявляет своё service credential и отдельное короткое
делегированное разрешение Fleet с audience `sdlc2-ai-runtime`. Fleet остаётся
владельцем назначения и проверяет project access у его владельца; Admin владеет
активным AI-профилем. Runtime не обращается к чужим БД и не принимает решение
о project membership из inference body.

Grant v1 — strict JSON envelope `schema_version`, `key_id`, `payload`,
`signature_hex`. Payload — исходная UTF-8 JSON строка claims; Ed25519 подписывает
байты `SDLC-AI-EXECUTION-V1\0` и эту строку без повторной сериализации.
Это исключает различия JSON canonicalisation между языками. Key ID выбирает
только deployment allowlist публичных ключей собственного issuer. Подпись,
issuer, audience и machine subject обязательны; unknown поля и дубликаты
типизированных claim fields не допускаются. Grant не переживает lease: текущий
verifier ограничивает issued→lease 30 секундами; clock leeway не продлевает lease.
Продление сохраняет scope/revision/fencing.

После проверки signature/claims возникает закрытый `AuthorizedExecution`.
Он не десериализуется из запроса. Его scope и revision сравниваются с request,
root binding и durable run. Новый fencing token не присваивает старый run;
неизвестный исход удерживает запись до owner reconciliation.

Request ID служит ключом durable inference journal. Полный request/transcript
хранится в существующем authenticated encrypted vault. Dispatch intent и
reservation общего $30 ledger записываются атомарно до внешнего I/O. После
crash нет повторного платного dispatch. Отмена после отправки не считается
доказательством остановки или нулевой стоимости.

## Последствия и пределы

Service credential не заменяет scoped grant. Grant не заменяет проверенный
issuer/project access и live lease/fencing у владельца; их интеграция остаётся
обязательной. Пока issuer и provider adapter не приняты, внешняя inference
route не открывается. Изменение active profile не меняет frozen root revision.
Ключ Fleet хранится только у issuer; агенты и broker не получают private key,
provider secrets, Docker credentials или checkout через этот контракт.

После восстановления backup external calls, refresh, dispatch и deployment
выключены; journal допускает readback/reconciliation, но не автоматический I/O.
## Уточнение реализации 2026-10-03

Library tool-result continuation наследует encrypted parent history, tools/schema
и output reserve; parent consumed pointer и Prepared child сохраняются атомарно.
Cost-overrun terminal billing proof не теряется: actual cost и provider outcome
сохраняются, новые платные reservations/dispatch запрещаются после restart.

Readback/cancellation internal HTTP отделены от admission/dispatch. Infer credential
имеет deployment machine subject, не совмещается с Admin credential и требует
отдельный execution trust file. Signed grant передаётся в body. Владение проверяется
по всему execution scope и fencing; unknown и foreign IDs неразличимы. Это не
реализация Fleet issuer/project access и не разрешение платного I/O. Для первого
rollout новые infer clients/trust не добавляются автоматически.

OpenRouter private reasoning history хранится в encrypted run sidecar, привязана
к exact assistant index и наследуется при tool continuation. Она не является
частью клиентского request/events; modified continuation не может её заменить.
Adapter wire accounting включает эти поля. Новые descriptor markers v2 требуют
повторной verification; immutable опубликованные revisions не переписываются.

## Уточнение transport 2026-10-04

Одноразовый OpenRouter permit связывает durable intent/reservation, frozen wire
request и scoped grant до I/O. HTTP клиент не перенаправляет запросы и не повторяет
POST. Lease проверяется во время ожидания; renewal требует неизменности всех
scope/fencing/revision полей. Потеря соединения сама по себе не снимает резерв.

Private journal сохраняет неизменяемый generation ID для GET-only cost readback.
Проверяются exact model/id и terminal metadata; decimal USD округляются вверх
в microdollars без float. Потерянный output с server stop даёт Failed, а не
выдуманный успешный результат. Отдельный cancelled receipt разрешает Cancelled.
Admission HTTP и verification producer остаются закрытыми до их собственной
интеграции и приёмки. Fixtures не публикуют profile evidence.

Paid wire и journal используют одну server-owned cost estimate. До intent
формируются provider price ceilings в точных decimal units USD/million tokens,
с request fee ceiling 0. Финальный wire, включая ceilings и private history,
повторно проверяется на configured context и input/output cost bounds.
Новые descriptors v3 запрещают использовать verification прежнего priced policy.
Цены и подтверждённый framing не принимаются из inference DTO. Реальная
проверка upstream caps и trusted pricing producer остаются prerequisites rollout.

Price producer получает exact-model catalog из bounded fixed-endpoint HTTPS
adapter и создаёт opaque snapshot (generation/hash/TTL 15 минут). Integer
decimal parsing округляет вверх; неизвестная ненулевая fee dimension блокируется.
Snapshot не является capability/access evidence и не открывает inference admission.

Aggregate budget readback не переносит ledger в Admin и не выполняет внешние
запросы. Runtime остаётся единственным владельцем reserve/settlement; human API
возвращает только точные decimal microdollars и причину блокировки. Unknown outcome
отображается как удержанный reserve, подписочная quota отдельно. Ошибка bridge
не трактуется как новый пустой бюджет. UI использует утверждённые Section/facts.

## Native filesystem boundary 2026-10-04

Native child ограничивается mandatory Landlock ABI >=3 до первого exec. Ruleset
подготавливается в broker; pre_exec выполняет только no-new-privileges и
restrict-self. Release policy разрешает exact executable, собственный tmpfs home,
empty read-only workdir, public CA/DNS и минимальные устройства. Broker vault/key,
clients, checkout и /proc не входят в allowlist; общий /tmp не разрешён.
Unsupported kernel/platform или failed enforcement блокируют native startup
с native_filesystem_isolation_unavailable, без небезопасного fallback.

RPC shell fixtures получают interpreter/libraries только cfg(test). Фактический
production policy probe использует библиотеку без cfg(test) и actual pinned
binary в отдельном network-none Compose. Descriptor v3-landlock-fs инвалидирует
старое verification evidence. ABI3 не обеспечивает network port/abstract socket
scopes; filesystem boundary не заменяет tool neutrality и inference compatibility.

Writer lock освобождается explicit unlock при drop Vault: flock не должен жить
за пределами владельца из-за краткой inherited CLOEXEC copy при fork. Exclusivity
живого writer сохраняется и проверяется отдельным regression.

## Native features 2026-10-04

Единый список 40 requested-false overrides задаётся CLI и проверяется config/read;
host skills не обнаруживаются. Закрыты дополнительные browser/computer/code-mode,
implicit elicitation/mentions, goal/sleep, skill install/search, daemon/local
automation, proxy/model fallback и unbounded retries. Descriptor v4-minimal-features
не означает verified capabilities: actual CLI подтвердил 39 effective false,
но нормализует unified_exec обратно в true. Shell_tool=false; фактический registry
и отсутствие внутренних actions/повторного I/O ещё проверяются перед inference.
Auth/account startup под mandatory FS policy проверяется отдельно от model access.

Pinned schema содержит allowProviderModelFallback: будущие thread/start передают
false. History import через ThreadResume.history явно Cloud-only/DO NOT USE и
не используется. Supported persistent thread или thread/inject_items требуют
собственной проверки role semantics, контекста, recovery и tool continuation.

## Model-owned native tool policy 2026-10-04

Actual pinned wire probe обнаружил встроенные exec/user-input/collaboration даже
при disabled feature flags: model metadata задаёт code_mode_only/V2 и clocks.
Поддержанный startup model_catalog_json позволяет ограничить tool selection без
смены exact model. Производная запись gpt-6-luna сохраняет original metadata и
ограничивает только tools; upstream license и source hash сохраняются рядом.

Catalog поставляется root-owned readonly в image рядом с executable, не в managed
home. Compiled byte identity, private path/link check, readonly modes и config
readback обязательны. Landlock добавляет только exact READ_FILE, без прав write,
exec или чтения всего каталога. Дополнительные agents/user-input/update-plan
controls отключены. Descriptor v5-readonly-tool-policy требует нового evidence.

Offline readonly qualification прошла normal-library production startup/config,
actual wire только functions.qa_echo, JSON Schema/SSE, caller continuation,
typed quota terminal и interrupt без retry/fallback. Real model access не проверен.
Pinned Config projection не содержит nested tool controls; includeLayers и exact
sessionFlags values/effective origins обязательны. Gate runtime: 80 tests/clippy.

Raw system history намеренно отбрасывается pinned SDK, хотя inject_items отвечает
успешно. System instructions следует передавать поддержанным baseInstructions,
который SDK кодирует developer message; developer/user/assistant history сохраняет
literal роли. Production codec и real semantic acceptance ещё не реализованы.
Native output hard bound и trusted numeric framing не выводятся из metadata/fixtures.
Admission остаётся закрытым; partial qualification не создаёт capabilities evidence.
