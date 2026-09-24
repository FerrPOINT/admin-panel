import { generateKeyPairSync, sign } from 'node:crypto'
import { expect, test, type Page, type Route } from '@playwright/test'

// Admin Panel e2e smoke: every shell page renders with mocked admin API.
// Auth note: in production the admin API rejects anonymous mutations (401/403);
// the UI reads data through GET endpoints, which the mocks below fulfill.

const now = '2026-09-05T10:00:00Z'
const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' })
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'smoke-e2e', alg: 'ES256', use: 'sig' }

function idToken(nonce: string) {
  const timestamp = Math.floor(Date.now() / 1000)
  const header = Buffer.from(JSON.stringify({ alg: 'ES256', typ: 'JWT', kid: jwk.kid })).toString(
    'base64url',
  )
  const payload = Buffer.from(
    JSON.stringify({
      iss: 'http://localhost:7701',
      aud: 'admin-panel',
      sub: 'u-e2e',
      email: 'admin@base.local',
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

const brandingDocument = {
  product_name: 'Base Platform',
  product_short_name: 'Base',
  logo_url: null,
  favicon_url: null,
  support_url: null,
  primary_color: '#0f766e',
  accent_color: '#f59e0b',
  surface_color: null,
}

const revisions = [
  {
    id: '11111111-1111-7111-8111-111111111111',
    revision: 2,
    state: 'published',
    document: brandingDocument,
    document_hash: 'hash-2',
    etag: '"branding-r2-abc"',
    created_by_subject: 'admin@base.local',
    created_at: now,
    published_by_subject: 'admin@base.local',
    published_at: now,
    based_on_revision: 1,
  },
  {
    id: '22222222-2222-7222-8222-222222222222',
    revision: 1,
    state: 'superseded',
    document: brandingDocument,
    document_hash: 'hash-1',
    etag: '"branding-r1-abc"',
    created_by_subject: 'admin@base.local',
    created_at: now,
    published_by_subject: 'admin@base.local',
    published_at: now,
    based_on_revision: null,
  },
]

const services = [
  {
    id: '33333333-3333-7333-8333-333333333331',
    service_key: 'ci-cd',
    display_name: 'CI/CD',
    owner_team: 'platform',
    status: 'active',
    active_declaration_id: '33333333-3333-7333-8333-333333333399',
    created_at: now,
    updated_at: now,
    version: 3,
  },
  {
    id: '33333333-3333-7333-8333-333333333332',
    service_key: 'wiki',
    display_name: 'Wiki',
    owner_team: 'platform',
    status: 'active',
    active_declaration_id: null,
    created_at: now,
    updated_at: now,
    version: 1,
  },
]

const auditEvents = [
  {
    id: '44444444-4444-7444-8444-444444444441',
    occurred_at: now,
    request_id: 'req-1',
    actor_subject: 'admin@base.local',
    actor_role: 'platform_admin',
    action: 'branding.published',
    entity_type: 'branding_revision',
    entity_id: '11111111-1111-7111-8111-111111111111',
    metadata: { revision: 2 },
  },
]

function routeJson(route: Route, body: unknown, status = 200) {
  return route.fulfill({
    status,
    contentType: 'application/json',
    body: JSON.stringify(body),
  })
}

async function installApiMocks(
  page: Page,
  capabilities = { mutate: true, manage_bindings: false },
  revisionFixtures = revisions,
) {
  let nonce = ''
  await page.route('http://localhost:7701/oidc/**', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname === '/oidc/authorize') {
      nonce = url.searchParams.get('nonce') ?? ''
      const callback = new URL(url.searchParams.get('redirect_uri')!)
      callback.searchParams.set('code', 'smoke-e2e-code')
      callback.searchParams.set('state', url.searchParams.get('state') ?? '')
      await route.fulfill({ status: 302, headers: { location: callback.toString() } })
    } else if (url.pathname === '/oidc/token') {
      await route.fulfill({
        contentType: 'application/json',
        headers: { 'access-control-allow-origin': '*' },
        body: JSON.stringify({
          access_token: 'e2e-token',
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
  await page.route('**/api/v1/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const path = url.pathname.replace('/api/v1', '')
    const method = request.method()

    if (
      method === 'GET' &&
      (path === '/branding/revisions' || path.split('?')[0] === '/branding-revisions')
    ) {
      return routeJson(route, {
        revisions: revisionFixtures,
        total: revisionFixtures.length,
      })
    }
    if (method === 'GET' && path.split('?')[0] === '/services') {
      return routeJson(route, { services, total: services.length })
    }
    if (method === 'GET' && path === '/services/ci-cd') {
      return routeJson(route, {
        service: services[0],
        declarations: [
          {
            id: '33333333-3333-7333-8333-333333333399',
            registry_entry_id: services[0].id,
            declaration_version: 1,
            integration_base_url: 'http://localhost:7712',
            capabilities: ['health.read', 'branding.runtime.read'],
            service_contract_version: '1.0.0',
            declared_by_subject: 'bootstrap',
            declared_at: now,
            approval_status: 'approved',
            approved_by_subject: 'admin@base.local',
            approved_at: now,
            content_hash: 'decl-hash',
          },
        ],
      })
    }
    if (method === 'GET' && path === '/services/ci-cd/checks') {
      return routeJson(route, {
        checks: [
          {
            id: '55555555-5555-7555-8555-555555555551',
            registry_entry_id: services[0].id,
            declaration_id: '33333333-3333-7333-8333-333333333399',
            capability_key: 'health.read',
            triggered_by_subject: 'admin@base.local',
            started_at: now,
            finished_at: now,
            outcome: 'success',
            http_status: 200,
            summary: 'HTTP 200',
            request_id: 'req-check-1',
          },
        ],
        total: 1,
      })
    }
    if (method === 'POST' && path === '/services/ci-cd/checks') {
      const capability = request.postDataJSON().capability
      return routeJson(
        route,
        {
          check_run: {
            id: '55555555-5555-7555-8555-555555555552',
            service_key: 'ci-cd',
            capability,
            outcome: 'success',
            http_status: 200,
            summary: 'HTTP 200',
          },
        },
        202,
      )
    }
    if (method === 'GET' && path === '/auth/me') {
      const auth = request.headers()['authorization'] ?? ''
      if (auth !== 'Bearer e2e-token') {
        return routeJson(route, { error: { code: 'UNAUTHORIZED', message: 'missing bearer' } }, 401)
      }
      return routeJson(route, {
        subject: 'u-e2e',
        email: 'admin@base.local',
        central_role: 'member',
        panel_role: 'platform_admin',
        capabilities,
      })
    }
    if (method === 'GET' && path === '/users') {
      return routeJson(route, [
        {
          id: 'u-e2e',
          email: 'admin@base.local',
          username: 'admin',
          display_name: 'Admin',
          status: 'active',
          setup_delivery_status: 'sent',
        },
      ])
    }
    if (method === 'GET' && path === '/tokens') {
      return routeJson(route, [])
    }
    if (method === 'GET' && path === '/token-services') {
      return routeJson(route, [
        {
          key: 'admin-panel',
          label: 'Admin Panel',
          scopes: ['admin-panel:read', 'admin-panel:write'],
        },
      ])
    }
    if (method === 'GET' && path === '/health/ready') {
      return routeJson(route, { status: 'ok', database: 'up' })
    }
    if (method === 'GET' && path.split('?')[0] === '/audit-events') {
      return routeJson(route, { events: auditEvents, total: auditEvents.length })
    }
    if (method === 'GET' && path === '/runtime/branding') {
      return routeJson(route, { revision: 2, updated_at: now, branding: brandingDocument })
    }
    if (method === 'GET' && path === '/runtime/services') {
      return routeJson(route, {
        services: [
          {
            key: 'ci-cd',
            label: 'CI/CD',
            url: 'http://localhost:7712',
            ui_url: 'http://localhost:7712',
            health: 'healthy',
            capabilities: ['health.read'],
            contract_version: '1.0.0',
          },
          {
            key: 'wiki',
            label: 'Wiki',
            url: 'http://localhost:7732',
            ui_url: 'http://localhost:7732',
            health: 'healthy',
            capabilities: ['health.read'],
            contract_version: '1.0.0',
          },
        ],
      })
    }
    return routeJson(
      route,
      { error: { code: 'NOT_FOUND', message: `unmocked ${method} ${path}` } },
      404,
    )
  })
}

test.beforeEach(async ({ page }) => {
  await installApiMocks(page)
})

test('overview renders platform summary', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: /обзор/i })).toBeVisible()
})

test('branding page shows published document fields', async ({ page }) => {
  await page.goto('/branding')
  await expect(page.getByText('Base Platform').first()).toBeVisible()
  await expect(page.getByLabel('Основной цвет: HEX')).toHaveValue('#0f766e')
})

test('revisions page lists revision 2 published', async ({ page }) => {
  await page.goto('/revisions')
  await expect(page.getByText(/2/).first()).toBeVisible()
  await expect(page.getByText(/published|опубликован/i).first()).toBeVisible()
})

test('services catalog lists fleet entries', async ({ page }) => {
  await page.goto('/services')
  await expect(page.getByText('CI/CD').first()).toBeVisible()
  await expect(page.getByText('Wiki').first()).toBeVisible()
})

test('service detail shows approved declaration', async ({ page }) => {
  await page.goto('/services/ci-cd')
  await expect(page.getByText('http://localhost:7712').first()).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Проверки интеграции' })).toBeVisible()
  await expect(page.getByText('admin@base.local').first()).toBeVisible()
})

test('service detail runs only the selected bounded capability check', async ({ page }) => {
  await page.goto('/services/ci-cd')
  await page.getByLabel('Возможность').selectOption('branding.runtime.read')
  const request = page.waitForRequest(
    (candidate) =>
      candidate.method() === 'POST' &&
      new URL(candidate.url()).pathname === '/api/v1/services/ci-cd/checks',
  )
  await page.getByRole('button', { name: 'Запустить проверку' }).click()
  expect((await request).postDataJSON()).toEqual({ capability: 'branding.runtime.read' })
  await expect(page.getByText('Проверка завершена успешно')).toBeVisible()
})

test('audit page lists branding.published event', async ({ page }) => {
  await page.goto('/audit')
  await page.locator('details summary').first().click()
  await expect(page.getByText('branding.published').first()).toBeVisible()
  await expect(page.getByText('admin@base.local').first()).toBeVisible()
  await expect(page.locator('details').first()).toContainText('Брендинг')
})

test('runtime page probes branding endpoint status and etag', async ({ page }) => {
  await page.goto('/runtime')
  await expect(page.getByText(/200 OK/).first()).toBeVisible()
  await expect(page.getByText('branding').first()).toBeVisible()
})

test('settings page renders', async ({ page }) => {
  await page.goto('/settings')
  await expect(page.getByRole('heading', { name: /настройк/i })).toBeVisible()
  await expect(page.getByText('Роль в панели')).toHaveCount(0)
})

test('service switcher links to other products', async ({ page }) => {
  await page.goto('/')
  // The switcher renders either as a click-menu (catalog v1.1) or as a
  // hover/focus dropdown with plain links (v1.0 fallback). Both must expose
  // navigation to the CI/CD product.
  const ciLink = page.locator('a[href="http://localhost:7712"]').first()
  const attached = (await ciLink.isVisible().catch(() => false)) || (await ciLink.count()) > 0
  if (!attached) {
    // v1.1 menu: open via the named or icon-only trigger button
    const switcher = page.getByRole('button', { name: /Открыть список сервисов/ }).first()
    await switcher.click()
    await expect(page.getByRole('menuitem', { name: /ci/i }).first()).toBeVisible()
    await page.keyboard.press('Escape')
  } else {
    await expect(ciLink.first()).toBeAttached()
  }
})

test('protected route completes OIDC and keeps tokens out of URL', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: /обзор/i })).toBeVisible()
  expect(page.url()).not.toContain('e2e-token')
  expect(page.url()).not.toContain('smoke-e2e-code')
})

