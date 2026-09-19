import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { useAuditEvents, useBrandingRevisions, useServices } from '@/shared/api/hooks'
import { OverviewPage } from './index'

vi.mock('@/shared/api/hooks', () => ({
  useServices: vi.fn(),
  useBrandingRevisions: vi.fn(),
  useAuditEvents: vi.fn(),
}))

const servicesMock = vi.mocked(useServices)
const revisionsMock = vi.mocked(useBrandingRevisions)
const auditMock = vi.mocked(useAuditEvents)

function renderOverview() {
  return render(<MemoryRouter><OverviewPage /></MemoryRouter>)
}

beforeEach(() => {
  servicesMock.mockReturnValue({
    isPending: false,
    isError: false,
    data: {
      total: 2,
      services: [
        { id: '1', service_key: 'admin-panel', display_name: 'Admin Panel', status: 'active', health_status: 'healthy' },
        { id: '2', service_key: 'ci-cd', display_name: 'CI/CD', status: 'pending', health_status: 'unknown' },
      ],
    },
  } as ReturnType<typeof useServices>)
  revisionsMock.mockReturnValue({
    isPending: false,
    isError: false,
    data: { revisions: [{ state: 'published', revision: 3 }] },
  } as ReturnType<typeof useBrandingRevisions>)
  auditMock.mockReturnValue({
    isPending: false,
    isError: false,
    data: { events: [{ id: 'e1', action: 'central_user.created', occurred_at: '2026-09-19T08:00:00Z' }] },
  } as ReturnType<typeof useAuditEvents>)
})

describe('OverviewPage', () => {
  it('shows service attention and readable recent activity', () => {
    renderOverview()
    expect(screen.getByRole('heading', { name: 'Обзор платформы' })).toBeInTheDocument()
    expect(screen.getAllByText('CI/CD')).toHaveLength(2)
    expect(screen.getByText('Ожидает одобрения')).toBeInTheDocument()
    expect(screen.getByText('Добавлен пользователь')).toBeInTheDocument()
    expect(screen.getByText('Версия 3')).toBeInTheDocument()
  })

  it('keeps services visible when the audit source fails', () => {
    auditMock.mockReturnValue({ isPending: false, isError: true, data: undefined, refetch: vi.fn() } as unknown as ReturnType<typeof useAuditEvents>)
    renderOverview()
    expect(screen.getByText('Admin Panel')).toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('Не удалось загрузить изменения')
  })
})
