# Screenshot Manifest

README embeds one desktop representative per implemented layout mode.
Responsive captures remain QA evidence and are not part of the README gallery.

Capture source: `frontend/e2e/smoke.spec.ts`, test `captures README interface evidence`.
The test uses stable API mocks, viewport 1920x1080 for desktop and 375x812 for
responsive evidence, and captures full pages after layout readiness and a
document-overflow assertion. Theme is the product default.

| File | Route | Layout | Viewport | PNG dimensions | README |
|---|---|---|---|---|---|
| [overview.png](overview.png) | `/` | `wide` | 1920x1080 | 1920x1080 | yes |
| [services.png](services.png) | `/services` | `wide` | 1920x1080 | 1920x1080 | no |
| [service-detail.png](service-detail.png) | `/services/ci-cd` | `detail-with-aside` | 1920x1080 | 1920x1232 | yes |
| [branding.png](branding.png) | `/branding` | `reading` | 1920x1080 | 1920x1080 | yes |
| [audit.png](audit.png) | `/audit` | `wide` | 1920x1080 | 1920x1080 | no |
| [wide.png](375x812/wide.png) | `/` | `wide` | 375x812 | 375x812 | no |
| [reading.png](375x812/reading.png) | `/branding` | `reading` | 375x812 | 375x812 | no |
| [detail-with-aside.png](375x812/detail-with-aside.png) | `/services/ci-cd` | `detail-with-aside` | 375x812 | 375x812 | no |

The full operational route and breakpoint matrix is asserted by the browser
tests under `frontend/e2e/`; this manifest describes the checked-in captures.

## AI foundation — 2026-10-05

Кадр [ai-foundation.png](ai-foundation.png): `/ai?provider=openrouter`, wide,
Chromium, 1920×1080 viewport, full-page, default theme. Собственный Playwright
context, явные OIDC/API fixtures без реальных аккаунтов или секретов.
Проверены Chromium/Firefox/WebKit и ширины 320, 375, 767, 768, 1279, 1280,
1440, 1920, 2560; responsive кадры хранятся в приватном QA output.
Команда: `pnpm exec playwright test e2e/ai-foundation.spec.ts`.
Fixture приёмка не подтверждает live SSO/provider availability.
