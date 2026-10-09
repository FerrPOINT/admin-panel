# Переход AI-функций Admin в AI Hub

Статус: Target approved, документация. Текущий Admin AI_PROFILE_V1 действует до cutover.
Переносятся разделы /ai: активный профиль/ревизии, бюджет платной приёмки, ChatGPT и
OpenRouter connections, ключ и managed login, каталог/поиск/выбор модели, отдельный
сохранённый context budget для каждой модели, CAS drafts/proof/publication и readback.
Сервисный ai-runtime станет источником требований target adapters/vault/ledger/journal.
Наличие кода Admin не означает target Hub qualification; старый proof не активирует Hub.

Admin остаётся владельцем branding/service catalogue/Namespace/integration metadata.
Identity/PAT принадлежат Central Auth. Fleet/Tracker/Workflow сохраняют grants/Task/tools.
После verified cutover карточка сервиса даёт status и ссылку. /ai направляет в Hub,
сохраняя валидную Namespace UUID-пару; секреты, grants и drafts не входят в URL.
API /api/v1/ai и runtime/ai не перенаправляют автоматически credential/inference POST.
Compatibility adapter и отключение source writes — отдельная поставка с rollback.

Полный mapping fields/API/stores, unit conversions и stop gates:
[AI Hub handoff](https://github.com/FerrPOINT/ai-hub/blob/main/docs/contracts/ADMIN_HANDOFF_V1.md).
Owner plan: [выделение](../plans/2026-10-09-ai-hub-extraction.md).

## Уточнение target transport и расходов

Reviewed Hub docs `45508cb53430c2f85d11a45b973672da81d36e84`: [SERVICE_ADAPTER_V1](https://github.com/FerrPOINT/ai-hub/blob/45508cb53430c2f85d11a45b973672da81d36e84/docs/contracts/SERVICE_ADAPTER_V1.md). Новый audience/domain, signed raw-body binding, exact Namespace/V2/profile revision/request identity, lease/fencing и revoke tombstones. Legacy runtime envelope/ordinal/workspace не принимается автоматически. Перенос signer trust и подключение consumer требует отдельной owner acceptance.

Project tariffs/source currencies/effective intervals и CAS принадлежат Hub; default 20% за 1M не создаёт receipt. Старые Admin microdollar aggregate/reserve сохраняются без выдуманной истории; actual source writes остаются до accepted cutover. Документационная ветка не меняет main/runtime.
