import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { UsersPage } from './index'
import { AuthProvider } from '@/shared/auth/auth-context'

afterEach(() => vi.unstubAllGlobals())

function renderPage() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={client}><AuthProvider><UsersPage /></AuthProvider></QueryClientProvider>)
}

function managedUser(index: number) {
  return {
    id: `u-${index}`,
    email: `user${index}@example.test`,
    username: `user${index}`,
    display_name: `Пользователь ${index}`,
    status: 'active',
    setup_delivery_status: 'sent',
  }
}

describe('UsersPage', () => {
  it('shows twenty users per page without losing the current server batch', async () => {
    const users = Array.from({ length: 25 }, (_, index) => ({
      id: `u-${index + 1}`,
      email: `user${index + 1}@example.test`,
      username: `user${index + 1}`,
      display_name: `Пользователь ${index + 1}`,
      status: 'active',
      setup_delivery_status: 'sent',
    }))
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(users), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)
    renderPage()

    await screen.findByText('Пользователь 1')
    expect(screen.queryByText('Пользователь 21')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    expect(screen.getByText('Пользователь 21')).toBeInTheDocument()
    expect(screen.queryByText('Пользователь 1')).not.toBeInTheDocument()
    expect(fetchMock.mock.calls.filter(([url]) => String(url).startsWith('/api/v1/users?'))).toHaveLength(1)
  })

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
    expect(await screen.findByText('Не удалось сохранить изменения. Проверьте данные и доставку письма.')).toBeInTheDocument()
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
    expect(screen.getByLabelText('Email')).toBeDisabled()
    expect(screen.getByLabelText('Имя')).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Повторить отправку' }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/users/u-1/password-link', expect.objectContaining({ method: 'POST' }),
    ))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(fetchMock.mock.calls.filter(([url, init]) => url === '/api/v1/users' && init?.method === 'POST')).toHaveLength(1)
  })

  it.each([100, 101])('does not navigate to a false empty page at a %i-user batch boundary', async (count) => {
    const allUsers = Array.from({ length: count }, (_, index) => managedUser(index + 1))
    const fetchMock = vi.fn((url: string) => {
      const offset = Number(new URL(url, 'http://localhost').searchParams.get('offset') ?? 0)
      return Promise.resolve(Response.json(allUsers.slice(offset, offset + 100)))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователь 1')
    for (const first of [21, 41, 61, 81]) {
      fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
      expect(await screen.findByText(`Пользователь ${first}`)).toBeInTheDocument()
    }
    await waitFor(() => expect(fetchMock.mock.calls.some(([url]) => String(url).includes('offset=100'))).toBe(true))
    if (count === 100) {
      await waitFor(() => expect(screen.getByRole('button', { name: 'Далее' })).toBeDisabled())
      expect(screen.queryByText('На этой странице нет пользователей')).not.toBeInTheDocument()
    } else {
      await waitFor(() => expect(screen.getByRole('button', { name: 'Далее' })).toBeEnabled())
      fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
      expect(await screen.findByText('Пользователь 101')).toBeInTheDocument()
      expect(screen.getByRole('button', { name: 'Далее' })).toBeDisabled()
    }
  })

  it('hides stale rows after a failed refetch and recovers without losing the page', async () => {
    let listRequests = 0
    const fetchMock = vi.fn((url: string) => {
      if (url.includes('/password-link')) return Promise.resolve(new Response(null, { status: 204 }))
      listRequests += 1
      if (listRequests === 2) return Promise.resolve(new Response(null, { status: 500 }))
      return Promise.resolve(Response.json([managedUser(1)]))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    expect(await screen.findByText('Пользователь 1')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Отправить ссылку: user1@example.test' }))
    expect(await screen.findByText(/Не удалось загрузить пользователей/)).toBeInTheDocument()
    expect(screen.queryByText('Пользователь 1')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Повторить' }))
    expect(await screen.findByText('Пользователь 1')).toBeInTheDocument()
  })

  it('locks the create draft and dialog until the request completes', async () => {
    let finishCreate: ((response: Response) => void) | undefined
    const fetchMock = vi.fn((url: string, init?: RequestInit) => {
      if (url === '/api/v1/users' && init?.method === 'POST') {
        return new Promise<Response>((resolve) => { finishCreate = resolve })
      }
      return Promise.resolve(Response.json([]))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователей пока нет')
    fireEvent.click(screen.getByRole('button', { name: 'Добавить' }))
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: 'new@example.test' } })
    fireEvent.change(screen.getByLabelText('Имя'), { target: { value: 'Новый пользователь' } })
    fireEvent.click(screen.getAllByRole('button', { name: 'Добавить' }).at(-1)!)
    await waitFor(() => expect(finishCreate).toBeDefined())
    expect(screen.getByLabelText('Email')).toBeDisabled()
    expect(screen.getByLabelText('Имя')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Сохраняем...' })).toBeDisabled()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await act(async () => finishCreate?.(Response.json({ ...managedUser(1), status: 'pending' }, { status: 201 })))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })

  it('retries a failed next-batch check before enabling navigation', async () => {
    const firstBatch = Array.from({ length: 100 }, (_, index) => managedUser(index + 1))
    let nextBatchRequests = 0
    const fetchMock = vi.fn((url: string) => {
      if (url.includes('offset=100')) {
        nextBatchRequests += 1
        return Promise.resolve(nextBatchRequests === 1
          ? new Response(null, { status: 500 })
          : Response.json([managedUser(101)]))
      }
      return Promise.resolve(Response.json(firstBatch))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователь 1')
    for (const first of [21, 41, 61, 81]) {
      fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
      expect(await screen.findByText(`Пользователь ${first}`)).toBeInTheDocument()
    }
    expect(await screen.findByText(/Не удалось проверить следующую страницу/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Далее' })).toBeDisabled()
    fireEvent.click(screen.getByRole('button', { name: 'Повторить' }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Далее' })).toBeEnabled())
    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    expect(await screen.findByText('Пользователь 101')).toBeInTheDocument()
  })

  it('keeps the status dialog open and its cancel action disabled while saving', async () => {
    let finishStatus: ((response: Response) => void) | undefined
    const fetchMock = vi.fn((url: string, init?: RequestInit) => {
      if (url.endsWith('/status') && init?.method === 'POST') {
        return new Promise<Response>((resolve) => { finishStatus = resolve })
      }
      return Promise.resolve(Response.json([managedUser(1)]))
    })
    vi.stubGlobal('fetch', fetchMock)
    renderPage()
    await screen.findByText('Пользователь 1')
    fireEvent.click(screen.getByRole('button', { name: 'Отключить: user1@example.test' }))
    fireEvent.click(screen.getByRole('button', { name: 'Подтвердить' }))
    await waitFor(() => expect(finishStatus).toBeDefined())
    expect(screen.getByRole('button', { name: 'Отмена' })).toBeDisabled()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    await act(async () => finishStatus?.(Response.json({ ...managedUser(1), status: 'disabled' })))
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  })
})
