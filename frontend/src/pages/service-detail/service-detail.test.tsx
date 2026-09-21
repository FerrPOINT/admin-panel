import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { toast } from 'sonner'
import {
  useApproveService,
  useChangeServiceStatus,
  usePatchService,
  useRunServiceCheck,
  useService,
  useServiceChecks,
} from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'
import { ServiceDetailPage } from './index'

vi.mock('@/shared/api/hooks', () => ({
  useService: vi.fn(),
  useApproveService: vi.fn(),
  usePatchService: vi.fn(),
  useChangeServiceStatus: vi.fn(),
  useServiceChecks: vi.fn(),
  useRunServiceCheck: vi.fn(),
}))
vi.mock('@/shared/auth/auth-context', () => ({ useAuth: vi.fn() }))
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

const changeStatus = vi.fn()
const patchService = vi.fn()
const refetchChecks = vi.fn()
const runCheck = vi.fn()

beforeEach(() => {
  changeStatus.mockClear()
  patchService.mockClear()
  refetchChecks.mockClear()
  runCheck.mockClear()
  vi.mocked(toast.success).mockClear()
  vi.mocked(useAuth).mockReturnValue({ canMutate: true } as ReturnType<typeof useAuth>)
  vi.mocked(useService).mockReturnValue({
    isLoading: false,
    isError: false,
    data: {
      service: {
        id: 's-1',
        service_key: 'admin-panel',
        display_name: 'Admin Panel',
        owner_team: 'platform',
        status: 'active',
        version: 1,
        active_declaration_id: null,
      },
      declarations: [],
    },
  } as unknown as ReturnType<typeof useService>)
  vi.mocked(useApproveService).mockReturnValue({
    isPending: false,
    mutate: vi.fn(),
  } as unknown as ReturnType<typeof useApproveService>)
  vi.mocked(usePatchService).mockReturnValue({
    isPending: false,
    isError: false,
    mutate: patchService,
  } as unknown as ReturnType<typeof usePatchService>)
  vi.mocked(useChangeServiceStatus).mockReturnValue({
    isPending: false,
    mutate: changeStatus,
  } as unknown as ReturnType<typeof useChangeServiceStatus>)
  vi.mocked(useServiceChecks).mockReturnValue({
    isLoading: false,
    isFetching: false,
    isError: false,
    data: { checks: [], total: 0 },
    refetch: refetchChecks,
  } as unknown as ReturnType<typeof useServiceChecks>)
  vi.mocked(useRunServiceCheck).mockReturnValue({
    isPending: false,
    isError: false,
    mutate: runCheck,
  } as unknown as ReturnType<typeof useRunServiceCheck>)
})

