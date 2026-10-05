# ADR-0018: AI-профиль и отдельный runtime

Дата: 2026-10-02. Решение пользователя принято; live приёмка ещё не выполнена.

Admin владеет реестром подключений и публикацией неизменяемого AI-профиля.
Inference и provider credentials принадлежат отдельному executable `ai-runtime`
в этом репозитории, отдельному own application service. Admin не становится
универсальным HTTP proxy/remote executor и не читает чужие БД. Base/Auth не
приобретают доменную AI-логику. Существующая policy доступа Admin сохраняется.

Все inference clients получают revision/provider/model/context budget без
секретов. Runtime выдаёт ошибки при недоступных provider/capability, без
скрытого fallback. Codex managed authorization хранится в own CODEX_HOME;
global auth и neighbor credentials не используются. Native model metadata
отделена от выбранного бюджета 256000; применение обязательно на запросах,
сжатии и вспомогательных вызовах. Активный execution фиксирует revision.

Credentials вводятся write-only и не попадают в registry/audit/logs/browser
storage. Restore state не включает OAuth refresh или внешние вызовы.
Новая feature resource migration не является разрешением пересоздать старое
обязательное хранилище. Preview approval предшествует live UI integration.

## Первый исполняемый компонент

`backend/ai-runtime` — Rust 1.88/Axum, отдельный workspace member. Credential
state использует AES-256-GCM (`ring`), случайный nonce и workspace как AAD.
Запись — private temporary file, fsync и atomic rename; process writer lock
(`fs2`) не позволяет двум экземплярам терять изменения. Ключ — отдельный
32-byte файл вне state directory; secret buffers очищаются через `zeroize`.
Обычный запуск требует owner marker, vault и ключ. Создание разрешено только
явной командой `init`; непустой каталог и существующий ключ не заменяются.

Internal HTTP аутентифицирует отдельные service tokens из private файла:
`ai:admin`, `ai:catalog`, `ai:infer`. Агентский `ai:infer` не разрешает credential
operations. Ответы включают только metadata, никакого generic upstream proxy.
Endpoint OpenRouter закреплён deployment code, redirects отключены.

Codex transport — stdio JSON-RPC, отдельный процесс с очищенным environment,
own HOME/CODEX_HOME и пустым workdir. Версия проверяется до initialize.
Schema declaration не считается live capability. Managed login имеет durable
operation state и зашифрованный auth checkpoint; расшифрованный auth.json разрешён
только на tmpfs. Tool requests пока отклоняются. До полной проверки native tool
continuation, контекста и inference активация ChatGPT недоступна.
Прототип не должен возвращать fabricated verification evidence.

Ключи сервисов и ciphertext не передаются агентам. OpenRouter catalog и native
model/list показывают `access_verified=false`. Native model/list текущей версии
не даёт подтверждённых context/output limits; runtime не подставляет 256K в
качестве физического предела. Runtime-процесс и live UI развёрнуты 3 октября в
SDLC2; verification/inference ещё не приняты.

3 октября: добавлен typed Admin bridge через единственный deployment endpoint
`http://ai-runtime:8760/`, без redirects, proxy env и retries. Credentials, login,
logout и catalog не принимают пользовательский endpoint. Write намерения
фиксируются в secret-free audit; unknown outcomes читаются по operation ID.
Ledger $30 хранится в encrypted state и удерживает резерв неопределённого запроса;
подключение к inference ещё требуется. Linux native binary закреплён на
0.159.0-alpha.12.1 с SHA-512; handshake/account-read/catalog проверены без сети,
авторизации и платных вызовов. Новый rollout требует отдельной feature migration.

Публикация использует immutable pending revision и transactional outbox Admin.
Worker читает runtime registration receipt перед доставкой и только затем
выполняет CAS active pointer с повторной проверкой draft/TTL. HTTP timeout не
освобождает pending. Human API передаёт только verification ID; runtime-owned
proof дополнительно связан с draft revision, поэтому возврат старых settings
после редактирования не возобновляет старую проверку. Legacy proof без binding
не разрешает новую регистрацию. Terminal rejection reason и audit атомарны.
Это foundation; production verification producer и inference ещё не приняты.
