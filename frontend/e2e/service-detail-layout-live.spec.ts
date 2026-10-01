import { mkdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

test.skip(process.env.SDLC_LIVE_QA !== '1', 'Requires live Admin and Central Auth')
test.skip(({ browserName }) => browserName !== 'chromium', 'Single browser live acceptance')
test.use({ trace: 'off' })

const account =
  process.env.SDLC_LIVE_QA === '1'
    ? (JSON.parse(
        readFileSync(
          process.env.SDLC_QA_SESSION_FILE ??
            fileURLToPath(
              new URL('../../../services-base/deploy/.local/qa-session.json', import.meta.url),
            ),
          'utf8',
        ),
      ) as { email: string; password: string })
    : { email: '', password: '' }
const base = process.env.PLAYWRIGHT_BASE_URL ?? 'http://localhost:7772'
const screenshots = fileURLToPath(
  new URL('../../../.local/screenshots/admin-detail-layout/', import.meta.url),
)

test('service detail has a 320 px shared rail from 1024 and stacks after primary below it', async ({
  page,
}) => {
  test.setTimeout(600_000)
  mkdirSync(screenshots, { recursive: true })
  const errors: string[] = []
  const mutations: string[] = []
  page.on('pageerror', (error) => errors.push(`page: ${error.message}`))
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(`console: ${message.text()}`)
  })
  page.on('requestfailed', (request) => {
    const reason = request.failure()?.errorText ?? 'unknown'
    if (!reason.includes('ERR_ABORTED'))
      errors.push(`${request.method()} ${new URL(request.url()).pathname}: ${reason}`)
  })
  page.on('response', (response) => {
    if (response.url().includes('/api/v1/') && response.status() >= 400)
      errors.push(`${response.status()} ${new URL(response.url()).pathname}`)
  })
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/') && request.method() !== 'GET')
      mutations.push(`${request.method()} ${new URL(request.url()).pathname}`)
  })

  await page.goto(`${base}/services/admin-panel`)
  await page.getByLabel('Email').fill(account.email)
  await page.getByLabel('Пароль').fill(account.password)
  await page.getByRole('button', { name: 'Войти', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Активный контракт интеграции' })).toBeVisible()

  for (const theme of ['light', 'gray', 'dark']) {
    await page.evaluate((value) => localStorage.setItem('theme', value), theme)
    for (const key of ['admin-panel', 'central-auth']) {
      await page.goto(`${base}/services/${key}`)
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
      const layout = page.locator('.page-split')
      const rail = layout.getByRole('complementary', { name: 'Состояние и действия сервиса' })
      await expect(rail).toBeVisible()
      await expect(rail.getByText(key, { exact: true })).toBeVisible()
      for (const [width, height] of [
        [375, 812],
        [768, 1024],
        [1023, 800],
        [1024, 800],
        [1279, 800],
        [1280, 800],
        [1440, 900],
        [1920, 1080],
        [2560, 1440],
      ]) {
        await page.setViewportSize({ width, height })
        await page.evaluate(() => window.scrollTo(0, 0))
        const dimensions = await layout.evaluate((element) => {
          const primary = element.firstElementChild!.getBoundingClientRect()
          const aside = element.querySelector('aside')!.getBoundingClientRect()
          return {
            primary: {
              x: primary.x,
              y: primary.y,
              right: primary.right,
              bottom: primary.bottom,
              width: primary.width,
            },
            aside: { x: aside.x, y: aside.y, width: aside.width },
            gap: parseFloat(getComputedStyle(element).columnGap),
          }
        })
        if (width >= 1024) {
          expect(dimensions.aside.width, `${key} ${theme} ${width} rail`).toBeCloseTo(320, 0)
          expect(dimensions.aside.x - dimensions.primary.right).toBeCloseTo(dimensions.gap, 0)
          expect(dimensions.aside.y).toBeCloseTo(dimensions.primary.y, 0)
        } else {
          expect(dimensions.aside.width).toBeCloseTo(dimensions.primary.width, 0)
          expect(dimensions.aside.x).toBeCloseTo(dimensions.primary.x, 0)
          expect(dimensions.aside.y).toBeGreaterThanOrEqual(dimensions.primary.bottom)
        }
        expect(
          await page.evaluate(
            () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
          ),
          `${key} ${theme} ${width} overflow`,
        ).toBeLessThanOrEqual(1)
        await page.screenshot({
          path: `${screenshots}/${key}-${theme}-${width}.png`,
          fullPage: true,
          animations: 'disabled',
        })
      }
      for (const width of [375, 2560]) {
        await page.setViewportSize({ width, height: 812 })
        for (const name of ['Отключить', 'Вывести из эксплуатации']) {
          const trigger = rail.getByRole('button', { name, exact: true })
          await trigger.focus()
          await page.keyboard.press('Enter')
          await expect(page.getByRole('dialog')).toBeVisible()
          await page.keyboard.press('Escape')
          await expect(page.getByRole('dialog')).toBeHidden()
          await expect(trigger).toBeFocused()
        }
      }
    }
  }
  expect(mutations, 'Geometry and confirmation-cancel must not write data').toEqual([])
  expect(errors).toEqual([])
})
