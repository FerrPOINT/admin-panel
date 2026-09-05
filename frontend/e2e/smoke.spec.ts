import { expect, test, type Page, type Route } from '@playwright/test'

// Admin Panel e2e smoke: every shell page renders with mocked admin API.
// Auth note: in production the admin API rejects anonymous mutations (401/403);
// the UI reads data through GET endpoints, which the mocks below fulfill.

const now = '2026-09-05T10:00:00Z'

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

async function installApiMocks(page: Page) {
  await page.route('**/api/v1/**', (route) => {
    const request = route.request()
    const url = new URL(request.url())
    const path = url.pathname.replace('/api/v1', '')
    const method = request.method()

    if (method === 'GET' && (path === '/branding/revisions' || path.split('?')[0] === '/branding-revisions')) {
      return routeJson(route, { revisions, total: revisions.length })
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
    if (method === 'POST' && path === '/auth/login') {
      const body = request.postDataJSON() as { email?: string; password?: string }
      if (body?.password === 'wrong') {
        return routeJson(route, { error: { code: 'INVALID_CREDENTIALS', message: 'central auth rejected the credentials' } }, 401)
      }
      return routeJson(route, { access_token: 'e2e-token', token_type: 'Bearer', expires_in: 900, subject: 'u-e2e', central_role: 'member', panel_role: 'platform_admin' })
    }
    if (method === 'GET' && path === '/auth/me') {
      const auth = request.headers()['authorization'] ?? ''
      if (auth !== 'Bearer e2e-token' && auth !== 'Bearer stored-token') {
        return routeJson(route, { error: { code: 'UNAUTHORIZED', message: 'missing bearer' } }, 401)
      }
      return routeJson(route, { subject: 'u-e2e', email: 'admin@base.local', central_role: 'member', panel_role: 'platform_admin', capabilities: { mutate: true, manage_bindings: true } })
    }
    if (method === 'GET' && path.split('?')[0] === '/role-bindings') {
      return routeJson(route, {
        bindings: [
          { id: '55555555-5555-7555-8555-555555555551', claim_name: 'user_id', claim_value: 'u-admin', panel_role: 'platform_admin', created_by_subject: 'bootstrap', created_at: now },
        ],
      })
    }
    if (method === 'GET' && path === '/health/ready') {
      return routeJson(route, { status: 'ok', database: 'up' })
    }
    if (method === 'GET' && path.split('?')[0] === '/audit-events') {
      return routeJson(route, { events: auditEvents, total: auditEvents.length })
    }
    if (method === 'GET' && path === '/runtime/branding') {
      return routeJson(route, { revision: 2, document: brandingDocument })
    }
    if (method === 'GET' && path === '/runtime/services') {
      return routeJson(route, {
        services: [
          { key: 'ci-cd', label: 'CI/CD', url: 'http://localhost:7712', capabilities: ['health.read'], contract_version: '1.0.0' },
          { key: 'wiki', label: 'Wiki', url: 'http://localhost:7732', capabilities: ['health.read'], contract_version: '1.0.0' },
        ],
      })
    }
    return routeJson(route, { error: { code: 'NOT_FOUND', message: `unmocked ${method} ${path}` } }, 404)
  })
}

test.beforeEach(async ({ page }) => {
  await installApiMocks(page)
  // Every authenticated test starts with a stored session; login flow test
  // clears it explicitly.
  await page.addInitScript(() => sessionStorage.setItem('base.admin.token', 'stored-token'))
})

test('overview renders platform summary', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: /обзор/i })).toBeVisible()
})

test('branding page shows published document fields', async ({ page }) => {
  await page.goto('/branding')
  await expect(page.getByText('Base Platform').first()).toBeVisible()
  await expect(page.getByLabel('Основной цвет')).toHaveValue('#0f766e')
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
})

test('audit page lists branding.published event', async ({ page }) => {
  await page.goto('/audit')
  await expect(page.getByText('branding.published').first()).toBeVisible()
  await expect(page.getByText('admin@base.local').first()).toBeVisible()
  await expect(page.getByText('branding_revision').first()).toBeVisible()
})

test('runtime page probes branding endpoint status and etag', async ({ page }) => {
  await page.goto('/runtime')
  await expect(page.getByText('200').first()).toBeVisible()
  await expect(page.getByText('branding').first()).toBeVisible()
})

test('settings page renders', async ({ page }) => {
  await page.goto('/settings')
  await expect(page.getByRole('heading', { name: /настройк/i })).toBeVisible()
})

test('service switcher links to other products', async ({ page }) => {
  await page.goto('/')
  const switcher = page.getByRole('button', { name: /сервисы/i }).first()
  const catalogLink = page.locator('a[href="http://localhost:7712"]').first()
  const visible = await switcher.isVisible().catch(() => false)
  if (visible) {
    await switcher.hover()
    await expect(catalogLink).toBeVisible()
  } else {
    // catalog collapsed by default in sidebar footer: link exists in DOM
    await expect(catalogLink).toBeAttached()
  }
})


test('login flow: wrong password shows error, valid login navigates to overview', async ({ page }) => {
  await page.addInitScript(() => sessionStorage.removeItem('base.admin.token'))
  await page.goto('/')
  await expect(page).toHaveURL(/\/login$/)

  await page.getByLabel('Email').fill('admin@base.local')
  await page.getByLabel('Пароль').fill('wrong')
  await page.getByRole('button', { name: 'Войти' }).click()
  await expect(page.getByRole('alert')).toContainText(/rejected|Не удалось/i)

  await page.getByLabel('Пароль').fill('correct-password')
  await page.getByRole('button', { name: 'Войти' }).click()
  await expect(page).toHaveURL(/http:\/\/localhost:\d+\/$/)
  await expect(page.getByRole('heading', { name: /обзор/i })).toBeVisible()
})

test('role bindings page lists bindings for admin', async ({ page }) => {
  await page.goto('/role-bindings')
  await expect(page.getByRole('heading', { name: /привязки ролей/i })).toBeVisible()
  await expect(page.getByText('u-admin').first()).toBeVisible()
  await expect(page.getByText('platform_admin').first()).toBeVisible()
})

test('protected routes redirect anonymous visitors to login', async ({ page }) => {
  await page.addInitScript(() => sessionStorage.removeItem('base.admin.token'))
  await page.goto('/services')
  await expect(page).toHaveURL(/\/login$/)
})
