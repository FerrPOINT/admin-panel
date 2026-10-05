import { generateKeyPairSync, sign } from 'node:crypto'
import { expect, test, type Page } from '@playwright/test'

const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' })
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'ai-qa', alg: 'ES256', use: 'sig' }

async function fixtures(page: Page) {
  let nonce = ''
  let conflict = false
  const saves: { body: unknown; revision: string | undefined }[] = []
  await page.route('http://localhost:7701/oidc/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/oidc/authorize') {
      nonce = url.searchParams.get('nonce') ?? ''
      const callback = new URL(url.searchParams.get('redirect_uri')!)
      callback.searchParams.set('code', 'ai-fixture-code')
      callback.searchParams.set('state', url.searchParams.get('state') ?? '')
      await route.fulfill({
        contentType: 'text/html; charset=utf-8',
        body: `<meta charset="utf-8"><a href=${JSON.stringify(callback.toString())}>Continue QA sign-in</a>`,
      })
    } else if (url.pathname === '/oidc/token') {
      const timestamp = Math.floor(Date.now() / 1000)
      const header = Buffer.from(JSON.stringify({ alg: 'ES256', kid: jwk.kid })).toString(
        'base64url',
      )
      const payload = Buffer.from(
        JSON.stringify({
          iss: 'http://localhost:7701',
          aud: 'admin-panel',
          sub: 'ai-fixture',
          email: 'ai-fixture@example.invalid',
          nonce,
          iat: timestamp,
          exp: timestamp + 900,
        }),
      ).toString('base64url')
      const signature = sign('sha256', Buffer.from(`${header}.${payload}`), {
        key: privateKey,
        dsaEncoding: 'ieee-p1363',
      }).toString('base64url')
      await route.fulfill({
        headers: { 'access-control-allow-origin': '*' },
        json: {
          access_token: 'fixture-only-access-token',
          id_token: `${header}.${payload}.${signature}`,
          expires_in: 900,
        },
      })
    } else if (url.pathname === '/oidc/jwks') {
      await route.fulfill({
        headers: { 'access-control-allow-origin': '*' },
        json: { keys: [jwk] },
      })
    } else await route.fulfill({ status: 404 })
  })
  const drafts = [
    {
      settings: { provider: 'chatgpt', model: 'gpt-6-luna', context_window_tokens: 256000 },
      draft_revision: 1,
      updated_at: '',
      model_contexts: [{ model: 'gpt-6-luna', context_window_tokens: 256000 }],
    },
    {
      settings: {
        provider: 'openrouter',
        model: 'deepseek/deepseek-v4.1-flash',
        context_window_tokens: 256000,
      },
      draft_revision: 1,
      updated_at: '',
      model_contexts: [
        { model: 'deepseek/deepseek-v4.1-flash', context_window_tokens: 256000 },
        { model: 'second-fixture', context_window_tokens: 192000 },
      ],
    },
  ]
  await page.route('**/api/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/v1/auth/me') {
      await route.fulfill({
        json: {
          subject: 'ai-fixture',
          panel_role: 'platform_viewer',
          capabilities: { mutate: true, manage_bindings: false },
        },
      })
    } else if (path === '/api/v1/runtime/services') await route.fulfill({ json: { services: [] } })
    else if (path === '/api/v1/runtime/branding') await route.fulfill({ json: { branding: null } })
    else if (path === '/api/v1/ai/providers') {
      await route.fulfill({
        json: {
          schema_version: 1,
          providers: drafts,
          runtime: {
            providers: drafts.map((draft) => ({
              id: draft.settings.provider,
              connected: true,
              capabilities_verified: false,
            })),
          },
          runtime_error: null,
        },
      })
    } else if (path === '/api/v1/ai/selection')
      await route.fulfill({ json: { configured: false, profile: null } })
    else if (path === '/api/v1/ai/budget') {
      await route.fulfill({
        json: {
          schema_version: 1,
          workspace: 'sdlc2',
          currency: 'USD',
          limit_microdollars: '30000000',
          settled_microdollars: '0',
          reserved_microdollars: '0',
          uncertain_microdollars: '0',
          available_microdollars: '30000000',
          unsettled_requests: 0,
          uncertain_requests: 0,
          blocked_reason: null,
        },
      })
    } else if (path.endsWith('/models')) {
      await route.fulfill({
        json: {
          provider: 'openrouter',
          access_verified: false,
          models: ['deepseek/deepseek-v4.1-flash', 'second-fixture'].map((id) => ({
            id,
            name: id,
            context_limit_tokens: 1000000,
            max_output_tokens: 1000,
          })),
        },
      })
    } else if (path === '/api/v1/ai/providers/openrouter' && route.request().method() === 'PUT') {
      const body = route.request().postDataJSON()
      saves.push({ body, revision: route.request().headers()['if-match'] })
      if (conflict)
        await route.fulfill({
          status: 412,
          json: { error: { code: 'stale_revision', message: 'fixture conflict' } },
        })
      else {
        drafts[1].settings = body
        drafts[1].draft_revision += 1
        drafts[1].model_contexts = drafts[1].model_contexts
          .filter((entry) => entry.model !== body.model)
          .concat({ model: body.model, context_window_tokens: body.context_window_tokens })
        await route.fulfill({ json: drafts[1] })
      }
    } else await route.fulfill({ status: 404, json: { error: { code: 'UNMOCKED' } } })
  })
  return {
    saves,
    conflict: () => {
      conflict = true
    },
  }
}

