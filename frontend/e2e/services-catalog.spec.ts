import { generateKeyPairSync, sign } from 'node:crypto'
import { expect, test, type Page } from '@playwright/test'

const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' })
const jwk = {
  ...publicKey.export({ format: 'jwk' }),
  kid: 'services-e2e',
  alg: 'ES256',
  use: 'sig',
}
const now = '2026-09-20T08:00:00Z'

function contrastRatio(foreground: string, background: string) {
  function luminance(color: string) {
    const channels = color
      .match(/[\d.]+/g)
      ?.slice(0, 3)
      .map(Number)
    if (!channels || channels.length !== 3) throw new Error(`Invalid color: ${color}`)
    const [red, green, blue] = channels.map((channel) => {
      const value = channel / 255
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
    })
    return 0.2126 * red + 0.7152 * green + 0.0722 * blue
  }
  const values = [luminance(foreground), luminance(background)].sort((a, b) => b - a)
  return (values[0] + 0.05) / (values[1] + 0.05)
}

function idToken(nonce: string) {
  const timestamp = Math.floor(Date.now() / 1000)
  const header = Buffer.from(JSON.stringify({ alg: 'ES256', typ: 'JWT', kid: jwk.kid })).toString(
    'base64url',
  )
  const payload = Buffer.from(
    JSON.stringify({
      iss: 'http://localhost:7701',
      aud: 'admin-panel',
      sub: 'services-e2e-user',
      email: 'services@example.test',
      nonce,
      iat: timestamp,
      exp: timestamp + 900,
    }),
  ).toString('base64url')
  const signature = sign('sha256', Buffer.from(`${header}.${payload}`), {
    key: privateKey,
    dsaEncoding: 'ieee-p1363',
  }).toString('base64url')
  return `${header}.${payload}.${signature}`
}

function entries() {
  return Array.from({ length: 25 }, (_, index) => ({
    id: `service-${index + 1}`,
    service_key: `qa-service-${String(index + 1).padStart(2, '0')}`,
    display_name:
      index === 24
        ? 'Очень длинное название сервиса для проверки узкого экрана'
        : `QA Service ${index + 1}`,
    owner_team: index === 24 ? 'Очень длинное название команды владельца' : 'QA team',
    status: index === 24 ? 'pending' : 'active',
    active_declaration_id: null,
    created_at: now,
    updated_at: now,
    version: 1,
    health_status: 'healthy',
  }))
}

async function installMocks(page: Page) {
  let nonce = ''
  const state: { failGet: boolean; failPost: boolean; releasePost?: () => void; posts: number } = {
    failGet: false,
    failPost: false,
    posts: 0,
  }
  await page.route('http://localhost:7701/oidc/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/oidc/authorize') {
      nonce = url.searchParams.get('nonce') ?? ''
      const callback = new URL(url.searchParams.get('redirect_uri')!)
      callback.searchParams.set('code', 'services-e2e-code')
      callback.searchParams.set('state', url.searchParams.get('state') ?? '')
      await route.fulfill({ status: 302, headers: { location: callback.toString() } })
    } else if (url.pathname === '/oidc/token') {
      await route.fulfill({
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: JSON.stringify({
          access_token: 'services-e2e-token',
          id_token: idToken(nonce),
          expires_in: 900,
        }),
      })
    } else if (url.pathname === '/oidc/jwks') {
      await route.fulfill({
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: JSON.stringify({ keys: [jwk] }),
      })
    } else {
      await route.fulfill({ status: 404 })
    }
  })
  await page.route('**/api/v1/**', async (route) => {
    const { pathname } = new URL(route.request().url())
    if (pathname === '/api/v1/auth/me') {
      await route.fulfill({
        json: {
          subject: 'services-e2e-user',
          email: 'services@example.test',
          panel_role: 'platform_admin',
          capabilities: { mutate: true, manage_bindings: false },
        },
      })
    } else if (pathname === '/api/v1/services' && route.request().method() === 'GET') {
      await route.fulfill(
        state.failGet
          ? {
              status: 503,
              json: { error: { code: 'UNAVAILABLE', message: 'catalog unavailable' } },
            }
          : { json: { services: entries(), total: 25 } },
      )
    } else if (pathname === '/api/v1/services' && route.request().method() === 'POST') {
      state.posts += 1
      await new Promise<void>((resolve) => {
        state.releasePost = resolve
      })
      await route.fulfill(
        state.failPost
          ? { status: 503, json: { error: { code: 'UNAVAILABLE', message: 'create unavailable' } } }
          : { json: { service: { ...entries()[0], id: 'created-service' } } },
      )
    } else if (pathname === '/api/v1/runtime/services') {
      await route.fulfill({ json: { services: [] } })
    } else if (pathname === '/api/v1/runtime/branding') {
      await route.fulfill({ json: { branding: null } })
    } else {
      await route.fulfill({ status: 404, json: { error: { code: 'UNMOCKED' } } })
    }
  })
  return state
}

