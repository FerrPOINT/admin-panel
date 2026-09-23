import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { TokensPage } from './index'

afterEach(() => vi.unstubAllGlobals())

describe('TokensPage', () => {
  it('loads product scopes from Central Auth instead of a local list', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((input: RequestInfo | URL) => {
        const url = String(input)
        const body = url.endsWith('/api/v1/token-services')
          ? [
              {
                key: 'service-pulse',
                label: 'Service Pulse',
                scopes: ['service-pulse:read', 'service-pulse:write'],
              },
            ]
          : []
        return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }))
      }),
    )
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )

    await screen.findByText('Токенов пока нет.')
    const create = await screen.findByRole('button', { name: 'Создать' })
    expect(create).toBeEnabled()
    fireEvent.click(create)

    const servicePulse = screen.getByText('Service Pulse').parentElement
    expect(servicePulse).not.toBeNull()
    expect(servicePulse?.querySelectorAll('input[type="checkbox"]')).toHaveLength(2)
  })

  it('paginates tokens and filters by status', async () => {
    const tokens = Array.from({ length: 12 }, (_, index) => ({
      id: `t-${index + 1}`,
      label: `Токен ${index + 1}`,
      scopes: ['wiki:read'],
      expires_at: index === 11 ? '2020-01-01T00:00:00Z' : '2099-01-01T00:00:00Z',
      created_at: '2026-09-19T00:00:00Z',
      last_used_at: null,
      revoked_at: null,
    }))
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((input: RequestInfo | URL) => {
        const body = String(input).endsWith('/api/v1/token-services') ? [] : tokens
        return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }))
      }),
    )
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )

    await screen.findByText('Токен 1')
    expect(screen.queryByText('Токен 11')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    expect(screen.getByText('Токен 11')).toBeInTheDocument()
    fireEvent.change(screen.getByRole('combobox', { name: 'Статус токена' }), {
      target: { value: 'expired' },
    })
    expect(screen.getByText('Токен 12')).toBeInTheDocument()
    expect(screen.queryByText('Токен 11')).not.toBeInTheDocument()
  })

  it('blocks token creation and offers retry when the central catalog is unavailable', async () => {
    const fetch = vi.fn().mockImplementation((input: RequestInfo | URL) => {
      if (String(input).endsWith('/api/v1/token-services')) {
        return Promise.resolve(
          new Response(JSON.stringify({ error: { code: 'CENTRAL_AUTH_UNAVAILABLE' } }), {
            status: 502,
          }),
        )
      }
      return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }))
    })
    vi.stubGlobal('fetch', fetch)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )

    expect(await screen.findByRole('alert')).toHaveTextContent('Не удалось загрузить доступы')
    expect(screen.getByRole('button', { name: 'Создать' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Повторить' }))
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3))
  })

  it('keeps the token draft after a failed create request', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        if (url.endsWith('/api/v1/token-services')) {
          return Promise.resolve(
            new Response(
              JSON.stringify([{ key: 'wiki', label: 'Wiki', scopes: ['wiki:read', 'wiki:write'] }]),
              { status: 200 },
            ),
          )
        }
        if (url.endsWith('/api/v1/tokens') && init?.method === 'POST') {
          return Promise.resolve(
            new Response(JSON.stringify({ error: { code: 'CENTRAL_AUTH_UNAVAILABLE' } }), {
              status: 502,
            }),
          )
        }
        return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }))
      }),
    )
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )

    const create = await screen.findByRole('button', { name: 'Создать' })
    await waitFor(() => expect(create).toBeEnabled())
    fireEvent.click(create)
    const dialog = screen.getByRole('dialog')
    fireEvent.change(within(dialog).getByLabelText('Название'), { target: { value: 'Deploy CLI' } })
    fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Чтение' }))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Создать' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Черновик сохранён')
    expect(within(dialog).getByLabelText('Название')).toHaveValue('Deploy CLI')
    expect(within(dialog).getByRole('checkbox', { name: 'Чтение' })).toBeChecked()
  })
})
