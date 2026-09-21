import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { api } from '@/shared/api/client'
import { useBrandingRevisions } from '@/shared/api/hooks'
import { BrandingPage } from './index'

vi.mock('@/shared/api/client', () => ({ api: { post: vi.fn() } }))
vi.mock('@/shared/api/hooks', () => ({ useBrandingRevisions: vi.fn() }))

function renderBranding() {
  const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <BrandingPage />
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(useBrandingRevisions).mockReturnValue({
    isPending: false,
    isError: false,
    data: {
      total: 1,
      revisions: [
        {
          state: 'published',
          revision: 3,
          document: {
            product_name: 'SDLC',
            product_short_name: 'SD',
            logo_url: null,
            favicon_url: null,
            support_url: null,
            primary_color: '#2563eb',
            accent_color: '#14b8a6',
            surface_color: '#f8fafc',
          },
        },
      ],
    },
  } as ReturnType<typeof useBrandingRevisions>)
})

describe('BrandingPage publication', () => {
  it('retries publishing the saved draft without creating another revision', async () => {
    vi.mocked(api.post)
      .mockResolvedValueOnce({ revision: { revision: 4 } })
      .mockRejectedValueOnce(new Error('network'))
      .mockResolvedValueOnce({})
    renderBranding()

    fireEvent.change(screen.getByLabelText('Название платформы'), {
      target: { value: 'SDLC next' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Черновик v4 сохранён')
    expect(screen.getByLabelText('Название платформы')).toHaveValue('SDLC next')
    expect(screen.getByLabelText('Название платформы')).toBeDisabled()

    fireEvent.click(screen.getByRole('button', { name: 'Повторить публикацию' }))
    await waitFor(() => expect(screen.getByText('Опубликована ревизия v4')).toBeInTheDocument())
    expect(vi.mocked(api.post).mock.calls.map(([url]) => url)).toEqual([
      '/api/v1/branding/revisions',
      '/api/v1/branding/revisions/4/publish',
      '/api/v1/branding/revisions/4/publish',
    ])
    expect(screen.getByLabelText('Название платформы')).not.toBeDisabled()
  })

  it('locks every field while creating and publishing, then clears success on edit', async () => {
    let resolveDraft!: (value: { revision: { revision: number } }) => void
    let resolvePublish!: (value: object) => void
    vi.mocked(api.post)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveDraft = resolve
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolvePublish = resolve
          }),
      )
    renderBranding()

    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Публикация...' })).toBeDisabled(),
    )
    expect(screen.getByLabelText('Название платформы')).toBeDisabled()
    expect(screen.getByLabelText('Основной цвет: HEX')).toBeDisabled()
    expect(screen.getByLabelText('Основной цвет: выбрать цвет')).toBeDisabled()
    expect(screen.getByLabelText('URL поддержки')).toBeDisabled()

    resolveDraft({ revision: { revision: 4 } })
    await waitFor(() =>
      expect(api.post).toHaveBeenCalledWith('/api/v1/branding/revisions/4/publish'),
    )
    expect(screen.getByLabelText('Название платформы')).toBeDisabled()
    expect(screen.getByLabelText('URL поддержки')).toBeDisabled()

    resolvePublish({})
    await waitFor(() => expect(screen.getByText('Опубликована ревизия v4')).toBeInTheDocument())
    fireEvent.change(screen.getByLabelText('Название платформы'), { target: { value: 'Other' } })
    expect(screen.queryByText('Опубликована ревизия v4')).not.toBeInTheDocument()
  })

  it('keeps the editable form after draft creation fails', async () => {
    vi.mocked(api.post).mockRejectedValueOnce(new Error('network'))
    renderBranding()

    fireEvent.change(screen.getByLabelText('Название платформы'), { target: { value: 'Unsaved' } })
    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Не удалось создать черновик')
    expect(screen.getByLabelText('Название платформы')).toHaveValue('Unsaved')
    expect(screen.getByLabelText('Название платформы')).not.toBeDisabled()
  })

  it('can edit values after a failed publish without silently reusing the old draft', async () => {
    vi.mocked(api.post)
      .mockResolvedValueOnce({ revision: { revision: 4 } })
      .mockRejectedValueOnce(new Error('network'))
      .mockResolvedValueOnce({ revision: { revision: 5 } })
      .mockResolvedValueOnce({})
    renderBranding()

    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Черновик v4 сохранён')
    fireEvent.click(screen.getByRole('button', { name: 'Изменить значения' }))
    expect(screen.getByRole('status')).toHaveTextContent('Черновик v4 остался в истории')
    fireEvent.change(screen.getByLabelText('Название платформы'), { target: { value: 'Revised' } })
    fireEvent.click(screen.getByRole('button', { name: 'Опубликовать' }))

    await waitFor(() => expect(screen.getByText('Опубликована ревизия v5')).toBeInTheDocument())
    expect(vi.mocked(api.post).mock.calls.map(([url]) => url)).toEqual([
      '/api/v1/branding/revisions',
      '/api/v1/branding/revisions/4/publish',
      '/api/v1/branding/revisions',
      '/api/v1/branding/revisions/5/publish',
    ])
    expect(vi.mocked(api.post).mock.calls[2]?.[1]).toMatchObject({ product_name: 'Revised' })
  })
})
