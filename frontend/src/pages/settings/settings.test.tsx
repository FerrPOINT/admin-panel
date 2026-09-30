import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { SettingsPage } from './index'

vi.mock('@/shared/auth/auth-context', () => ({
  useAuth: () => ({ session: { subject: 'qa-user' } }),
  authToken: () => null,
}))

afterEach(() => vi.unstubAllGlobals())

function renderSettings() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: 0 } } })
  render(
    <QueryClientProvider client={client}>
      <SettingsPage />
    </QueryClientProvider>,
  )
}

describe('SettingsPage readiness', () => {
  it('accepts an empty status-only 200 and requests the readiness endpoint', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetch)
    renderSettings()
    expect(await screen.findByText('Готова')).toBeInTheDocument()
    expect(fetch.mock.calls[0]?.[0]).toBe('/health/ready')
  })

  it('distinguishes 503 readiness failure from network unavailability and supports retry', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 503 }))
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
    vi.stubGlobal('fetch', fetch)
    renderSettings()
    expect(await screen.findByText('Не готова')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Проверить готовность' }))
    expect(await screen.findByText('Недоступна')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Проверить готовность' }))
    expect(await screen.findByText('Готова')).toBeInTheDocument()
  })

  it('rejects the HTML SPA fallback instead of reporting healthy', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response('<html>SPA</html>', {
          status: 200,
          headers: { 'Content-Type': 'text/html; charset=utf-8' },
        }),
      ),
    )
    renderSettings()
    expect(await screen.findByText('Недоступна')).toBeInTheDocument()
    expect(screen.queryByText('Готова')).not.toBeInTheDocument()
  })

  it('locks retry while checking and hides a previous success during revalidation', async () => {
    let resolve!: (response: Response) => void
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
      .mockImplementationOnce(
        () =>
          new Promise<Response>((done) => {
            resolve = done
          }),
      )
    vi.stubGlobal('fetch', fetch)
    renderSettings()
    expect(await screen.findByText('Готова')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Проверить готовность' }))
    expect(await screen.findByText('Проверка…')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Проверить готовность' })).toBeDisabled()
    expect(screen.queryByText('Готова')).not.toBeInTheDocument()
    resolve(new Response(null, { status: 503 }))
    expect(await screen.findByText('Не готова')).toBeInTheDocument()
  })
})
