import { afterEach, expect, it, vi } from 'vitest'

vi.mock('@/shared/auth/auth-context', () => ({ authToken: () => null }))

afterEach(() => { vi.unstubAllEnvs(); vi.unstubAllGlobals(); vi.resetModules() })

it('keeps API requests under the deployed section prefix', async () => {
  vi.resetModules()
  vi.stubEnv('BASE_URL', '/admin/')
  vi.stubEnv('VITE_API_BASE_URL', undefined)
  const fetchMock = vi.fn().mockResolvedValue(new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } }))
  vi.stubGlobal('fetch', fetchMock)
  const client = await import('./client')
  await client.api.get('/api/v1/health')
  const input = fetchMock.mock.calls[0]![0] as string | Request
  const path = typeof input === 'string' ? input : new URL(input.url).pathname
  expect(path).toBe('/admin/api/v1/health')
})
