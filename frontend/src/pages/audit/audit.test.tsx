import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '@/shared/api/client'
import { AuditPage } from './index'

vi.mock('@/shared/api/client', () => ({ api: { get: vi.fn() } }))

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

    const action = await screen.findByText('Опубликован брендинг')
    const details = action.closest('details')
    expect(details).not.toHaveAttribute('open')
    await user.click(action)
    expect(details).toHaveAttribute('open')
    expect(screen.getByText('request-123')).toBeInTheDocument()

    await user.selectOptions(screen.getByRole('combobox', { name: 'Тип сущности' }), 'service')
    await waitFor(() => expect(api.get).toHaveBeenCalledWith(expect.stringContaining('entity_type=service')))
  })
})
