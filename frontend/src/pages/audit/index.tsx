import { useState } from 'react'
import { ChevronDown, ChevronLeft, ChevronRight, Copy, Filter } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api } from '@/shared/api/client'
import type { AuditEvent } from '@/shared/api/hooks'
import { auditActionLabel, auditDate, shortIdentifier } from '@/shared/ui/audit-format'

const PAGE_SIZE = 20

const ENTITY_TYPES = ['', 'service', 'branding_revision', 'declaration', 'role_binding'] as const
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
          {event.entity_id ? ` · ${shortIdentifier(event.entity_id)}` : ''}
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

  const events = audit.data?.events ?? []
  const hasMore = events.length === PAGE_SIZE

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-xl font-semibold">Аудит изменений</h1>
        <p className="mt-1 text-sm text-text-muted">Журнал изменений Admin Panel.</p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <label className="flex min-h-10 max-w-sm flex-1 items-center gap-2 rounded-md border border-border bg-surface px-3 text-sm focus-within:ring-2 focus-within:ring-accent">
          <Filter className="h-4 w-4 shrink-0 text-text-muted" aria-hidden="true" />
          <span className="sr-only">Действие</span>
          <input
            value={action}
            onChange={(event) => { setAction(event.target.value); setPage(0) }}
            placeholder="Действие, например branding.published"
            className="min-w-0 flex-1 bg-transparent outline-none"
          />
        </label>
        <select
          aria-label="Тип сущности"
          value={entityType}
          onChange={(event) => { setEntityType(event.target.value); setPage(0) }}
          className="min-h-10 rounded-md border border-border bg-surface px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
          {ENTITY_TYPES.map((type) => <option key={type} value={type}>{type === '' ? 'Все типы сущностей' : ENTITY_LABELS[type]}</option>)}
        </select>
      </div>

      <div className="border-y border-border bg-surface">
        <div className="hidden grid-cols-[130px_minmax(160px,1.3fr)_minmax(120px,1fr)_minmax(100px,1fr)_24px] gap-3 border-b border-border px-4 py-2 text-xs font-medium text-text-muted lg:grid">
          <span>Время</span><span>Действие</span><span>Сущность</span><span>Автор</span><span />
        </div>
        {audit.isPending && <p role="status" className="px-4 py-5 text-sm text-text-muted">Загрузка аудита…</p>}
        {audit.isError && <p role="alert" className="px-4 py-5 text-sm text-danger">Не удалось загрузить журнал. <button type="button" className="underline" onClick={() => void audit.refetch()}>Повторить</button></p>}
        {events.map((event) => <AuditRow key={event.id} event={event} />)}
        {audit.data && events.length === 0 && <p className="px-4 py-6 text-sm text-text-muted">Нет событий по выбранным фильтрам.</p>}
      </div>

      <div className="flex items-center justify-between gap-3">
        <button type="button" onClick={() => setPage((current) => Math.max(0, current - 1))} disabled={page === 0 || audit.isFetching} className="inline-flex min-h-10 items-center gap-1 rounded-md border border-border px-3 text-sm disabled:opacity-40">
          <ChevronLeft className="h-4 w-4" aria-hidden="true" /> Назад
        </button>
        <span className="text-center text-xs text-text-muted">Страница {page + 1} · {events.length} событий</span>
        <button type="button" onClick={() => setPage((current) => current + 1)} disabled={!hasMore || audit.isFetching} className="inline-flex min-h-10 items-center gap-1 rounded-md border border-border px-3 text-sm disabled:opacity-40">
          Вперёд <ChevronRight className="h-4 w-4" aria-hidden="true" />
        </button>
      </div>
    </div>
  )
}
