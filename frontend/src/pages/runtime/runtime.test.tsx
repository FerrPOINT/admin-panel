import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { RuntimePage } from './index'

afterEach(() => vi.unstubAllGlobals())

const branding = { revision: 1, updated_at: '2026-09-19T00:00:00Z', branding: { product_name: 'SDLC', product_short_name: 'SDLC', primary_color: '#2563eb', accent_color: '#14b8a6' } }
const wiki = { key: 'wiki', label: 'Wiki', url: 'http://wiki-api.test', ui_url: 'http://wiki-ui.test', health: 'healthy', contract_version: '1.0.0', capabilities: ['ui.render'] }

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

  it('hides stale catalog rows on a failed refresh and restores a validated snapshot on 304', async () => {
    let catalogRequests = 0
    const fetch = vi.fn((url: string) => {
      if (url.endsWith('/branding')) return Promise.resolve(Response.json(branding))
      catalogRequests += 1
      if (catalogRequests === 1) return Promise.resolve(Response.json({ services: [wiki] }, { headers: { ETag: '"catalog-v1"' } }))
      if (catalogRequests === 2) return Promise.resolve(new Response(null, { status: 500 }))
      return Promise.resolve(new Response(null, { status: 304 }))
    })
    vi.stubGlobal('fetch', fetch)

    render(<RuntimePage />)
    expect(await screen.findByText('Wiki')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    expect(await screen.findByText(/Не удалось загрузить каталог: HTTP 500/)).toBeInTheDocument()
    expect(screen.queryByText('Wiki')).not.toBeInTheDocument()
    expect(screen.queryByText('Возможности каталога: 1')).not.toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    expect(await screen.findByText('Wiki')).toBeInTheDocument()
    expect(fetch).toHaveBeenLastCalledWith(expect.stringContaining('/services'), {
      cache: 'no-cache',
      headers: { 'If-None-Match': '"catalog-v1"' },
    })
  })

  it('does not cache an invalid response ETag after a valid catalog snapshot', async () => {
    let catalogRequests = 0
    const fetch = vi.fn((url: string) => {
      if (url.endsWith('/branding')) return Promise.resolve(Response.json(branding))
      catalogRequests += 1
      if (catalogRequests === 1) return Promise.resolve(Response.json({ services: [wiki] }, { headers: { ETag: '"catalog-v1"' } }))
      if (catalogRequests === 2) return Promise.resolve(Response.json({ services: {} }, { headers: { ETag: '"invalid-v2"' } }))
      return Promise.resolve(new Response(null, { status: 304 }))
    })
    vi.stubGlobal('fetch', fetch)

    render(<RuntimePage />)
    expect(await screen.findByText('Wiki')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    expect(await screen.findByText(/^Не удалось загрузить каталог: Некорректный ответ каталога$/)).toBeInTheDocument()
    expect(screen.queryByText('Wiki')).not.toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    expect(await screen.findByText('Wiki')).toBeInTheDocument()
    expect(fetch).toHaveBeenLastCalledWith(expect.stringContaining('/services'), {
      cache: 'no-cache',
      headers: { 'If-None-Match': '"catalog-v1"' },
    })
  })

  it('rejects 304 without a snapshot, but accepts 304 for a validated empty catalog', async () => {
    let catalogRequests = 0
    let brandingRequests = 0
    const fetch = vi.fn((url: string) => {
      if (url.endsWith('/branding')) {
        brandingRequests += 1
        return Promise.resolve(brandingRequests === 1
          ? new Response(null, { status: 304 })
          : Response.json(branding))
      }
      catalogRequests += 1
      if (catalogRequests === 1) return Promise.resolve(new Response(null, { status: 304 }))
      if (catalogRequests === 2) return Promise.resolve(Response.json({ services: [] }, { headers: { ETag: '"empty"' } }))
      return Promise.resolve(new Response(null, { status: 304 }))
    })
    vi.stubGlobal('fetch', fetch)

    render(<RuntimePage />)
    expect(await screen.findByText(/^Не удалось загрузить брендинг: 304 без сохранённого ответа брендинга$/)).toBeInTheDocument()
    expect(await screen.findByText(/^Не удалось загрузить каталог: 304 без сохранённого ответа каталога$/)).toBeInTheDocument()
    expect(screen.queryByText(/Каталог пуст/)).not.toBeInTheDocument()

    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[0])
    expect(await screen.findByText('Ревизия 1')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    expect(await screen.findByText(/Каталог пуст/)).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[1])
    await waitFor(() => expect(fetch).toHaveBeenLastCalledWith(expect.stringContaining('/services'), {
      cache: 'no-cache',
      headers: { 'If-None-Match': '"empty"' },
    }))
    expect(screen.getByText(/Каталог пуст/)).toBeInTheDocument()
  })

  it('does not cache an invalid branding ETag', async () => {
    let brandingRequests = 0
    const fetch = vi.fn((url: string) => {
      if (!url.endsWith('/branding')) return Promise.resolve(Response.json({ services: [] }))
      brandingRequests += 1
      if (brandingRequests === 1) return Promise.resolve(Response.json({ broken: true }, { headers: { ETag: '"invalid"' } }))
      return Promise.resolve(Response.json(branding, { headers: { ETag: '"branding-v1"' } }))
    })
    vi.stubGlobal('fetch', fetch)

    render(<RuntimePage />)
    expect(await screen.findByText(/^Не удалось загрузить брендинг: Некорректный ответ брендинга$/)).toBeInTheDocument()
    expect(await screen.findByText(/Каталог пуст/)).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[0])
    expect(await screen.findByText('Ревизия 1')).toBeInTheDocument()
    const brandingCalls = fetch.mock.calls.filter(([url]) => url.endsWith('/branding'))
    expect(brandingCalls[1]).toEqual([expect.stringContaining('/branding'), { cache: 'no-cache', headers: undefined }])
  })

  it('labels the previous branding response during refresh', async () => {
    let brandingRequests = 0
    let finishRefresh: ((response: Response) => void) | undefined
    vi.stubGlobal('fetch', vi.fn((url: string) => {
      if (!url.endsWith('/branding')) return Promise.resolve(Response.json({ services: [] }))
      brandingRequests += 1
      if (brandingRequests === 1) return Promise.resolve(Response.json(branding, { headers: { ETag: '"branding-v1"' } }))
      return new Promise<Response>((resolve) => { finishRefresh = resolve })
    }))

    render(<RuntimePage />)
    expect(await screen.findByText('Ревизия 1')).toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Обновить' })[0])
    expect(await screen.findByText('Обновление брендинга. Показан предыдущий ответ.')).toBeInTheDocument()
    expect(screen.getByText('Ревизия 1')).toBeInTheDocument()
    await act(async () => { finishRefresh?.(new Response(null, { status: 304 })) })
    expect(screen.queryByText('Обновление брендинга. Показан предыдущий ответ.')).not.toBeInTheDocument()
  })
})
