import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/shared/api/client'
import { useBrandingRevisions } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'
import { RevisionsPage } from './index'

vi.mock('@/shared/api/client', () => ({ api: { post: vi.fn() } }))
vi.mock('@/shared/api/hooks', () => ({ useBrandingRevisions: vi.fn() }))
vi.mock('@/shared/auth/auth-context', () => ({ useAuth: vi.fn() }))
vi.mock('sonner', () => ({ toast: { success: vi.fn() } }))

function renderRevisions() {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <RevisionsPage />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(useAuth).mockReturnValue({ canMutate: true } as ReturnType<typeof useAuth>)
  vi.mocked(api.post).mockResolvedValue({})
  vi.mocked(useBrandingRevisions).mockReturnValue({
    isLoading: false,
    isPending: false,
    isError: false,
    data: {
      total: 2,
      revisions: [
        {
          id: 'draft-4',
          revision: 4,
          state: 'draft',
          based_on_revision: 3,
          document: {
            product_name: 'SDLC next',
            product_short_name: 'SD',
            logo_url: null,
            favicon_url: null,
            support_url: null,
            primary_color: '#5871da',
            accent_color: '#2aa88b',
            surface_color: '#171717',
          },
          document_hash: 'hash-4',
          etag: 'etag-4',
          created_by_subject: 'test',
          created_at: '2026-09-19T06:30:00Z',
          published_by_subject: null,
          published_at: null,
        },
        {
          id: 'published-3',
          revision: 3,
          state: 'published',
          based_on_revision: null,
          document: {
            product_name: 'SDLC',
            product_short_name: 'SD',
            logo_url: null,
            favicon_url: null,
            support_url: null,
            primary_color: '#5871da',
            accent_color: '#2aa88b',
            surface_color: '#171717',
          },
          document_hash: 'hash-3',
          etag: 'etag-3',
          created_by_subject: 'test',
          created_at: '2026-09-18T06:30:00Z',
          published_by_subject: 'test',
          published_at: '2026-09-18T06:31:00Z',
        },
      ],
    },
  } as ReturnType<typeof useBrandingRevisions>)
})

describe('RevisionsPage', () => {
  it('keeps draft comparison available in read-only mode without mutation commands', () => {
    vi.mocked(useAuth).mockReturnValue({ canMutate: false } as ReturnType<typeof useAuth>)
    renderRevisions()

    expect(screen.getAllByText('Только чтение')).toHaveLength(2)
    expect(screen.queryByRole('button', { name: 'Опубликовать' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Отозвать' })).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('button', { name: 'Сравнить' })[0]!)
    expect(screen.getByText('Изменения относительно базовой ревизии')).toBeInTheDocument()
    expect(api.post).not.toHaveBeenCalled()
  })

  it('does not publish until the specific revision is confirmed', async () => {
    renderRevisions()

    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('Опубликовать версию v4?')).toBeInTheDocument()
    expect(api.post).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Отмена' }))
    expect(api.post).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))
    fireEvent.click(
      within(await screen.findByRole('alertdialog')).getByRole('button', { name: 'Опубликовать' }),
    )
    await waitFor(() =>
      expect(api.post).toHaveBeenCalledWith('/api/v1/branding/revisions/4/publish'),
    )
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('does not withdraw until confirmed and keeps failures visible for retry', async () => {
    vi.mocked(api.post).mockRejectedValueOnce(new Error('network'))
    renderRevisions()

    fireEvent.click(screen.getByRole('button', { name: 'Отозвать' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('Отозвать черновик v4?')).toBeInTheDocument()
    expect(api.post).not.toHaveBeenCalled()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Отозвать' }))
    await waitFor(() =>
      expect(api.post).toHaveBeenCalledWith('/api/v1/branding/revisions/4/withdraw'),
    )
    expect(await within(dialog).findByRole('alert')).toHaveTextContent(
      'Не удалось отозвать черновик',
    )

    fireEvent.click(within(dialog).getByRole('button', { name: 'Отозвать' }))
    await waitFor(() => expect(api.post).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })
})
