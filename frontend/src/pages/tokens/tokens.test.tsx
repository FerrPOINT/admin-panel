import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { TokensPage } from './index'
import { useAuth } from '@/shared/auth/auth-context'

vi.mock('@/shared/auth/auth-context', () => ({ useAuth: vi.fn(), authToken: () => null }))

afterEach(() => vi.unstubAllGlobals())
beforeEach(() => {
  vi.mocked(useAuth).mockReturnValue({ canMutate: true } as ReturnType<typeof useAuth>)
})

describe('TokensPage', () => {
  function renderRevocation(deleteRequest: () => Promise<Response>) {
    const revoked = new Set<string>()
    const tokens = [1, 2].map((index) => ({
      id: `t-${index}`,
      label: `Own QA ${index}`,
      scopes: ['wiki:read'],
      expires_at: '2099-01-01T00:00:00Z',
      created_at: '2026-09-19T00:00:00Z',
      last_used_at: null,
      revoked_at: null,
    }))
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        if (init?.method === 'DELETE') {
          const response = await deleteRequest()
          if (response.ok) revoked.add(url.split('/').at(-1)!)
          return response
        }
        if (url.endsWith('/api/v1/token-services')) return Response.json([])
        return Response.json(
          tokens.map((token) => ({
            ...token,
            revoked_at: revoked.has(token.id) ? '2026-10-04T00:00:00Z' : null,
          })),
        )
      }),
    )
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )
  }

  it('keeps confirmation visible and blocks Cancel/Escape while revocation is pending', async () => {
    let finish!: (response: Response) => void
    const deletion = vi.fn(() => new Promise<Response>((resolve) => (finish = resolve)))
    renderRevocation(deletion)
    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: 'Отозвать Own QA 1' }))
    const dialog = screen.getByRole('alertdialog')
    await user.click(within(dialog).getByRole('button', { name: 'Отозвать' }))
    expect(within(dialog).getByRole('button', { name: 'Отмена' })).toBeDisabled()
    expect(within(dialog).getByRole('button', { name: 'Отозвать' })).toBeDisabled()
    await user.keyboard('{Escape}')
    expect(dialog).toBeInTheDocument()
    expect(deletion).toHaveBeenCalledTimes(1)
    await act(async () => finish(new Response(null, { status: 204 })))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('restores focus after Cancel and clears an error before another confirmation', async () => {
    renderRevocation(() =>
      Promise.resolve(Response.json({ error: { code: 'NOT_FOUND' } }, { status: 404 })),
    )
    const user = userEvent.setup()
    const first = await screen.findByRole('button', { name: 'Отозвать Own QA 1' })
    await user.click(first)
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Отозвать',
      }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent('Не удалось отозвать токен')
    await user.click(screen.getByRole('button', { name: 'Отмена' }))
    await waitFor(() => expect(first).toHaveFocus())
    await user.click(screen.getByRole('button', { name: 'Отозвать Own QA 2' }))
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })

  it('returns focus to Create after an active token disappears on successful revocation', async () => {
    renderRevocation(() => Promise.resolve(new Response(null, { status: 204 })))
    const user = userEvent.setup()
    await screen.findByText('Own QA 1')
    fireEvent.change(screen.getByRole('combobox', { name: 'Статус токена' }), {
      target: { value: 'active' },
    })
    await user.click(screen.getByRole('button', { name: 'Отозвать Own QA 1' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Отозвать',
      }),
    )
    await waitFor(() => expect(screen.queryByText('Own QA 1')).not.toBeInTheDocument())
    await waitFor(() => expect(screen.getByRole('button', { name: 'Создать' })).toHaveFocus())
  })

  it('keeps token metadata readable without create or revoke controls', async () => {
    vi.mocked(useAuth).mockReturnValue({ canMutate: false } as ReturnType<typeof useAuth>)
    const token = {
      id: 't-1',
      label: 'Read-only CLI',
      scopes: ['wiki:read'],
      expires_at: '2099-01-01T00:00:00Z',
      created_at: '2026-09-19T00:00:00Z',
      last_used_at: null,
      revoked_at: null,
    }
    const fetchMock = vi.fn().mockResolvedValue(Response.json([token]))
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <TokensPage />
      </QueryClientProvider>,
    )

    expect(await screen.findByText('Read-only CLI')).toBeInTheDocument()
    expect(screen.getByText('Только чтение')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Создать' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Отозвать/ })).not.toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('/api/v1/tokens')
  })

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
