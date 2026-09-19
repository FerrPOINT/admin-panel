import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Eye, Send, Undo2, GitCompare } from 'lucide-react'
import { toast } from 'sonner'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@sdlc/ui/ui'
import { api } from '@/shared/api/client'
import {
  type BrandingDocument,
  type BrandingRevision,
  useBrandingRevisions,
} from '@/shared/api/hooks'

const FIELDS: (keyof BrandingDocument)[] = [
  'product_name',
  'product_short_name',
  'logo_url',
  'favicon_url',
  'support_url',
  'primary_color',
  'accent_color',
  'surface_color',
]

const FIELD_LABELS: Record<string, string> = {
  product_name: 'Название платформы',
  product_short_name: 'Короткое название',
  logo_url: 'URL логотипа',
  favicon_url: 'URL favicon',
  support_url: 'URL поддержки',
  primary_color: 'Основной цвет',
  accent_color: 'Второстепенный цвет',
  surface_color: 'Поверхность',
}
const STATE_LABELS: Record<BrandingRevision['state'], string> = {
  draft: 'Черновик',
  published: 'Опубликована',
  superseded: 'Заменена',
  withdrawn: 'Отозвана',
}

function diffDocuments(base: BrandingDocument | undefined, next: BrandingDocument | undefined) {
  if (!base || !next) return []
  return FIELDS.filter((field) => base[field] !== next[field]).map((field) => ({
    field,
    from: String(base[field] ?? '—'),
    to: String(next[field] ?? '—'),
  }))
}

