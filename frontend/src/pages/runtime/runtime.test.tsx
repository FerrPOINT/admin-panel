import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { RuntimePage } from './index'

afterEach(() => vi.unstubAllGlobals())

describe('RuntimePage', () => {
  it('shows branding failure and rejects an invalid catalog without crashing', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => {
      if (url.endsWith('/branding')) return Promise.reject(new Error('Сеть недоступна'))
      return Promise.resolve({
        ok: true,
        status: 200,
        statusText: 'OK',
        headers: new Headers(),
        json: async () => ({ services: { key: 'broken' } }),
      })
    }))

    render(<RuntimePage />)
    expect(await screen.findByText(/Не удалось загрузить брендинг: Сеть недоступна/)).toBeInTheDocument()
    expect(await screen.findByText(/Не удалось загрузить каталог: Некорректный ответ каталога/)).toBeInTheDocument()
  })

  it('distinguishes absent branding from an invalid branding payload', async () => {
    const fetch = vi.fn((url: string) => Promise.resolve(url.endsWith('/branding')
      ? new Response(null, { status: 404 })
      : Response.json({ services: [] })))
    vi.stubGlobal('fetch', fetch)
    const { unmount } = render(<RuntimePage />)
    expect(await screen.findByText(/Нет опубликованного документа: приложения/)).toBeInTheDocument()
    unmount()

    fetch.mockImplementation((url: string) => Promise.resolve(url.endsWith('/branding')
      ? Response.json({ broken: true })
      : Response.json({ services: [] })))
    render(<RuntimePage />)
    expect(await screen.findByText(/Не удалось загрузить брендинг: Некорректный ответ брендинга/)).toBeInTheDocument()
  })
})