async function login(page: Page) {
  await page.goto('/ai?provider=openrouter')
  await page.getByRole('link', { name: 'Continue QA sign-in' }).click()
  await expect(page.getByLabel('Контекст, тыс. токенов')).toHaveValue('256')
  await page.getByRole('button', { name: /OpenRouter/ }).click()
  await expect(page.getByLabel('Модель')).toHaveValue('deepseek/deepseek-v4.1-flash')
}

test('AI route protects access, keeps activation closed and fits narrow viewports', async ({
  page,
}, info) => {
  test.setTimeout(120_000)
  await fixtures(page)
  await login(page)
  await expect(page.getByRole('button', { name: 'Проверить модель' })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Сделать активным' })).toBeDisabled()
  for (const width of [320, 375, 767, 768, 1279, 1280, 1440, 1920, 2560]) {
    await page.setViewportSize({ width, height: 1080 })
    await page.evaluate(() => window.scrollTo(0, 0))
    await page.screenshot({ path: info.outputPath(`ai-${width}.png`), fullPage: true })
    const overflow = await page.evaluate(() =>
      [...document.querySelectorAll('body *')]
        .filter(
          (element) =>
            element.getBoundingClientRect().right > window.innerWidth + 1 ||
            element.scrollWidth > element.clientWidth + 1,
        )
        .map(
          (element) =>
            `${element.tagName}.${element.className}: right=${element.getBoundingClientRect().right}, scroll=${element.scrollWidth}, client=${element.clientWidth}`,
        ),
    )
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
      JSON.stringify(
        await page.evaluate(() => ({
          viewport: innerWidth,
          document: document.documentElement.scrollWidth,
          body: document.body.scrollWidth,
        })),
      ) +
        '\n' +
        overflow.join('\n'),
    ).toBe(true)
  }
})

test('context uses token units and stale revision preserves the local draft', async ({ page }) => {
  const state = await fixtures(page)
  await login(page)
  await page.getByLabel('Контекст, тыс. токенов').fill('255')
  await expect(page.getByLabel('Контекст, тыс. токенов')).toHaveValue('255')
  await page.getByRole('button', { name: 'Сохранить черновик' }).click()
  await expect.poll(() => state.saves.length).toBe(1)
  expect(state.saves[0]).toEqual({
    body: {
      provider: 'openrouter',
      model: 'deepseek/deepseek-v4.1-flash',
      context_window_tokens: 255000,
    },
    revision: '"1"',
  })
  state.conflict()
  await page.getByLabel('Контекст, тыс. токенов').fill('200')
  await expect(page.getByLabel('Контекст, тыс. токенов')).toHaveValue('200')
  await page.getByRole('button', { name: 'Сохранить черновик' }).click()
  await expect(page.getByRole('alert')).toContainText('черновик сохранён')
  await expect(page.getByLabel('Контекст, тыс. токенов')).toHaveValue('200')
})

test('unsaved AI settings can stay on the page or leave explicitly', async ({ page }) => {
  await fixtures(page)
  await login(page)
  await page.getByLabel('Контекст, тыс. токенов').fill('200')
  await page.getByRole('link', { name: 'Обзор', exact: true }).click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('button', { name: 'Остаться', exact: true }).click()
  await expect(page.getByLabel('Контекст, тыс. токенов')).toHaveValue('200')
  await page.getByRole('link', { name: 'Обзор', exact: true }).click()
  await page.getByRole('button', { name: 'Уйти без сохранения' }).click()
  await expect(page).toHaveURL(/\/$/)
})
