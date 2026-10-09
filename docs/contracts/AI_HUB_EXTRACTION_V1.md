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
