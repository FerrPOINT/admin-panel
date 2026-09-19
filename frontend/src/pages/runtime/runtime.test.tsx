import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { RuntimePage } from './index'

afterEach(() => vi.unstubAllGlobals())

describe('RuntimePage', () => {
  it('keeps service details collapsed until requested', async () => {
    vi.stubGlobal('fetch', vi.fn((url: string) => Promise.resolve(Response.json(url.endsWith('/branding')
      ? { revision: 1, updated_at: '2026-09-19T00:00:00Z', branding: { product_name: 'SDLC', product_short_name: 'SDLC', primary_color: '#2563eb', accent_color: '#14b8a6' } }
      : { services: [{ key: 'wiki', label: 'Wiki', url: 'http://wiki-api.test', ui_url: 'http://wiki-ui.test', health: 'healthy', contract_version: '1.0.0', capabilities: ['ui.render'] }] }))))

    render(<RuntimePage />)
    const service = await screen.findByText('Wiki')
    const details = service.closest('details')
    expect(details).not.toHaveAttribute('open')
    fireEvent.click(service)
    expect(details).toHaveAttribute('open')
    expect(screen.getByText('http://wiki-api.test')).toBeInTheDocument()
    expect(screen.getByText('Ревизия 1')).toBeInTheDocument()
  })

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
