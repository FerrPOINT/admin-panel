import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Check, Palette } from 'lucide-react'
import { api } from '@/shared/api/client'
import { type BrandingDocument, useBrandingRevisions } from '@/shared/api/hooks'

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

export function BrandingPage() {
  const revisions = useBrandingRevisions()
  const queryClient = useQueryClient()
  const [form, setForm] = useState<BrandingDocument | null>(null)
  const document = form ?? currentDocument(revisions.data)
  const [published, setPublished] = useState<number | null>(null)

  const createDraft = useMutation({
    mutationFn: () => api.post<{ revision: { revision: number } }>('/api/v1/branding/revisions', document),
    onSuccess: (data) => queryClient.invalidateQueries({ queryKey: ['branding-revisions'] }).then(() => data),
  })
  const publish = useMutation({
    mutationFn: (revision: number) => api.post(`/api/v1/branding/revisions/${revision}/publish`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['branding-revisions'] }),
  })

  const update = <K extends keyof BrandingDocument>(key: K, value: BrandingDocument[K]) =>
    setForm({ ...document, [key]: value })

  const saveAndPublish = async () => {
    const draft = await createDraft.mutateAsync()
    await publish.mutateAsync(draft.revision.revision)
    setPublished(draft.revision.revision)
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Брендинг платформы</h1>
          <p className="mt-1 text-sm text-text-muted">
            Публикуется как проверяемая runtime-конфигурация. Произвольный CSS не допускается.
          </p>
        </div>
        <button
          onClick={saveAndPublish}
          disabled={createDraft.isPending || publish.isPending}
          className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {createDraft.isPending || publish.isPending ? 'Публикация...' : 'Опубликовать'}
        </button>
      </div>

      {createDraft.isError || publish.isError ? (
        <div className="rounded-md border border-danger/40 bg-danger/10 p-3 text-sm text-danger">
          Не удалось сохранить ревизию. Проверьте поля и доступность API.
        </div>
      ) : null}
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
            <input value={document.product_name} onChange={(e) => update('product_name', e.target.value)} className="w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent" />
          </label>
          <label className="block text-sm">
            <span className="mb-1.5 block text-text-secondary">Короткое название</span>
            <input value={document.product_short_name} onChange={(e) => update('product_short_name', e.target.value)} className="w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent" />
          </label>
          <div className="grid gap-4 sm:grid-cols-2">
            {([
              ['primary_color', 'Основной цвет'],
              ['accent_color', 'Второстепенный цвет'],
              ['surface_color', 'Поверхность'],
            ] as const).map(([key, label]) => (
              <label key={key} className="block text-sm">
                <span className="mb-1.5 block text-text-secondary">{label}</span>
                <span className="flex overflow-hidden rounded-md border border-border bg-background focus-within:border-accent">
                  <input type="color" value={document[key] ?? '#ffffff'} onChange={(e) => update(key, e.target.value)} className="h-10 w-11 border-0 bg-transparent p-1" />
                  <input value={document[key] ?? ''} onChange={(e) => update(key, e.target.value)} className="min-w-0 flex-1 bg-transparent px-2 outline-none" />
                </span>
              </label>
            ))}
          </div>
          <label className="block text-sm">
            <span className="mb-1.5 block text-text-secondary">URL поддержки</span>
            <input value={document.support_url ?? ''} onChange={(e) => update('support_url', e.target.value || null)} placeholder="https://..." className="w-full rounded-md border border-border bg-background px-3 py-2 outline-none focus:border-accent" />
          </label>
        </section>

        <section className="rounded-lg border border-border bg-surface p-5">
          <h2 className="mb-4 text-sm font-medium text-text-secondary">Предпросмотр</h2>
          <div className="overflow-hidden rounded-xl border border-border" style={{ background: document.surface_color ?? '#f8fafc' }}>
            <div className="flex items-center justify-between px-4 py-3 text-white" style={{ background: document.primary_color }}>
              <span className="flex items-center gap-2 text-sm font-semibold"><Palette className="h-4 w-4" />{document.product_short_name}</span>
              <span className="text-xs opacity-80">Войти</span>
            </div>
            <div className="p-4" style={{ color: '#1e293b' }}>
              <div className="text-base font-semibold">{document.product_name}</div>
              <div className="mt-3 rounded-lg bg-white p-3 text-sm shadow-sm">
                <div className="font-medium">Карточка приложения</div>
                <button className="mt-3 rounded-md px-3 py-1.5 text-xs font-medium text-white" style={{ background: document.accent_color }}>
                  Действие
                </button>
              </div>
            </div>
          </div>
          <p className="mt-3 text-xs text-text-muted">Потребители применят только утверждённые semantic tokens; при недоступности API работают встроенные defaults.</p>
        </section>
      </div>
    </div>
  )
}
