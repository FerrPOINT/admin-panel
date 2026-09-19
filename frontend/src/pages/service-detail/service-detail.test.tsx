import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { useApproveService, useChangeServiceStatus, usePatchService, useService } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'
import { ServiceDetailPage } from './index'

vi.mock('@/shared/api/hooks', () => ({
  useService: vi.fn(),
  useApproveService: vi.fn(),
  usePatchService: vi.fn(),
  useChangeServiceStatus: vi.fn(),
}))
vi.mock('@/shared/auth/auth-context', () => ({ useAuth: vi.fn() }))

const changeStatus = vi.fn()

beforeEach(() => {
  changeStatus.mockClear()
  vi.mocked(useAuth).mockReturnValue({ canMutate: true } as ReturnType<typeof useAuth>)
  vi.mocked(useService).mockReturnValue({
    isLoading: false,
    isError: false,
    data: {
      service: { id: 's-1', service_key: 'admin-panel', display_name: 'Admin Panel', owner_team: 'platform', status: 'active', version: 1, active_declaration_id: null },
      declarations: [],
    },
  } as unknown as ReturnType<typeof useService>)
  vi.mocked(useApproveService).mockReturnValue({ isPending: false, mutate: vi.fn() } as unknown as ReturnType<typeof useApproveService>)
  vi.mocked(usePatchService).mockReturnValue({ isPending: false, mutate: vi.fn() } as unknown as ReturnType<typeof usePatchService>)
  vi.mocked(useChangeServiceStatus).mockReturnValue({ isPending: false, mutate: changeStatus } as unknown as ReturnType<typeof useChangeServiceStatus>)
})

describe('ServiceDetailPage', () => {
  it('requires confirmation before disabling a service', async () => {
    render(<MemoryRouter initialEntries={['/services/admin-panel']}><Routes><Route path="/services/:serviceKey" element={<ServiceDetailPage />} /></Routes></MemoryRouter>)

    fireEvent.click(screen.getByRole('button', { name: 'Отключить' }))
    expect(changeStatus).not.toHaveBeenCalled()
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('Отключить сервис?')).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Отключить' }))
    expect(changeStatus).toHaveBeenCalledWith({ action: 'disable', version: 1 }, expect.any(Object))
  })
})
