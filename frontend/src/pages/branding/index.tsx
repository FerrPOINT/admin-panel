import { useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Check, Palette } from 'lucide-react'
import { api } from '@/shared/api/client'
import { type BrandingDocument, useBrandingRevisions } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'

const DEFAULT_BRANDING: BrandingDocument = {
  product_name: 'Base Platform',
  product_short_name: 'Base',
  logo_url: null,
  favicon_url: null,
  support_url: null,
  primary_color: '#2563eb',
  accent_color: '#14b8a6',
  surface_color: '#f8fafc',
}

function currentDocument(revisions: ReturnType<typeof useBrandingRevisions>['data']) {
  return revisions?.revisions.find((r) => r.state === 'published')?.document ?? DEFAULT_BRANDING
}

export function readableForeground(color: string): '#000000' | '#ffffff' {
  if (!/^#[0-9a-f]{6}$/i.test(color)) return '#000000'
  const channels = [1, 3, 5].map((index) => {
    const channel = parseInt(color.slice(index, index + 2), 16) / 255
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  })
  const luminance = 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!
  return (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) ? '#000000' : '#ffffff'
}

export function BrandingPage() {
  const { canMutate } = useAuth()
  const revisions = useBrandingRevisions()
  const queryClient = useQueryClient()
  const [form, setForm] = useState<BrandingDocument | null>(null)
  const document = form ?? currentDocument(revisions.data)
  const [published, setPublished] = useState<number | null>(null)
  const [draftRevision, setDraftRevision] = useState<number | null>(null)
  const [previousDraft, setPreviousDraft] = useState<number | null>(null)
  const savingRef = useRef(false)

  const createDraft = useMutation({
    mutationFn: (payload: BrandingDocument) =>
      api.post<{ revision: { revision: number } }>('/api/v1/branding/revisions', payload),
    onSuccess: (data) =>
      queryClient.invalidateQueries({ queryKey: ['branding-revisions'] }).then(() => data),
  })
  const publish = useMutation({
    mutationFn: (revision: number) => api.post(`/api/v1/branding/revisions/${revision}/publish`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['branding-revisions'] }),
  })

  const saving = createDraft.isPending || publish.isPending
  const formLocked = !canMutate || saving || draftRevision !== null
  const update = <K extends keyof BrandingDocument>(key: K, value: BrandingDocument[K]) => {
    setForm({ ...document, [key]: value })
    setPublished(null)
  }

  const saveAndPublish = async () => {
    if (!canMutate || savingRef.current) return
    savingRef.current = true
    setPublished(null)
    try {
      const revision = draftRevision ?? (await createDraft.mutateAsync(document)).revision.revision
      setDraftRevision(revision)
      await publish.mutateAsync(revision)
      setDraftRevision(null)
      setPreviousDraft(null)
      setPublished(revision)
    } catch {
      // Mutation errors are rendered below without discarding the draft form.
    } finally {
      savingRef.current = false
    }
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Брендинг платформы</h1>
          <p className="mt-1 text-sm text-text-muted">
            Публикуется как проверяемая конфигурация. Произвольный CSS не допускается.
          </p>
        </div>
        {canMutate ? (
          <button
            type="button"
            onClick={saveAndPublish}
            disabled={revisions.isPending || revisions.isError || saving}
            className="min-h-10 rounded-md bg-accent px-4 text-sm font-medium text-accent-foreground hover:bg-accent-hover disabled:opacity-50"
          >
            {saving
              ? 'Публикация...'
              : draftRevision !== null
                ? 'Повторить публикацию'
                : 'Опубликовать'}
          </button>
        ) : (
          <span className="rounded border border-border px-2 py-1 text-xs text-text-muted">
            Только чтение
          </span>
        )}
      </div>

      {revisions.isPending && (
        <p role="status" className="text-sm text-text-muted">
          Загрузка текущего брендинга…
        </p>
      )}
      {revisions.isError && (
        <p role="alert" className="text-sm text-danger">
          Не удалось загрузить текущий брендинг.{' '}
          <button type="button" className="underline" onClick={() => void revisions.refetch()}>
            Повторить
          </button>
        </p>
      )}

      {createDraft.isError || publish.isError ? (
        <div
          role="alert"
          className="rounded-md border border-danger/40 bg-danger/10 p-3 text-sm text-danger"
        >
          {draftRevision !== null
            ? `Черновик v${draftRevision} сохранён, но публикация не удалась.`
            : 'Не удалось создать черновик. Проверьте поля и доступность API.'}
          {draftRevision !== null && (
            <button
              type="button"
              className="ml-2 min-h-10 underline"
              onClick={() => {
                setPreviousDraft(draftRevision)
                setDraftRevision(null)
                publish.reset()
              }}
            >
              Изменить значения
            </button>
          )}
        </div>
      ) : null}
      {previousDraft !== null && (
        <p role="status" className="text-sm text-text-muted">
          Черновик v{previousDraft} остался в истории без публикации. Следующая попытка создаст
          новую ревизию.
        </p>
      )}
      {published ? (
        <div className="flex items-center gap-2 rounded-md border border-success/30 bg-success/10 p-3 text-sm text-success">
          <Check className="h-4 w-4" /> Опубликована ревизия v{published}
        </div>
      ) : null}

      <div className="grid gap-6 lg:grid-cols-[1fr_360px]">
        <section className="space-y-4 rounded-lg border border-border bg-surface p-5">
          <h2 className="text-sm font-medium text-text-secondary">Параметры</h2>
          <label className="block text-sm">
            <span className="mb-1.5 block text-text-secondary">Название платформы</span>
            <input
              disabled={formLocked}
              value={document.product_name}
              onChange={(e) => update('product_name', e.target.value)}
              className="min-h-10 w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent"
            />
          </label>
          <label className="block text-sm">
            <span className="mb-1.5 block text-text-secondary">Короткое название</span>
            <input
              disabled={formLocked}
              value={document.product_short_name}
              onChange={(e) => update('product_short_name', e.target.value)}
              className="min-h-10 w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent"
            />
          </label>
          <div className="grid gap-4 sm:grid-cols-2">
            {(
              [
                ['primary_color', 'Основной цвет'],
                ['accent_color', 'Второстепенный цвет'],
                ['surface_color', 'Поверхность'],
              ] as const
            ).map(([key, label]) => (
              <div key={key} className="block text-sm">
                <span className="mb-1.5 block text-text-secondary">{label}</span>
                <span className="flex overflow-hidden rounded-md border border-border bg-background focus-within:border-accent">
                  <input
                    type="color"
                    disabled={formLocked}
                    aria-label={`${label}: выбрать цвет`}
                    value={document[key] ?? '#ffffff'}
                    onChange={(e) => update(key, e.target.value)}
                    className="h-10 w-11 border-0 bg-transparent p-1"
                  />
                  <input
                    disabled={formLocked}
                    aria-label={`${label}: HEX`}
                    value={document[key] ?? ''}
                    onChange={(e) => update(key, e.target.value)}
                    className="min-w-0 flex-1 bg-transparent px-2 outline-none"
                  />
                </span>
              </div>
            ))}
          </div>
          <label className="block text-sm">
            <span className="mb-1.5 block text-text-secondary">URL поддержки</span>
            <input
              disabled={formLocked}
              value={document.support_url ?? ''}
              onChange={(e) => update('support_url', e.target.value || null)}
              placeholder="https://..."
              className="min-h-10 w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent"
            />
          </label>
        </section>

        <section className="rounded-lg border border-border bg-surface p-5">
          <h2 className="mb-4 text-sm font-medium text-text-secondary">Предпросмотр</h2>
          <div
            className="overflow-hidden rounded-xl border border-border"
            style={{ background: document.surface_color ?? '#f8fafc' }}
          >
            <div
              className="flex items-center justify-between px-4 py-3"
              style={{
                background: document.primary_color,
                color: readableForeground(document.primary_color),
              }}
            >
              <span className="flex items-center gap-2 text-sm font-semibold">
                <Palette className="h-4 w-4" />
                {document.product_short_name}
              </span>
              <span className="text-xs">Войти</span>
            </div>
            <div
              className="p-4"
              style={{ color: readableForeground(document.surface_color ?? '#f8fafc') }}
            >
              <div className="text-base font-semibold">{document.product_name}</div>
              <div className="mt-3 rounded-lg bg-white p-3 text-sm text-[#1e293b] shadow-sm">
                <div className="font-medium">Карточка приложения</div>
                <span
                  className="mt-3 inline-block rounded-md px-3 py-1.5 text-xs font-medium"
                  style={{
                    background: document.accent_color,
                    color: readableForeground(document.accent_color),
                  }}
                >
                  Действие
                </span>
              </div>
            </div>
          </div>
          <p className="mt-3 text-xs text-text-muted">
            Приложения применят только утверждённые цвета; при недоступности API используются
            встроенные значения.
          </p>
        </section>
      </div>
    </div>
  )
}
