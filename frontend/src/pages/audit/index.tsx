import { useEffect, useState, type FormEvent } from 'react'
import { ChevronDown, ChevronLeft, ChevronRight, Copy, Filter, RotateCcw } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api } from '@/shared/api/client'
import type { AuditEvent } from '@/shared/api/hooks'
import { auditActionLabel, auditActionOptions, auditDate, shortIdentifier } from '@/shared/ui/audit-format'

const PAGE_SIZE = 20

const ENTITY_TYPES = ['', 'service', 'branding_revision', 'declaration', 'role_binding', 'central_user'] as const
const ENTITY_LABELS: Record<string, string> = {
  service: 'Сервис',
  branding_revision: 'Брендинг',
  declaration: 'Декларация',
  role_binding: 'Привязка роли',
  central_user: 'Пользователь',
}
const ROLE_LABELS: Record<string, string> = {
  platform_admin: 'Администратор',
  platform_operator: 'Оператор',
  platform_viewer: 'Наблюдатель',
}

function CopyValue({ label, value }: { label: string; value: string | null }) {
  if (!value) return null
  return (
    <div className="flex min-w-0 items-center gap-2 text-xs">
      <span className="shrink-0 text-text-muted">{label}</span>
      <code className="min-w-0 break-all text-text-secondary">{value}</code>
      <button
        type="button"
        className="inline-flex min-h-10 min-w-10 shrink-0 items-center justify-center rounded-md text-text-muted hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent"
        aria-label={`Скопировать ${label.toLowerCase()}`}
        title={`Скопировать ${label.toLowerCase()}`}
        onClick={() => void navigator.clipboard.writeText(value).then(() => toast.success('Скопировано')).catch(() => toast.error('Не удалось скопировать'))}
      >
        <Copy className="h-4 w-4" aria-hidden="true" />
      </button>
    </div>
  )
}

function AuditRow({ event }: { event: AuditEvent }) {
  return (
    <details className="group border-b border-border last:border-b-0">
      <summary className="grid min-h-12 cursor-pointer list-none grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 px-3 py-2 text-sm hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent lg:grid-cols-[130px_minmax(160px,1.3fr)_minmax(120px,1fr)_minmax(100px,1fr)_24px] lg:px-4 [&::-webkit-details-marker]:hidden">
        <time dateTime={event.occurred_at} className="col-start-2 row-start-1 whitespace-nowrap text-xs text-text-muted lg:col-start-1">
          {auditDate(event.occurred_at)}
        </time>
        <span className="col-start-1 row-start-1 min-w-0 truncate font-medium lg:col-start-2" title={event.action}>
          {auditActionLabel(event.action)}
        </span>
        <span className="col-start-1 row-start-2 min-w-0 truncate text-xs text-text-secondary lg:col-start-3 lg:row-start-1 lg:text-sm">
          {ENTITY_LABELS[event.entity_type] ?? event.entity_type}
          {event.entity_id && <span className="hidden lg:inline"> · {shortIdentifier(event.entity_id)}</span>}
        </span>
        <span className="col-start-1 row-start-3 hidden min-w-0 truncate text-xs text-text-muted lg:col-start-4 lg:row-start-1 lg:block" title={event.actor_subject ?? undefined}>
          {shortIdentifier(event.actor_subject)}
        </span>
        <ChevronDown className="col-start-2 row-start-2 h-4 w-4 text-text-muted transition-transform group-open:rotate-180 lg:col-start-5 lg:row-start-1" aria-hidden="true" />
      </summary>
      <div className="grid gap-3 border-t border-border bg-surface-raised px-3 py-3 text-sm lg:grid-cols-2 lg:px-4">
        <div className="space-y-1">
          <p className="font-medium">Детали события</p>
          <p className="text-xs text-text-secondary">{event.action} · {ROLE_LABELS[event.actor_role ?? ''] ?? event.actor_role ?? 'Роль не указана'}</p>
          <CopyValue label="Субъект" value={event.actor_subject} />
          <CopyValue label="Сущность" value={event.entity_id} />
          <CopyValue label="Request ID" value={event.request_id} />
        </div>
        <div className="min-w-0">
          <p className="mb-2 font-medium">Метаданные</p>
          {event.metadata && Object.keys(event.metadata).length > 0 ? (
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border bg-surface p-3 font-mono text-xs text-text-secondary">{JSON.stringify(event.metadata, null, 2)}</pre>
          ) : <p className="text-xs text-text-muted">Нет дополнительных данных.</p>}
        </div>
      </div>
    </details>
  )
}

