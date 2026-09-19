import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '@/shared/api/client'
import { AuditPage } from './index'

vi.mock('@/shared/api/client', () => ({ api: { get: vi.fn() } }))

beforeEach(() => vi.mocked(api.get).mockReset())

function event(index: number, action = 'central_user.created', entityType = 'central_user') {
  return {
    id: `event-${index}`,
    occurred_at: '2026-09-19T08:00:00Z',
    request_id: `request-${index}`,
    actor_subject: `subject-${index}`,
    actor_role: 'platform_admin',
    action,
    entity_type: entityType,
    entity_id: `entity-${index}`,
    metadata: {},
  }
}

function mockEvents(events: ReturnType<typeof event>[]) {
  vi.mocked(api.get).mockImplementation(async (path) => {
    const url = new URL(path, 'http://test.local')
    const filtered = events.filter((item) =>
      (!url.searchParams.has('action') || item.action === url.searchParams.get('action')) &&
      (!url.searchParams.has('entity_type') || item.entity_type === url.searchParams.get('entity_type')),
    )
    const offset = Number(url.searchParams.get('offset') ?? 0)
    const limit = Number(url.searchParams.get('limit') ?? 20)
    return { events: filtered.slice(offset, offset + limit), total: filtered.length } as never
  })
}

function renderAudit() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(<QueryClientProvider client={queryClient}><AuditPage /></QueryClientProvider>)
}

describe('AuditPage', () => {
  it('shows a compact event row and reveals technical details on demand', async () => {
    vi.mocked(api.get).mockResolvedValue({
      total: 1,
      events: [{
        id: 'event-1',
        occurred_at: '2026-09-19T08:00:00Z',
        request_id: 'request-123',
        actor_subject: 'subject-123',
        actor_role: 'platform_admin',
        action: 'branding.published',
        entity_type: 'branding_revision',
        entity_id: 'revision-1',
        metadata: { revision: 3 },
      }],
    })
    const user = userEvent.setup()
    renderAudit()

    const action = await screen.findByText('Опубликован брендинг', { selector: 'span' })
    const details = action.closest('details')
    expect(details).not.toHaveAttribute('open')
    await user.click(action)
    expect(details).toHaveAttribute('open')
    expect(screen.getByText('request-123')).toBeInTheDocument()

    await user.selectOptions(screen.getByRole('combobox', { name: 'Тип сущности' }), 'service')
    await waitFor(() => expect(api.get).toHaveBeenCalledWith(expect.stringContaining('entity_type=service')))
  })

  it.each([20, 40])('does not offer an empty page after exactly %i events', async (count) => {
    mockEvents(Array.from({ length: count }, (_, index) => event(index)))
    const user = userEvent.setup()
    const view = renderAudit()
    await waitFor(() => expect(view.container.querySelectorAll('details')).toHaveLength(20))
    expect(screen.getByText(`1–20 из ${count}`)).toBeInTheDocument()

    const next = screen.getByRole('button', { name: 'Вперёд' })
    if (count === 20) {
      expect(next).toBeDisabled()
    } else {
      await user.click(next)
      await waitFor(() => expect(screen.getByText('21–40 из 40')).toBeInTheDocument())
      expect(view.container.querySelectorAll('details')).toHaveLength(20)
      expect(next).toBeDisabled()
    }
  })

  it('returns to the last valid page when the event count shrinks', async () => {
    const records = Array.from({ length: 40 }, (_, index) => event(index))
    vi.mocked(api.get).mockImplementation(async (path) => {
      const offset = Number(new URL(path, 'http://test.local').searchParams.get('offset') ?? 0)
      if (offset === 20) records.splice(20)
      return { events: records.slice(offset, offset + 20), total: records.length }
    })
    const user = userEvent.setup()
    const view = renderAudit()
    await screen.findByText('1–20 из 40')
    await user.click(screen.getByRole('button', { name: 'Вперёд' }))
    await waitFor(() => expect(view.container.querySelectorAll('details')).toHaveLength(20))
    await screen.findByText('1–20 из 20')
    expect(screen.getByRole('button', { name: 'Назад' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Вперёд' })).toBeDisabled()
  })

  it('offers user events and applies a custom exact action only on submit', async () => {
    mockEvents([
      ...Array.from({ length: 20 }, (_, index) => event(index)),
      event(20, 'branding.published', 'branding_revision'),
    ])
    const user = userEvent.setup()
    renderAudit()
    await screen.findByText('1–20 из 21')

    await user.selectOptions(screen.getByRole('combobox', { name: 'Тип сущности' }), 'central_user')
    await screen.findByText('1–20 из 20')
    expect(api.get).toHaveBeenCalledWith(expect.stringContaining('entity_type=central_user'))

    await user.selectOptions(screen.getByRole('combobox', { name: 'Действие' }), 'central_user.created')
    await waitFor(() => expect(api.get).toHaveBeenCalledWith(expect.stringContaining('action=central_user.created')))
    const callsBeforeModeChange = vi.mocked(api.get).mock.calls.length
    await user.selectOptions(screen.getByRole('combobox', { name: 'Действие' }), 'custom')
    expect(api.get).toHaveBeenCalledTimes(callsBeforeModeChange)
    const callsBeforeTyping = vi.mocked(api.get).mock.calls.length
    await user.type(screen.getByRole('textbox', { name: 'Точный код действия' }), 'branding.published')
    expect(api.get).toHaveBeenCalledTimes(callsBeforeTyping)
    await user.click(screen.getByRole('button', { name: 'Применить' }))
    await waitFor(() => expect(api.get).toHaveBeenCalledWith(expect.stringContaining('action=branding.published')))
    expect(screen.getByText('0–0 из 0')).toBeInTheDocument()
  })

  it('distinguishes a failed load from an empty result and allows retry', async () => {
    vi.mocked(api.get)
      .mockRejectedValueOnce(new Error('unavailable'))
      .mockResolvedValue({ events: [], total: 0 })
    const user = userEvent.setup()
    renderAudit()
    await screen.findByRole('alert')
    expect(screen.getByText('Число событий недоступно')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Повторить' }))
    await screen.findByText('Нет событий по выбранным фильтрам.')
    expect(screen.getByText('0–0 из 0')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Вперёд' })).toBeDisabled()
  })
})
