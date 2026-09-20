import { act, fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useCreateService, useServices } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'
import { ServicesPage } from './index'

vi.mock('@/shared/api/hooks', () => ({
  useServices: vi.fn(),
  useCreateService: vi.fn(),
}))
vi.mock('@/shared/auth/auth-context', () => ({ useAuth: vi.fn() }))

const createMutate = vi.fn()
const createReset = vi.fn()
const refetch = vi.fn()
const entries = Array.from({ length: 25 }, (_, index) => ({
  id: `service-${index + 1}`,
  service_key: `service-${String(index + 1).padStart(2, '0')}`,
  display_name: `Сервис ${String(index + 1).padStart(2, '0')}`,
  owner_team: index >= 20 ? 'qa' : 'platform',
  status: index >= 20 ? 'disabled' : 'active',
  updated_at: '2026-09-20T10:00:00Z',
  health_status: 'healthy',
}))

function renderPage() {
  return render(
    <MemoryRouter>
      <ServicesPage />
    </MemoryRouter>,
  )
}

function mockCreate(overrides: Record<string, unknown> = {}) {
  vi.mocked(useCreateService).mockReturnValue({
    mutate: createMutate,
    reset: createReset,
    isPending: false,
    isError: false,
    error: null,
    ...overrides,
  } as unknown as ReturnType<typeof useCreateService>)
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(useAuth).mockReturnValue({ canMutate: true } as ReturnType<typeof useAuth>)
  vi.mocked(useServices).mockReturnValue({
    data: { services: entries, total: entries.length },
    isLoading: false,
    isError: false,
    refetch,
  } as unknown as ReturnType<typeof useServices>)
  mockCreate()
})

describe('ServicesPage', () => {
  it('paginates 25 services and resets the page when search or status changes', () => {
    renderPage()
    expect(screen.getByRole('status')).toHaveTextContent('Показано 20 из 25 сервисов')
    expect(screen.queryByText('Сервис 21')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Далее' }))
    expect(screen.getByText('Сервис 21')).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Показано 5 из 25 сервисов')

    fireEvent.change(screen.getByRole('searchbox', { name: 'Найти сервис' }), {
      target: { value: 'Сервис 01' },
    })
    expect(screen.getByText('Сервис 01')).toBeInTheDocument()
    expect(screen.queryByText('Сервис 21')).not.toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'Страницы каталога' })).not.toBeInTheDocument()

    fireEvent.change(screen.getByRole('searchbox', { name: 'Найти сервис' }), {
      target: { value: '' },
    })
    fireEvent.change(screen.getByRole('combobox', { name: 'Состояние сервиса' }), {
      target: { value: 'disabled' },
    })
    expect(screen.getByText('Сервис 21')).toBeInTheDocument()
    expect(screen.queryByText('Сервис 01')).not.toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Показано 5 из 5 сервисов')

    fireEvent.change(screen.getByRole('searchbox', { name: 'Найти сервис' }), {
      target: { value: 'отсутствует' },
    })
    expect(screen.getByText('По заданным условиям сервисы не найдены.')).toBeInTheDocument()
    expect(screen.queryByText('Каталог пуст.')).not.toBeInTheDocument()
  })

  it('hides stale rows after an error and offers retry', () => {
    vi.mocked(useServices).mockReturnValue({
      data: { services: entries, total: entries.length },
      isLoading: false,
      isError: true,
      refetch,
    } as unknown as ReturnType<typeof useServices>)
    renderPage()

    expect(screen.getByRole('alert')).toHaveTextContent('Не удалось загрузить сервисы')
    expect(screen.queryByText('Сервис 01')).not.toBeInTheDocument()
    expect(screen.queryByRole('searchbox')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Повторить' }))
    expect(refetch).toHaveBeenCalledOnce()
  })

  it('distinguishes an empty registry from an empty search result', () => {
    vi.mocked(useServices).mockReturnValue({
      data: { services: [], total: 0 },
      isLoading: false,
      isError: false,
      refetch,
    } as unknown as ReturnType<typeof useServices>)
    renderPage()

    expect(screen.getByText('Каталог пуст.')).toBeInTheDocument()
    expect(screen.queryByRole('searchbox')).not.toBeInTheDocument()
    expect(screen.queryByText('По заданным условиям сервисы не найдены.')).not.toBeInTheDocument()
  })

  it('locks a pending create and retains the draft after failure', () => {
    const view = renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Добавить сервис' }))
    fireEvent.change(screen.getByLabelText('Ключ сервиса'), { target: { value: 'new-service' } })
    fireEvent.change(screen.getByLabelText('Название'), { target: { value: 'Новый сервис' } })
    fireEvent.change(screen.getByLabelText('Команда-владелец'), { target: { value: 'platform' } })
    fireEvent.change(screen.getByLabelText('Базовый URL (HTTPS или localhost)'), {
      target: { value: 'http://localhost:7801' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Создать сервис' }))
    expect(createMutate).toHaveBeenCalledOnce()

    mockCreate({ isPending: true })
    view.rerender(
      <MemoryRouter>
        <ServicesPage />
      </MemoryRouter>,
    )
    expect(screen.getByLabelText('Ключ сервиса')).toBeDisabled()
    expect(screen.getByLabelText('health.read')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Отмена' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Закрыть форму' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Создаём...' })).toBeDisabled()

    mockCreate({ isError: true, error: new Error('Сервис уже существует') })
    view.rerender(
      <MemoryRouter>
        <ServicesPage />
      </MemoryRouter>,
    )
    expect(screen.getByRole('alert')).toHaveTextContent('Сервис уже существует')
    expect(screen.getByLabelText('Ключ сервиса')).toHaveValue('new-service')
    expect(screen.getByLabelText('Название')).toHaveValue('Новый сервис')

    const onSuccess = createMutate.mock.calls[0]![1].onSuccess as () => void
    act(() => onSuccess())
    expect(screen.queryByLabelText('Ключ сервиса')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Добавить сервис' }))
    expect(screen.getByLabelText('Ключ сервиса')).toHaveValue('')
  })

  it('does not expose mutations to a viewer', () => {
    vi.mocked(useAuth).mockReturnValue({ canMutate: false } as ReturnType<typeof useAuth>)
    renderPage()
    expect(screen.queryByRole('button', { name: 'Добавить сервис' })).not.toBeInTheDocument()
    expect(screen.getByText('Сервис 01').closest('a')).toHaveAttribute(
      'href',
      '/services/service-01',
    )
  })
})