export function AuditPage() {
  const [actionChoice, setActionChoice] = useState('')
  const [customAction, setCustomAction] = useState('')
  const [action, setAction] = useState('')
  const [entityType, setEntityType] = useState('')
  const [page, setPage] = useState(0)

  const params = new URLSearchParams()
  if (action) params.set('action', action)
  if (entityType) params.set('entity_type', entityType)
  params.set('limit', String(PAGE_SIZE))
  params.set('offset', String(page * PAGE_SIZE))

  const audit = useQuery({
    queryKey: ['audit-events', action, entityType, page],
    queryFn: () => api.get<{ events: AuditEvent[]; total: number }>(`/api/v1/audit-events?${params.toString()}`),
  })
  const isLoadingPage = audit.isPending || audit.isFetching
  const visiblePage = audit.isSuccess && !isLoadingPage ? audit.data : null

  useEffect(() => {
    if (visiblePage && page > 0 && page * PAGE_SIZE >= visiblePage.total) {
      setPage(Math.max(0, Math.ceil(visiblePage.total / PAGE_SIZE) - 1))
    }
  }, [visiblePage, page])

  const events = visiblePage?.events ?? []
  const total = visiblePage?.total ?? 0
  const hasMore = (page + 1) * PAGE_SIZE < total
  const rangeStart = total ? page * PAGE_SIZE + 1 : 0
  const rangeEnd = page * PAGE_SIZE + events.length

  const applyCustomAction = (event: FormEvent) => {
    event.preventDefault()
    const next = customAction.trim()
    if (!next) return
    setAction(next)
    setPage(0)
  }

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Аудит изменений</h1>
        <p className="mt-1 text-sm text-text-muted">Журнал изменений Admin Panel.</p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <select
          aria-label="Действие"
          value={actionChoice}
          onChange={(event) => {
            const next = event.target.value
            setActionChoice(next)
            if (next !== 'custom') {
              setAction(next)
              setPage(0)
            }
          }}
          className="min-h-10 w-full min-w-0 rounded-md border border-border bg-surface px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent sm:w-auto"
        >
          <option value="">Все действия</option>
          {auditActionOptions.map(([code, label]) => <option key={code} value={code}>{label}</option>)}
          <option value="custom">Точный код…</option>
        </select>
        <select
          aria-label="Тип сущности"
          value={entityType}
          onChange={(event) => { setEntityType(event.target.value); setPage(0) }}
          className="min-h-10 w-full rounded-md border border-border bg-surface px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent sm:w-auto"
        >
          {ENTITY_TYPES.map((type) => <option key={type} value={type}>{type === '' ? 'Все типы сущностей' : ENTITY_LABELS[type]}</option>)}
        </select>
        {actionChoice === 'custom' && (
          <form className="flex w-full flex-wrap items-center gap-2" onSubmit={applyCustomAction}>
            <label className="flex min-h-10 min-w-48 flex-1 items-center gap-2 rounded-md border border-border bg-surface px-3 text-sm focus-within:ring-2 focus-within:ring-accent">
              <Filter className="h-4 w-4 shrink-0 text-text-muted" aria-hidden="true" />
              <span className="sr-only">Точный код действия</span>
              <input
                value={customAction}
                onChange={(event) => setCustomAction(event.target.value)}
                placeholder="Например, branding.published"
                className="min-h-10 min-w-0 flex-1 bg-transparent outline-none"
              />
            </label>
            <button type="submit" disabled={!customAction.trim()} className="min-h-10 rounded-md border border-border px-3 text-sm disabled:opacity-40">
              Применить
            </button>
            {action && <span role="status" className="text-xs text-text-muted">Применён: {action}</span>}
          </form>
        )}
      </div>

      <div className="border-y border-border bg-surface">
        <div className="hidden grid-cols-[130px_minmax(160px,1.3fr)_minmax(120px,1fr)_minmax(100px,1fr)_24px] gap-3 border-b border-border px-4 py-2 text-xs font-medium text-text-muted lg:grid">
          <span>Время</span><span>Действие</span><span>Сущность</span><span>Автор</span><span />
        </div>
        {isLoadingPage && <p role="status" className="px-4 py-5 text-sm text-text-muted">{audit.isPending ? 'Загрузка аудита…' : 'Обновляем журнал…'}</p>}
        {audit.isError && !isLoadingPage && (
          <div role="alert" className="flex flex-wrap items-center gap-2 px-4 py-3 text-sm text-danger">
            <span>Не удалось загрузить журнал.</span>
            <button type="button" className="inline-flex min-h-10 items-center gap-2 rounded-md border border-danger/40 px-3 hover:bg-danger/10 focus-visible:outline-2 focus-visible:outline-accent" onClick={() => void audit.refetch()}>
              <RotateCcw className="h-4 w-4" aria-hidden="true" /> Повторить
            </button>
          </div>
        )}
        {events.map((event) => <AuditRow key={event.id} event={event} />)}
        {visiblePage && events.length === 0 && <p className="px-4 py-6 text-sm text-text-muted">Нет событий по выбранным фильтрам.</p>}
      </div>

      <div className="flex items-center justify-between gap-3">
        <button type="button" onClick={() => setPage((current) => Math.max(0, current - 1))} disabled={page === 0 || audit.isFetching} className="inline-flex min-h-10 items-center gap-1 rounded-md border border-border px-3 text-sm disabled:opacity-40">
          <ChevronLeft className="h-4 w-4" aria-hidden="true" /> Назад
        </button>
        <span className="text-center text-xs text-text-muted">
          {isLoadingPage ? 'Загрузка…' : audit.isError ? 'Число событий недоступно' : `${rangeStart}–${rangeEnd} из ${total}`}
        </span>
        <button type="button" onClick={() => setPage((current) => current + 1)} disabled={!hasMore || audit.isFetching} className="inline-flex min-h-10 items-center gap-1 rounded-md border border-border px-3 text-sm disabled:opacity-40">
          Вперёд <ChevronRight className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>
    </div>
  )
}