test('users page lists centrally managed accounts', async ({ page }) => {
  await page.goto('/users')
  await expect(page.getByRole('heading', { name: 'Пользователи' })).toBeVisible()
  await expect(page.getByText('admin@base.local').first()).toBeVisible()
})

test('read-only capabilities keep data visible without exposing mutation commands', async ({
  page,
}) => {
  const writes: string[] = []
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.pathname.startsWith('/api/v1/') && request.method() !== 'GET') {
      writes.push(`${request.method()} ${url.pathname}`)
    }
  })
  const readOnlyRevisions = [
    {
      ...revisions[0],
      id: '22222222-2222-7222-8222-222222222223',
      revision: 3,
      state: 'draft',
      published_by_subject: null,
      published_at: null,
      based_on_revision: 2,
    },
    ...revisions,
  ]
  await page.unrouteAll()
  await installApiMocks(page, { mutate: false, manage_bindings: false }, readOnlyRevisions)

  const identityResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith('/api/v1/auth/me'),
  )
  await page.goto('/branding')
  await expect((await identityResponse).json()).resolves.toMatchObject({
    capabilities: { mutate: false, manage_bindings: false },
  })
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByLabel('Название платформы')).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Опубликовать' })).toHaveCount(0)

  await page.goto('/revisions')
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Опубликовать' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Отозвать' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Сравнить' }).first()).toBeVisible()

  await page.goto('/services')
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Добавить сервис' })).toHaveCount(0)

  await page.goto('/services/ci-cd')
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Запустить проверку' })).toHaveCount(0)
  await expect(page.getByRole('heading', { name: 'Новая декларация' })).toHaveCount(0)

  await page.goto('/users')
  await expect(page.getByText('admin@base.local').first()).toBeVisible()
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Добавить' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: /Действия с пользователем/ })).toHaveCount(0)

  await page.goto('/tokens')
  await expect(page.getByText('Токенов пока нет.')).toBeVisible()
  await expect(page.getByText('Только чтение').first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Создать' })).toHaveCount(0)
  expect(writes).toEqual([])
})

