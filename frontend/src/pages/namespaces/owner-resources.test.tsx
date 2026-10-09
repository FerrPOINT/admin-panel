import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ExistingResourcePicker, OwnerCounters } from './owner-resources'

const catalog = vi.hoisted(() => ({
  services: [] as { key: string; ui_url: string }[],
}))
vi.mock('@sdlc/ui/ui', async (load) => ({
  ...(await load<object>()),
  usePlatformServices: () => catalog,
}))
vi.mock('@/shared/auth/auth-context', () => ({
  useAuth: () => ({ session: { subject: 'reader', token: 'sso-session' } }),
}))

const owner = { kind: 'tracker_project' as const, instance_id: 'tracker-instance' }
const namespace = { registry_instance_id: 'registry', namespace_id: 'namespace' }
let client: QueryClient
let request: ReturnType<typeof vi.fn>

beforeEach(() => {
  catalog.services = []
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  request = vi.fn()
  vi.stubGlobal('fetch', request)
})
afterEach(() => {
  cleanup()
  client.clear()
  vi.unstubAllGlobals()
})

function view(children: React.ReactNode) {
  return render(<QueryClientProvider client={client}>{children}</QueryClientProvider>)
}

describe('Unavailable Namespace resource owners', () => {
  it('reports an absent resource catalog instead of loading indefinitely', () => {
    view(
      <ExistingResourcePicker owner={owner} namespaceId="namespace" value="" onChange={vi.fn()} />,
    )
    expect(screen.getByRole('alert')).toHaveTextContent('Каталог продукта недоступен')
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    const retry = screen.getByRole('button', { name: 'Обновить' })
    expect(retry).toBeDisabled()
    fireEvent.click(retry)
    expect(request).not.toHaveBeenCalled()
  })

  it('reports unavailable counters and does not read a missing endpoint', () => {
    view(<OwnerCounters kind={owner.kind} ref={namespace} resourceId="project" generation={2} />)
    expect(screen.getByRole('alert')).toHaveTextContent('Показатели источника недоступны')
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Обновить' })).toBeDisabled()
    expect(request).not.toHaveBeenCalled()
  })

  it('loads the resources when the missing product becomes available', async () => {
    request.mockResolvedValue({
      ok: true,
      json: async () => [
        {
          resource: { ...owner, resource_id: 'project' },
          label: 'Project',
          resource_key: 'PROJECT',
        },
      ],
    })
    const picker = () => (
      <ExistingResourcePicker owner={owner} namespaceId="namespace" value="" onChange={vi.fn()} />
    )
    const rendered = view(picker())
    expect(request).not.toHaveBeenCalled()
    catalog.services = [{ key: 'task-tracker', ui_url: 'https://tracker.example' }]
    rendered.rerender(<QueryClientProvider client={client}>{picker()}</QueryClientProvider>)
    await waitFor(() =>
      expect(screen.getByRole('option', { name: /Project · PROJECT/ })).toBeVisible(),
    )
    expect(request).toHaveBeenCalledWith(
      'https://tracker.example/api/v1/namespace-available-resources?limit=50&offset=0',
      expect.objectContaining({ headers: { Authorization: 'Bearer sso-session' } }),
    )
  })
})