function RevisionDiff({
  base,
  next,
  label,
}: {
  base: BrandingDocument
  next: BrandingDocument
  label: string
}) {
  const diff = diffDocuments(base, next)
  return (
    <div className="mt-3 border-t border-border pt-3">
      <div className="mb-2 flex items-center gap-1.5 text-xs font-medium text-text-secondary">
        <GitCompare className="h-3.5 w-3.5" />
        {label}
      </div>
      {diff.length === 0 ? (
        <div className="text-xs text-text-muted">Нет отличий от базовой ревизии.</div>
      ) : (
        <div className="space-y-1">
          {diff.map((change) => (
            <div key={change.field} className="flex min-w-0 flex-wrap items-baseline gap-2 text-xs">
              <span className="w-36 shrink-0 text-text-muted">
                {FIELD_LABELS[change.field] ?? change.field}
              </span>
              <code className="min-w-0 break-all text-danger line-through">{change.from}</code>
              <span className="text-text-muted">→</span>
              <code className="min-w-0 break-all text-success">{change.to}</code>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

export function RevisionsPage() {
  const revisions = useBrandingRevisions()
  const queryClient = useQueryClient()
  const [expanded, setExpanded] = useState<number | null>(null)
  const [pendingAction, setPendingAction] = useState<{
    type: 'publish' | 'withdraw'
    revision: number
  } | null>(null)

  const publish = useMutation({
    mutationFn: (revision: number) => api.post(`/api/v1/branding/revisions/${revision}/publish`),
    onSuccess: () => {
      toast.success('Конфигурация опубликована')
      setPendingAction(null)
      void queryClient.invalidateQueries({ queryKey: ['branding-revisions'] })
    },
  })
  const withdraw = useMutation({
    mutationFn: (revision: number) => api.post(`/api/v1/branding/revisions/${revision}/withdraw`),
    onSuccess: () => {
      toast.success('Черновик отозван')
      setPendingAction(null)
      void queryClient.invalidateQueries({ queryKey: ['branding-revisions'] })
    },
  })

  const actionPending = publish.isPending || withdraw.isPending
  const actionError = pendingAction?.type === 'publish' ? publish.isError : withdraw.isError

  function openAction(type: 'publish' | 'withdraw', revision: number) {
    publish.reset()
    withdraw.reset()
    setPendingAction({ type, revision })
  }

  function confirmAction() {
    if (!pendingAction || actionPending) return
    if (pendingAction.type === 'publish') publish.mutate(pendingAction.revision)
    else withdraw.mutate(pendingAction.revision)
  }

  const list = revisions.data?.revisions ?? []
  const byNumber = new Map(list.map((r) => [r.revision, r]))
  const published = list.find((r) => r.state === 'published')

  const baseFor = (revision: BrandingRevision): BrandingDocument | undefined => {
    if (revision.based_on_revision != null) {
      return byNumber.get(revision.based_on_revision)?.document
    }
    return published && published.revision !== revision.revision ? published.document : undefined
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Конфигурации</h1>
        <p className="mt-1 text-sm text-text-muted">
          Неизменяемая история брендинга. Откат создаёт новую публикацию, а не переписывает прошлое.
        </p>
      </div>
      <div className="overflow-hidden rounded-lg border border-border bg-surface">
        <div className="hidden grid-cols-[80px_1fr_140px_180px_200px] gap-4 border-b border-border px-4 py-3 text-xs text-text-muted md:grid">
          <span>Версия</span>
          <span>Название</span>
          <span>Статус</span>
          <span>Создана</span>
          <span />
        </div>
        {revisions.isLoading ? (
          <div className="p-5 text-sm text-text-muted">Загрузка версий...</div>
        ) : null}
        {revisions.isError ? (
          <div role="alert" className="p-5 text-sm text-danger">
            Не удалось загрузить конфигурации.{' '}
            <button type="button" className="underline" onClick={() => void revisions.refetch()}>
              Повторить
            </button>
          </div>
        ) : null}
        {list.map((revision) => {
          const base = baseFor(revision)
          const isOpen = expanded === revision.revision
          return (
            <div
              key={revision.id}
              className="border-b border-border px-4 py-4 text-sm last:border-0"
            >
              <div className="grid gap-2 md:grid-cols-[80px_1fr_140px_180px_200px] md:items-center md:gap-4">
                <span className="font-mono text-text-secondary">
                  v{revision.revision}
                  {revision.based_on_revision != null ? (
                    <span className="ml-1 text-[10px] text-text-muted">
                      ← v{revision.based_on_revision}
                    </span>
                  ) : null}
                </span>
                <span>{revision.document.product_name}</span>
                <span
                  className={
                    revision.state === 'published'
                      ? 'text-success'
                      : revision.state === 'draft'
                        ? 'text-warning'
                        : revision.state === 'withdrawn'
                          ? 'text-danger'
                          : 'text-text-muted'
                  }
                >
                  {STATE_LABELS[revision.state]}
                </span>
                <span className="text-text-muted">
                  {new Date(revision.created_at).toLocaleString('ru-RU')}
                </span>
                <span className="flex flex-wrap gap-2">
                  {revision.state === 'draft' ? (
                    <>
                      <button
                        type="button"
                        onClick={() => openAction('publish', revision.revision)}
                        disabled={actionPending}
                        className="inline-flex min-h-10 items-center gap-1 rounded border border-border px-3 text-xs hover:bg-surface-raised"
                      >
                        <Send className="h-4 w-4" />
                        Опубликовать
                      </button>
                      <button
                        type="button"
                        onClick={() => openAction('withdraw', revision.revision)}
                        disabled={actionPending}
                        className="inline-flex min-h-10 items-center gap-1 rounded border border-border px-3 text-xs text-danger hover:bg-surface-raised"
                      >
                        <Undo2 className="h-4 w-4" />
                        Отозвать
                      </button>
                    </>
                  ) : (
                    <span className="inline-flex items-center gap-1 text-xs text-text-muted">
                      <Eye className="h-4 w-4" />
                      Только чтение
                    </span>
                  )}
                  <button
                    type="button"
                    onClick={() => setExpanded(isOpen ? null : revision.revision)}
                    className="inline-flex min-h-10 items-center gap-1 rounded border border-border px-3 text-xs hover:bg-surface-raised"
                  >
                    <GitCompare className="h-4 w-4" />
                    {isOpen ? 'Скрыть сравнение' : 'Сравнить'}
                  </button>
                </span>
              </div>
              {isOpen && base ? (
                <RevisionDiff
                  base={base}
                  next={revision.document}
                  label={`Изменения относительно базовой ревизии`}
                />
              ) : null}
              {isOpen && !base ? (
                <div className="mt-3 border-t border-border pt-3 text-xs text-text-muted">
                  Базовая ревизия недоступна (первая публикация или база отозвана).
                </div>
              ) : null}
            </div>
          )
        })}
        {!revisions.isPending && !revisions.isError && list.length === 0 ? (
          <div className="p-8 text-center text-sm text-text-muted">
            Черновиков и опубликованных ревизий пока нет.
          </div>
        ) : null}
      </div>
      <AlertDialog
        open={pendingAction !== null}
        onOpenChange={(open) => {
          if (!open && !actionPending) setPendingAction(null)
        }}
      >
        <AlertDialogContent className="w-[calc(100vw-2rem)] max-w-md rounded-md">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pendingAction?.type === 'publish'
                ? `Опубликовать версию v${pendingAction.revision}?`
                : `Отозвать черновик v${pendingAction?.revision}?`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingAction?.type === 'publish'
                ? 'Эта конфигурация станет текущей для всей платформы. Предыдущая публикация сохранится в истории.'
                : 'Черновик будет отозван и больше не сможет быть опубликован.'}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {actionError && (
            <p role="alert" className="text-sm text-danger">
              {pendingAction?.type === 'publish'
                ? 'Не удалось опубликовать конфигурацию.'
                : 'Не удалось отозвать черновик.'}
            </p>
          )}
          <div className="flex flex-wrap justify-end gap-2">
            <AlertDialogCancel disabled={actionPending} className="min-h-10 sm:min-h-10">
              Отмена
            </AlertDialogCancel>
            <AlertDialogAction
              disabled={actionPending}
              className={
                pendingAction?.type === 'publish'
                  ? 'min-h-10 bg-accent text-accent-foreground hover:bg-accent-hover sm:min-h-10'
                  : 'min-h-10 sm:min-h-10'
              }
              onClick={(event) => {
                event.preventDefault()
                confirmAction()
              }}
            >
              {actionPending
                ? 'Выполняется...'
                : pendingAction?.type === 'publish'
                  ? 'Опубликовать'
                  : 'Отозвать'}
            </AlertDialogAction>
          </div>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
