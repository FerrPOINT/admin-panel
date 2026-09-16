import { defineConfig, devices } from '@playwright/test'

const useExternalServer = Boolean(process.env.PLAYWRIGHT_BASE_URL)
const previewPort = process.env.PLAYWRIGHT_PREVIEW_PORT ?? '4177'
const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? `http://localhost:${previewPort}`
const reuseExistingServer = process.env.PLAYWRIGHT_REUSE_SERVER === '1'

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'list',
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'firefox',
      use: { ...devices['Desktop Firefox'] },
    },
    {
      name: 'webkit',
      use: { ...devices['Desktop Safari'] },
    },
  ],
  webServer: useExternalServer
    ? undefined
    : {
        command: 'node scripts/playwright-preview.mjs',
        url: baseURL,
        reuseExistingServer,
      },
})