test('branding and primary management controls remain touch-sized on mobile and tablet', async ({
  page,
}) => {
  for (const width of [375, 768]) {
    await page.setViewportSize({ width, height: width === 375 ? 812 : 1024 })
    for (const [path, heading] of [
      ['/branding', 'Брендинг платформы'],
      ['/users', 'Пользователи'],
      ['/tokens', 'Личные API-токены'],
    ] as const) {
      await page.goto(path)
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible()
      const smallTargets = await page
        .locator('main button, main input:not([type="checkbox"]), main select, main textarea')
        .evaluateAll((elements) =>
          elements
            .filter((element) => {
              const rect = element.getBoundingClientRect()
              return rect.width > 0 && rect.height > 0
            })
            .map((element) => {
              const rect = element.getBoundingClientRect()
              return {
                label:
                  element.getAttribute('aria-label') ??
                  ('labels' in element
                    ? [...(element.labels ?? [])]
                        .map((label) => label.textContent?.trim())
                        .join(' ')
                    : element.textContent?.trim()),
                width: Math.round(rect.width * 10) / 10,
                height: Math.round(rect.height * 10) / 10,
              }
            })
            .filter((target) => target.width < 40 || target.height < 40),
        )
      expect(smallTargets, `${path} at ${width}px`).toEqual([])
    }
  }
})

test('protected routes initiate Central Auth', async ({ page }) => {
  const authorize = page.waitForRequest((request) =>
    request.url().startsWith('http://localhost:7701/oidc/authorize'),
  )
  await page.goto('/services')
  const url = new URL((await authorize).url())
  expect(url.searchParams.get('client_id')).toBe('admin-panel')
  expect(url.searchParams.get('code_challenge_method')).toBe('S256')
  await expect(page.getByRole('heading', { name: 'Каталог сервисов' })).toBeVisible()
})