test('catalog paginates and fits 320–1920px in every theme', async ({ page }) => {
  await installMocks(page)
  await page.goto('/services')
  await expect(page.getByRole('heading', { name: 'Каталог сервисов' })).toBeVisible()
  await page.addStyleTag({
    content: '*, *::before, *::after { transition: none !important; animation: none !important; }',
  })
  await expect(page.getByRole('link', { name: /QA Service/ })).toHaveCount(20)
  await expect(page.getByText('Показано 20 из 25 сервисов')).toBeVisible()

  for (const width of [320, 375, 768, 1280, 1920]) {
    await page.setViewportSize({ width, height: width < 768 ? 812 : 900 })
    for (const theme of ['light', 'gray', 'dark']) {
      await page.evaluate(
        (value) => document.documentElement.setAttribute('data-theme', value),
        theme,
      )
      await expect
        .poll(() =>
          page.evaluate(
            () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
          ),
        )
        .toBe(0)
      const background = await page
        .getByRole('combobox', { name: 'Состояние сервиса' })
        .evaluate((select) => getComputedStyle(select).backgroundColor)
      expect(background).not.toBe('transparent')
      const rgba = background.match(/^rgba\([^,]+,[^,]+,[^,]+,\s*([\d.]+)\)$/)
      expect(rgba ? Number(rgba[1]) : 1).toBe(1)
      const smallControls = await page.locator('main button, main select').evaluateAll((controls) =>
        controls.flatMap((control) => {
          const { width: controlWidth, height } = control.getBoundingClientRect()
          return controlWidth > 0 && height > 0 && (controlWidth < 40 || height < 40)
            ? [
                `${control.textContent?.trim()}: ${controlWidth}x${height}, viewport=${innerWidth}, sm=${matchMedia('(width >= 40rem)').matches}, min-height=${getComputedStyle(control).minHeight}, class=${control.className}`,
              ]
            : []
        }),
      )
      expect(smallControls).toEqual([])
      const colors = await page.evaluate(() => {
        const input = document.querySelector('input[type="search"]')!
        const name = document.querySelector('main a[href^="/services/"] span span')!
        const surface = document.querySelector('main a[href^="/services/"]')!.parentElement!
        return {
          input: getComputedStyle(input).backgroundColor,
          placeholder: getComputedStyle(input, '::placeholder').color,
          name: getComputedStyle(name).color,
          surface: getComputedStyle(surface).backgroundColor,
        }
      })
      expect(
        contrastRatio(colors.placeholder, colors.input),
        JSON.stringify({ width, theme, colors }),
      ).toBeGreaterThanOrEqual(4.5)
      expect(
        contrastRatio(colors.name, colors.surface),
        JSON.stringify({ width, theme, colors }),
      ).toBeGreaterThanOrEqual(4.5)
      if (width === 768) {
        const nameOverflow = await page
          .locator('main a[href="/services/qa-service-01"] span span')
          .evaluate((name) => name.scrollWidth - name.clientWidth)
        expect(nameOverflow).toBe(0)
      }
      if (
        (width === 375 && theme === 'light') ||
        (width === 768 && theme === 'gray') ||
        (width === 1280 && theme === 'dark')
      ) {
        await page.screenshot({
          path: `test-results/services-catalog-${width}-${theme}.png`,
          fullPage: true,
        })
      }
    }
  }

  await page.getByRole('button', { name: 'Далее' }).click()
  await expect(page.getByText('Показано 5 из 25 сервисов')).toBeVisible()
  await page.getByRole('combobox', { name: 'Состояние сервиса' }).selectOption('pending')
  await expect(page.getByText('1 / 1')).toBeHidden()
  await expect(page.getByText('Показано 1 из 1 сервисов')).toBeVisible()
  await page.getByRole('searchbox', { name: 'Найти сервис' }).fill('absent')
  await expect(page.getByText('По заданным условиям сервисы не найдены.')).toBeVisible()
})

test('catalog hides stale rows on refetch error and keeps create draft on failure', async ({
  page,
}) => {
  const state = await installMocks(page)
  await page.setViewportSize({ width: 1280, height: 800 })
  await page.goto('/services')
  await expect(page.getByText('Показано 20 из 25 сервисов')).toBeVisible()
  state.failGet = true
  await page.getByRole('link', { name: 'Обзор' }).click()
  await page.getByRole('link', { name: 'Каталог сервисов' }).click()
  await expect(page.getByRole('alert')).toContainText('Не удалось загрузить сервисы', {
    timeout: 15000,
  })
  await expect(page.getByText('Показано 20 из 25 сервисов')).toBeHidden()
  state.failGet = false
  await page.getByRole('button', { name: 'Повторить' }).click()
  await expect(page.getByText('Показано 20 из 25 сервисов')).toBeVisible()

  await page.getByRole('button', { name: 'Добавить сервис' }).click()
  await page.getByLabel('Ключ сервиса').fill('qa-created')
  await page.getByLabel('Название').fill('QA Created')
  await page.getByLabel('Команда-владелец').fill('QA team')
  await page.getByLabel('Базовый URL (HTTPS или localhost)').fill('http://localhost:9000')
  state.failPost = true
  await page.getByRole('button', { name: 'Создать сервис' }).click()
  await expect.poll(() => state.posts).toBe(1)
  await expect(page.getByRole('button', { name: 'Закрыть форму' })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Отмена' })).toBeDisabled()
  await expect(page.getByLabel('Ключ сервиса')).toBeDisabled()
  state.releasePost?.()
  await expect(page.getByRole('alert')).toContainText('create unavailable')
  await expect(page.getByLabel('Ключ сервиса')).toHaveValue('qa-created')
  state.failPost = false
  await page.getByRole('button', { name: 'Создать сервис' }).click()
  await expect.poll(() => state.posts).toBe(2)
  state.releasePost?.()
  await expect(page.getByRole('button', { name: 'Добавить сервис' })).toBeVisible()
})
