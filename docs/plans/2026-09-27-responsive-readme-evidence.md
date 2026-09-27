# План: адаптивные UI-скриншоты README

## Цель

Обновить визуальные доказательства Admin Panel после унификации рабочей области:
desktop-страницы должны использовать общую геометрию shell, а мобильные кадры —
подтверждать отсутствие горизонтального переполнения для всех семантических
режимов контента.

## Изменения

- добавить opt-in Playwright-сценарий пересъёмки README на детерминированных моках;
- переснять существующие страницы README при `1920x1080`;
- добавить мобильные кадры `wide`, `reading` и `detail-with-aside` при `375x812`;
- обновить README и проверить локальные ссылки валидатором репозитория.

## Проверка

- `UPDATE_README_SCREENSHOTS=1 pnpm exec playwright test e2e/smoke.spec.ts --grep "captures README" --project=chromium`
- `pnpm lint`
- `pnpm typecheck`
- `pnpm test`
- `pnpm build`
- `python scripts/verify_readme.py`