describe('ServiceDetailPage', () => {
  it('requires confirmation before disabling a service', async () => {
    render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Отключить' }))
    expect(changeStatus).not.toHaveBeenCalled()
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('Отключить сервис?')).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Отключить' }))
    expect(changeStatus).toHaveBeenCalledWith({ action: 'disable', version: 1 }, expect.any(Object))
  })

  it('locks declaration fields during submission and preserves them after failure', () => {
    const view = render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )
    fireEvent.change(screen.getByLabelText('Базовый URL'), {
      target: { value: 'http://localhost:7801' },
    })
    expect(screen.getByLabelText('health.read')).toBeChecked()
    fireEvent.click(screen.getByRole('button', { name: 'Отправить декларацию' }))
    expect(patchService).toHaveBeenCalledWith(
      {
        version: 1,
        body: {
          declaration: {
            declaration_version: 1,
            integration_base_url: 'http://localhost:7801',
            public_ui_url: null,
            service_contract_version: '1.0.0',
            capabilities: ['health.read'],
          },
        },
      },
      expect.any(Object),
    )

    vi.mocked(usePatchService).mockReturnValue({
      isPending: true,
      isError: false,
      mutate: patchService,
    } as unknown as ReturnType<typeof usePatchService>)
    view.rerender(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )
    expect(screen.getByLabelText('Базовый URL')).toBeDisabled()
    expect(screen.getByLabelText('health.read')).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Отправляем...' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Отключить' })).toBeDisabled()

    vi.mocked(usePatchService).mockReturnValue({
      isPending: false,
      isError: true,
      error: new Error('Версия устарела'),
      mutate: patchService,
    } as unknown as ReturnType<typeof usePatchService>)
    view.rerender(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )
    expect(screen.getByRole('alert')).toHaveTextContent('Версия устарела')
    expect(screen.getByLabelText('Базовый URL')).toHaveValue('http://localhost:7801')
    expect(screen.getByLabelText('health.read')).toBeChecked()
    fireEvent.click(screen.getByRole('button', { name: 'Отправить декларацию' }))
    expect(patchService).toHaveBeenCalledTimes(2)
  })

  it('preserves the public UI route when creating a declaration from an active UI service', () => {
    vi.mocked(useService).mockReturnValue({
      isLoading: false,
      isError: false,
      data: {
        service: {
          id: 's-1',
          service_key: 'admin-panel',
          display_name: 'Admin Panel',
          owner_team: 'platform',
          status: 'active',
          version: 3,
          active_declaration_id: 'd-1',
        },
        declarations: [
          {
            id: 'd-1',
            declaration_version: 4,
            integration_base_url: 'http://admin-api:7771',
            public_ui_url: 'http://localhost:7772',
            service_contract_version: '1.0.0',
            capabilities: ['health.read', 'ui.render'],
            approval_status: 'approved',
          },
        ],
      },
    } as unknown as ReturnType<typeof useService>)
    const view = render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByLabelText('Базовый URL')).toHaveValue('http://admin-api:7771')
    expect(screen.getByLabelText('Публичный URL веб-интерфейса')).toHaveValue(
      'http://localhost:7772',
    )
    expect(screen.getByLabelText('ui.render')).toBeChecked()
    fireEvent.change(screen.getByLabelText('Базовый URL'), {
      target: { value: 'http://admin-api:7901' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Отправить декларацию' }))
    expect(patchService).toHaveBeenCalledWith(
      {
        version: 3,
        body: {
          declaration: {
            declaration_version: 5,
            integration_base_url: 'http://admin-api:7901',
            public_ui_url: 'http://localhost:7772',
            service_contract_version: '1.0.0',
            capabilities: ['health.read', 'ui.render'],
          },
        },
      },
      expect.any(Object),
    )

    vi.mocked(useService).mockReturnValue({
      isLoading: false,
      isError: false,
      data: {
        service: {
          id: 's-1',
          service_key: 'admin-panel',
          display_name: 'Admin Panel',
          owner_team: 'platform',
          status: 'active',
          version: 4,
          active_declaration_id: 'd-2',
        },
        declarations: [
          {
            id: 'd-2',
            declaration_version: 5,
            integration_base_url: 'http://admin-api:7902',
            public_ui_url: 'http://localhost:7902',
            service_contract_version: '1.0.0',
            capabilities: ['health.read', 'ui.render'],
            approval_status: 'approved',
          },
        ],
      },
    } as unknown as ReturnType<typeof useService>)
    view.rerender(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )
    expect(screen.getByLabelText('Базовый URL')).toHaveValue('http://admin-api:7901')
    act(() => patchService.mock.calls[0]![1].onSuccess())
    expect(screen.getByLabelText('Базовый URL')).toHaveValue('http://admin-api:7902')
    expect(screen.getByLabelText('Публичный URL веб-интерфейса')).toHaveValue(
      'http://localhost:7902',
    )
  })

  it('does not claim a new pending declaration after an idempotent submission', () => {
    vi.mocked(useService).mockReturnValue({
      isLoading: false,
      isError: false,
      data: {
        service: {
          id: 's-1',
          service_key: 'admin-panel',
          display_name: 'Admin Panel',
          owner_team: 'platform',
          status: 'active',
          version: 3,
          active_declaration_id: 'd-1',
        },
        declarations: [
          {
            id: 'd-1',
            declaration_version: 4,
            integration_base_url: 'http://admin-api:7771',
            public_ui_url: 'http://localhost:7772',
            service_contract_version: '1.0.0',
            capabilities: ['health.read', 'ui.render'],
            approval_status: 'approved',
          },
        ],
      },
    } as unknown as ReturnType<typeof useService>)
    render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Отправить декларацию' }))
    expect(patchService).toHaveBeenCalledTimes(1)
    act(() => patchService.mock.calls[0]![1].onSuccess())
    expect(toast.success).toHaveBeenCalledWith(
      'Декларация обработана; актуальный статус указан в истории',
    )
  })

  it('runs only a declared known capability and renders check history', () => {
    vi.mocked(useService).mockReturnValue({
      isLoading: false,
      isError: false,
      data: {
        service: {
          id: 's-1',
          service_key: 'admin-panel',
          display_name: 'Admin Panel',
          owner_team: 'platform',
          status: 'active',
          version: 3,
          active_declaration_id: 'd-1',
        },
        declarations: [
          {
            id: 'd-1',
            declaration_version: 4,
            integration_base_url: 'http://admin-api:7771',
            public_ui_url: null,
            service_contract_version: '1.0.0',
            capabilities: ['health.read', 'branding.runtime.read', 'unknown.write'],
            approval_status: 'approved',
          },
        ],
      },
    } as unknown as ReturnType<typeof useService>)
    vi.mocked(useServiceChecks).mockReturnValue({
      isLoading: false,
      isFetching: false,
      isError: false,
      data: {
        total: 1,
        checks: [
          {
            id: 'check-1',
            capability_key: 'health.read',
            outcome: 'success',
            http_status: 200,
            summary: 'HTTP 200',
            triggered_by_subject: 'operator@example.test',
            started_at: '2026-09-21T09:00:00Z',
          },
        ],
      },
      refetch: refetchChecks,
    } as unknown as ReturnType<typeof useServiceChecks>)
    render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === 'P' &&
          Boolean(element.textContent?.includes('operator@example.test')),
      ),
    ).toBeVisible()
    expect(screen.getByText('HTTP 200')).toBeVisible()
    const capability = screen.getByLabelText('Возможность')
    expect(within(capability).queryByRole('option', { name: 'unknown.write' })).toBeNull()
    fireEvent.change(capability, { target: { value: 'branding.runtime.read' } })
    fireEvent.click(screen.getByRole('button', { name: 'Запустить проверку' }))
    expect(runCheck).toHaveBeenCalledWith('branding.runtime.read', expect.any(Object))
    act(() =>
      runCheck.mock.calls[0]![1].onSuccess({
        check_run: { outcome: 'success' },
      }),
    )
    expect(toast.success).toHaveBeenCalledWith('Проверка завершена успешно')
  })

  it('offers retry when check history fails', () => {
    vi.mocked(useServiceChecks).mockReturnValue({
      isLoading: false,
      isFetching: false,
      isError: true,
      data: undefined,
      refetch: refetchChecks,
    } as unknown as ReturnType<typeof useServiceChecks>)
    render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('Не удалось загрузить историю проверок')
    fireEvent.click(screen.getByRole('button', { name: 'Повторить' }))
    expect(refetchChecks).toHaveBeenCalledOnce()
  })

  it('keeps the detail read-only for a viewer', () => {
    vi.mocked(useAuth).mockReturnValue({ canMutate: false } as ReturnType<typeof useAuth>)
    render(
      <MemoryRouter initialEntries={['/services/admin-panel']}>
        <Routes>
          <Route path="/services/:serviceKey" element={<ServiceDetailPage />} />
        </Routes>
      </MemoryRouter>,
    )
    expect(screen.getByText('Admin Panel')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Отправить декларацию' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Отключить' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Запустить проверку' })).not.toBeInTheDocument()
  })
})
