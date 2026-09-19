import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { UsersPage } from './index'
import { AuthProvider } from '@/shared/auth/auth-context'

afterEach(() => vi.unstubAllGlobals())

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><AuthProvider><UsersPage /></AuthProvider></QueryClientProvider>)
}

describe('UsersPage', () => {
  it('keeps entered data when creating a user fails', async () => {
    const fetchMock = vi.fn().mockImplementation((_url: string, init?: RequestInit) =>
      Promise.resolve(init?.method === 'POST'
        ? new Response(JSON.stringify({ error: { code: 'SMTP_UNAVAILABLE', message: 'Mail unavailable' } }), { status: 503 })
        : new Response('[]', { status: 200 })))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователей пока нет')
    fireEvent.click(screen.getByRole('button', { name: 'Добавить' }))
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'new@example.test' } })
    fireEvent.change(screen.getByLabelText('Имя'), { target: { value: 'Новый пользователь' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Добавить' }).at(-1)!)
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/users', expect.objectContaining({ method: 'POST' })))
    expect(screen.getByLabelText('Email')).toHaveValue('new@example.test')
    expect(screen.getByLabelText('Имя')).toHaveValue('Новый пользователь')
    expect(screen.getByRole('dialog')).toBeInTheDocument()
  })

  it('retries failed delivery without creating a second account', async () => {
    const fetchMock = vi.fn().mockImplementation((url: string, init?: RequestInit) => {
      if (url === '/api/v1/users' && init?.method === 'POST') {
        return Promise.resolve(new Response(JSON.stringify({
          id: 'u-1', email: 'new@example.test', display_name: 'Новый',
          username: 'new', status: 'pending', setup_delivery_status: 'failed',
        }), { status: 202 }))
      }
      if (url === '/api/v1/users/u-1/password-link') return Promise.resolve(new Response(null, { status: 204 }))
      return Promise.resolve(new Response('[]', { status: 200 }))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователей пока нет')
    fireEvent.click(screen.getByRole('button', { name: 'Добавить' }))
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'new@example.test' } })
    fireEvent.change(screen.getByLabelText('Имя'), { target: { value: 'Новый' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Добавить' }).at(-1)!)
    await screen.findByText(/Учётка создана, письмо не доставлено/)
    fireEvent.click(screen.getByRole('button', { name: 'Повторить отправку' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/users/u-1/password-link', expect.objectContaining({ method: 'POST' }),
    ))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(fetchMock.mock.calls.filter(([url, init]) => url === '/api/v1/users' && init?.method === 'POST')).toHaveLength(1)
  })
})
