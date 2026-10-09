# Ревью standalone AI runtime builder

PR #33 обновляет только builder в `backend/Dockerfile.ai-runtime`.
Закрепляем точный Rust 1.98.1 согласно `BASE_INTEGRATION.md`, сохраняя
исходный полный Bookworm image и runtime stage. MSRV остаётся 1.88.0.

Изменение не выбирает этот Dockerfile для установленного runtime и не меняет
отдельный `backend/ai-runtime/Dockerfile.umbrella`, Compose или SDK pin.

Проверки: Docker build из tracked candidate и exact pinned Base,
README validator/tests, diff check и полный CI на финальном head.
Первоначальный Dependabot CI не получил private Base checkout token;
его skipped jobs и failed MSRV job не считаются пройденными проверками.
Workflow permissions и обработка secrets не изменяются.
