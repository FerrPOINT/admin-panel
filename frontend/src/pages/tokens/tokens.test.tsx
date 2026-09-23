import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { TokensPage } from './index'

afterEach(() => vi.unstubAllGlobals())

describe('TokensPage', () => {
  it('offers Service Pulse read and write scopes', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify([]), { status: 200 })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><TokensPage /></QueryClientProvider>)

    await screen.findByText('Токенов пока нет.')
    fireEvent.click(screen.getByRole('button', { name: 'Создать' }))

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
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify(tokens), { status: 200 })))
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><TokensPage /></QueryClientProvider>)

    await screen.findByText('Токен 1')
    expect(screen.queryByText('Токен 11')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    expect(screen.getByText('Токен 11')).toBeInTheDocument()
    fireEvent.change(screen.getByRole('combobox', { name: 'Статус токена' }), { target: { value: 'expired' } })
    expect(screen.getByText('Токен 12')).toBeInTheDocument()
    expect(screen.queryByText('Токен 11')).not.toBeInTheDocument()
  })
})
