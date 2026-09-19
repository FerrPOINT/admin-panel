import { generateKeyPairSync, sign } from 'node:crypto'
import { resolve } from 'node:path'
import { expect, test, type Page } from '@playwright/test'

const screenshotDir = resolve('..', 'docs', 'assets', 'screens', '2026-09-19-audit')
const now = '2026-09-19T08:00:00Z'
const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' })
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'audit-e2e', alg: 'ES256', use: 'sig' }

function idToken(nonce: string) {
  const timestamp = Math.floor(Date.now() / 1000)
  const header = Buffer.from(JSON.stringify({ alg: 'ES256', typ: 'JWT', kid: jwk.kid })).toString('base64url')
  const payload = Buffer.from(JSON.stringify({
    iss: 'http://localhost:7701', aud: 'admin-panel', sub: 'audit-e2e-user',
    email: 'audit@example.test', nonce, iat: timestamp, exp: timestamp + 900,
  })).toString('base64url')
  const signature = sign('sha256', Buffer.from(`${header}.${payload}`), {
    key: privateKey,
    dsaEncoding: 'ieee-p1363',
  }).toString('base64url')
  return `${header}.${payload}.${signature}`
}

function events(count: number) {
  return Array.from({ length: count }, (_, index) => ({
    id: `event-${index}`,
    occurred_at: now,
    request_id: `request-${index}`,
    actor_subject: 'audit-e2e-user',
    actor_role: 'platform_admin',
    action: index < 20 ? 'central_user.created' : 'branding.published',
    entity_type: index < 20 ? 'central_user' : 'branding_revision',
    entity_id: null,
    metadata: {},
  }))
}

async function installMocks(page: Page, count: number, tokens: unknown[] = []) {
  let nonce = ''
  const auditRequests: string[] = []
  const mutations: string[] = []
  const unmocked: string[] = []
  const state = { failAudit: false }
  const dataset = events(count)

  page.on('request', (request) => {
    if (request.url().includes('/api/v1/') && request.method() !== 'GET') {
      mutations.push(`${request.method()} ${new URL(request.url()).pathname}`)
    }
  })
  await page.route('http://localhost:7701/oidc/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/oidc/authorize') {
      nonce = url.searchParams.get('nonce') ?? ''
      const callback = new URL(url.searchParams.get('redirect_uri')!)
      callback.searchParams.set('code', 'audit-e2e-code')
      callback.searchParams.set('state', url.searchParams.get('state') ?? '')
      await route.fulfill({ status: 302, headers: { location: callback.toString() } })
    } else if (url.pathname === '/oidc/token') {
      await route.fulfill({
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: JSON.stringify({ access_token: 'audit-e2e-token', id_token: idToken(nonce), expires_in: 900 }),
      })
    } else if (url.pathname === '/oidc/jwks') {
      await route.fulfill({
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: JSON.stringify({ keys: [jwk] }),
      })
    } else {
      unmocked.push(url.pathname)
      await route.fulfill({ status: 404 })
    }
  })
  await page.route('**/api/v1/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/api/v1/auth/me') {
      await route.fulfill({ json: {
        subject: 'audit-e2e-user', email: 'audit@example.test', panel_role: 'platform_admin',
        capabilities: { mutate: true, manage_bindings: false },
      } })
    } else if (url.pathname === '/api/v1/audit-events') {
      auditRequests.push(url.search)
      if (state.failAudit) {
        await route.fulfill({ status: 503, json: { error: { code: 'UNAVAILABLE' } } })
        return
      }
      const filtered = dataset.filter((item) =>
        (!url.searchParams.has('action') || item.action === url.searchParams.get('action')) &&
        (!url.searchParams.has('entity_type') || item.entity_type === url.searchParams.get('entity_type')),
      )
      const offset = Number(url.searchParams.get('offset') ?? 0)
      const limit = Number(url.searchParams.get('limit') ?? 50)
      await route.fulfill({ json: { events: filtered.slice(offset, offset + limit), total: filtered.length } })
    } else if (url.pathname === '/api/v1/tokens') {
      if (route.request().method() === 'POST') {
        await route.fulfill({ json: {
          id: 'qa-issued', label: 'qa-focus', scopes: ['wiki:read'], secret: 'qa-secret',
          expires_at: '2099-01-01T00:00:00Z', created_at: now,
          last_used_at: null, revoked_at: null,
        } })
      } else {
        await route.fulfill({ json: tokens })
      }
    } else if (url.pathname === '/api/v1/runtime/services') {
      await route.fulfill({ json: { services: [] } })
    } else if (url.pathname === '/api/v1/runtime/branding') {
      await route.fulfill({ json: { branding: null } })
    } else {
      unmocked.push(url.pathname)
      await route.fulfill({ status: 404, json: { error: { code: 'UNMOCKED' } } })
    }
  })
  return { auditRequests, mutations, unmocked, state }
}

