import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { ai, type Draft } from '@/shared/api/ai'
import { AiPage } from './ai'

const workspace = vi.hoisted(() => ({ setDirty: vi.fn() }))
vi.mock('@/shared/ai-ui/workspace', () => ({ useAiWorkspace: () => workspace }))
vi.mock('@/shared/api/ai', async (original) => {
  const module = await original<typeof import('@/shared/api/ai')>()
  return {
    ...module,
    ai: { registry: vi.fn(), selection: vi.fn(), models: vi.fn(), save: vi.fn(), budget: vi.fn() },
  }
})
afterEach(cleanup)
beforeEach(() => {
  vi.clearAllMocks()
  const draft: Draft = {
    settings: { provider: 'openrouter', model: 'one', context_window_tokens: 128000 },
    draft_revision: 4,
    updated_at: '',
    model_contexts: [
      { model: 'one', context_window_tokens: 128000 },
      { model: 'two', context_window_tokens: 192000 },
    ],
  }
  vi.mocked(ai.registry).mockResolvedValue({
    schema_version: 1,
    providers: [draft],
    runtime_error: null,
    runtime: { providers: [{ id: 'openrouter', connected: true, capabilities_verified: false }] },
  })
  vi.mocked(ai.selection).mockResolvedValue({ configured: false, profile: null })
  vi.mocked(ai.budget).mockResolvedValue({
    schema_version: 1,
    workspace: 'sdlc2',
    currency: 'USD',
    limit_microdollars: '30000000',
    settled_microdollars: '100',
    reserved_microdollars: '1500',
    uncertain_microdollars: '1500',
    available_microdollars: '29998400',
    unsettled_requests: 1,
    uncertain_requests: 1,
    blocked_reason: null,
  })
  vi.mocked(ai.models).mockResolvedValue({
    provider: 'openrouter',
    access_verified: false,
    models: ['one', 'two', 'new'].map((id) => ({
      id,
      name: id,
      context_limit_tokens: 1000000,
      max_output_tokens: 1000,
    })),
  })
  vi.mocked(ai.save).mockImplementation(async (settings) => ({
    ...draft,
    settings,
    draft_revision: 5,
    model_contexts: [
      ...draft.model_contexts!.filter((entry) => entry.model !== settings.model),
      { model: settings.model, context_window_tokens: settings.context_window_tokens },
    ],
  }))
})

it('shows held unknown cost and a provider overrun blocks paid requests', async () => {
  const budget = await ai.budget()
  vi.mocked(ai.budget).mockResolvedValue({
    ...budget,
    blocked_reason: 'provider_cost_exceeds_reservation',
  })
  render(
    <MemoryRouter>
      <AiPage />
    </MemoryRouter>,
  )
  expect(await screen.findByText('$29.9984')).toBeInTheDocument()
  expect(screen.getByText(/Резерв \$0.0015 удерживается/)).toBeInTheDocument()
  expect(screen.getByRole('alert')).toHaveTextContent('Новые платные запросы заблокированы')
  expect(screen.getByText(/Квота подписки ChatGPT учитывается отдельно/)).toBeInTheDocument()
})

it('does not replace unavailable budget with zero or block the saved draft', async () => {
  vi.mocked(ai.budget).mockRejectedValue(new Error('fixture runtime unavailable'))
  render(
    <MemoryRouter initialEntries={['/ai?provider=openrouter']}>
      <AiPage />
    </MemoryRouter>,
  )
  expect(await screen.findByText('Бюджет недоступен. Остаток не подтверждён.')).toBeInTheDocument()
  expect(await screen.findByDisplayValue('128')).toBeInTheDocument()
  expect(screen.queryByText('$30.00')).not.toBeInTheDocument()
})

it('refreshes budget readback without discarding an unsaved context', async () => {
  render(
    <MemoryRouter initialEntries={['/ai?provider=openrouter']}>
      <AiPage />
    </MemoryRouter>,
  )
  const input = await screen.findByDisplayValue('128')
  fireEvent.change(input, { target: { value: '255' } })
  const current = await ai.budget()
  vi.mocked(ai.budget).mockResolvedValue({
    ...current,
    available_microdollars: '29000000',
    reserved_microdollars: '999900',
  })
  fireEvent.click(screen.getByRole('button', { name: /^Обновить бюджет$/ }))
  expect(await screen.findByText('$29.00')).toBeInTheDocument()
  expect(input).toHaveValue(255)
})

it('restores model budgets, keeps invalid edits and saves only the selected model budget', async () => {
  render(
    <MemoryRouter initialEntries={['/ai?provider=openrouter']}>
      <AiPage />
    </MemoryRouter>,
  )
  const context = await screen.findByLabelText('Контекст, тыс. токенов')
  expect(context).toHaveValue(128)
  fireEvent.click(screen.getByRole('button', { name: 'Обновить каталог' }))
  const model = screen.getByLabelText('Модель')
  await waitFor(() => expect(model).toBeEnabled())
  fireEvent.change(context, { target: { value: '' } })
  fireEvent.change(model, { target: { value: 'two' } })
  expect(context).toHaveValue(192)
  fireEvent.change(context, { target: { value: '200' } })
  fireEvent.change(model, { target: { value: 'one' } })
  expect(context).toHaveValue(null)
  expect(screen.getByRole('button', { name: 'Сохранить черновик' })).toBeDisabled()
  fireEvent.change(model, { target: { value: 'new' } })
  expect(context).toHaveValue(256)
  fireEvent.change(model, { target: { value: 'two' } })
  expect(context).toHaveValue(200)
  fireEvent.click(screen.getByRole('button', { name: 'Сохранить черновик' }))
  await screen.findByRole('status')
  expect(ai.save).toHaveBeenCalledWith(
    { provider: 'openrouter', model: 'two', context_window_tokens: 200000 },
    4,
  )
  expect(context).toHaveValue(200)
  fireEvent.change(model, { target: { value: 'one' } })
  expect(context).toHaveValue(null)
  expect(workspace.setDirty).toHaveBeenLastCalledWith(true)
})

it('keeps the chosen model budget when refreshing a concurrently changed server draft', async () => {
  const initial = await ai.registry()
  vi.mocked(ai.registry)
    .mockResolvedValueOnce(initial)
    .mockResolvedValueOnce({
      ...initial,
      providers: initial.providers.map((draft) => ({
        ...draft,
        draft_revision: 8,
        model_contexts: [
          { model: 'one', context_window_tokens: 128000 },
          { model: 'two', context_window_tokens: 320000 },
        ],
      })),
    })
  render(
    <MemoryRouter initialEntries={['/ai?provider=openrouter']}>
      <AiPage />
    </MemoryRouter>,
  )
  const context = await screen.findByLabelText('Контекст, тыс. токенов')
  fireEvent.click(screen.getByRole('button', { name: 'Обновить каталог' }))
  const model = screen.getByLabelText('Модель')
  await waitFor(() => expect(model).toBeEnabled())
  fireEvent.change(model, { target: { value: 'two' } })
  expect(context).toHaveValue(192)
  fireEvent.click(screen.getByRole('button', { name: 'Обновить ревизию, сохранив черновик' }))
  await waitFor(() => expect(ai.registry).toHaveBeenCalledTimes(3))
  expect(context).toHaveValue(192)
  fireEvent.click(screen.getByRole('button', { name: 'Сохранить черновик' }))
  await screen.findByRole('status')
  expect(ai.save).toHaveBeenCalledWith(
    { provider: 'openrouter', model: 'two', context_window_tokens: 192000 },
    8,
  )
})