test('audit filters and pagination stay compact without extra requests', async ({ page }) => {
  const traffic = await installMocks(page, 40)
  await page.goto('/audit')
  await expect(page.getByRole('heading', { name: 'Аудит изменений' })).toBeVisible()
  await expect(page.getByText('1–20 из 40')).toBeVisible()

  for (const width of [375, 1280, 1920, 2560]) {
    await page.setViewportSize({ width, height: width === 375 ? 812 : 900 })
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(0)
    const small = await page.locator('main button, main select, main summary').evaluateAll((controls) =>
      controls.filter((control) => {
        const rect = control.getBoundingClientRect()
        return rect.width > 0 && rect.height > 0 && (rect.width < 40 || rect.height < 40)
      }).length,
    )
    expect(small).toBe(0)
    await page.screenshot({ path: resolve(screenshotDir, `audit-${width}.png`), fullPage: true })
  }

  await page.setViewportSize({ width: 375, height: 812 })
  await page.getByRole('button', { name: 'Вперёд' }).click()
  await expect(page.getByText('21–40 из 40')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Вперёд' })).toBeDisabled()
  await page.getByRole('combobox', { name: 'Тип сущности' }).selectOption('central_user')
  await expect(page.getByText('1–20 из 20')).toBeVisible()
  expect(traffic.auditRequests.at(-1)).toContain('entity_type=central_user')

  await page.getByRole('combobox', { name: 'Действие' }).selectOption('central_user.created')
  await expect.poll(() => traffic.auditRequests.at(-1)).toContain('action=central_user.created')
  const beforeModeChange = traffic.auditRequests.length
  await page.getByRole('combobox', { name: 'Действие' }).selectOption('custom')
  expect(traffic.auditRequests).toHaveLength(beforeModeChange)
  const beforeTyping = traffic.auditRequests.length
  await page.getByRole('textbox', { name: 'Точный код действия' }).pressSequentially('branding.published')
  expect(traffic.auditRequests).toHaveLength(beforeTyping)
  await page.getByRole('button', { name: 'Применить' }).click()
  await expect(page.getByText('0–0 из 0')).toBeVisible()
  expect(traffic.auditRequests.at(-1)).toContain('action=branding.published')
  await page.screenshot({ path: resolve(screenshotDir, 'audit-exact-empty-375.png'), fullPage: true })
  expect(traffic.mutations).toEqual([])
  expect(traffic.unmocked).toEqual([])
})

test('exactly 20 events end on the first page', async ({ page }) => {
  const traffic = await installMocks(page, 20)
  await page.goto('/audit')
  await expect(page.getByText('1–20 из 20')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Вперёд' })).toBeDisabled()
  expect(traffic.mutations).toEqual([])
  expect(traffic.unmocked).toEqual([])
})

test('audit load error offers retry without losing the filters', async ({ page }) => {
  const traffic = await installMocks(page, 40)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.goto('/audit')
  await expect(page.getByText('1–20 из 40')).toBeVisible()
  traffic.state.failAudit = true
  await page.getByRole('combobox', { name: 'Тип сущности' }).selectOption('central_user')
  await expect(page.getByRole('alert')).toBeVisible({ timeout: 15000 })
  await expect(page.getByText('Число событий недоступно')).toBeVisible()
  const retryHeight = await page.getByRole('button', { name: 'Повторить' }).evaluate((button) => button.getBoundingClientRect().height)
  expect(retryHeight).toBeGreaterThanOrEqual(40)
  await page.screenshot({ path: resolve(screenshotDir, 'audit-error-375.png'), fullPage: true })
  traffic.state.failAudit = false
  await page.getByRole('button', { name: 'Повторить' }).click()
  await expect(page.getByText('1–20 из 20')).toBeVisible()
  await expect(page.getByRole('combobox', { name: 'Тип сущности' })).toHaveValue('central_user')
  expect(traffic.mutations).toEqual([])
  expect(traffic.unmocked).toEqual([])
})

test('token scope checkboxes have 40px labels and fit the mobile dialog', async ({ page }) => {
  const traffic = await installMocks(page, 0)
  await page.setViewportSize({ width: 375, height: 812 })
  await page.goto('/tokens')
  await expect(page.getByRole('heading', { name: 'Личные API-токены' })).toBeVisible()
  await page.getByRole('button', { name: 'Создать' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  const geometry = await dialog.locator('fieldset label').evaluateAll((labels) =>
    labels.map((label) => ({ width: label.getBoundingClientRect().width, height: label.getBoundingClientRect().height })),
  )
  expect(geometry).toHaveLength(12)
  expect(geometry.every(({ width, height }) => width >= 40 && height >= 40)).toBe(true)
  const contrast = await dialog.locator('form label').first().evaluate((label) => {
    const channels = (color: string) => [...color.matchAll(/\d+/g)].slice(0, 3).map((match) => Number(match[0]) / 255)
    const luminance = (color: string) => {
      const [red, green, blue] = channels(color).map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
      return 0.2126 * red + 0.7152 * green + 0.0722 * blue
    }
    const foreground = luminance(getComputedStyle(label).color)
    const background = luminance(getComputedStyle(label.closest('[role="dialog"]')!).backgroundColor)
    return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05)
  })
  expect(contrast).toBeGreaterThanOrEqual(4.5)
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBe(0)
  await page.screenshot({ path: resolve(screenshotDir, 'token-create-375.png'), fullPage: true })
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(page.getByRole('button', { name: 'Создать', exact: true })).toBeFocused()
  expect(traffic.mutations).toEqual([])
  expect(traffic.unmocked).toEqual([])
})

test('canceling token revocation returns focus to its action', async ({ page }) => {
  const traffic = await installMocks(page, 0, [{
    id: 'qa-token', label: 'QA token', scopes: ['wiki:read'],
    expires_at: '2099-01-01T00:00:00Z', created_at: now,
    last_used_at: null, revoked_at: null,
  }])
  await page.goto('/tokens')
  const action = page.getByRole('button', { name: 'Отозвать QA token' })
  await action.click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(action).toBeFocused()
  expect(traffic.mutations).toEqual([])
  expect(traffic.unmocked).toEqual([])
})

test('issued token dialog returns focus to create after dismissal', async ({ page }) => {
  const traffic = await installMocks(page, 0)
  await page.goto('/tokens')
  const create = page.getByRole('button', { name: 'Создать', exact: true })
  await create.click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('textbox', { name: 'Название' }).fill('qa-focus')
  await dialog.getByText('Wiki').locator('..').getByRole('checkbox', { name: 'Чтение' }).check()
  await dialog.getByRole('button', { name: 'Создать', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Секрет токена' })).toBeVisible()
  await page.getByRole('button', { name: 'Готово' }).click()
  await expect(page.getByRole('dialog')).toBeHidden()
  await expect(create).toBeFocused()
  expect(traffic.mutations).toEqual(['POST /api/v1/tokens'])
  expect(traffic.unmocked).toEqual([])
})
