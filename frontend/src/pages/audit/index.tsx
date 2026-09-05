import { useState } from 'react'
import { Filter, History, ChevronLeft, ChevronRight } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/shared/api/client'
import type { AuditEvent } from '@/shared/api/hooks'

const PAGE_SIZE = 20

const ENTITY_TYPES = ['', 'service', 'branding_revision', 'declaration', 'role_binding'] as const

const ROLE_BADGE: Record<string, string> = {
  platform_admin: 'border-success/40 bg-success/10 text-success',
  platform_operator: 'border-accent/40 bg-accent/10 text-accent',
  platform_viewer: 'border-border bg-surface-raised text-text-muted',
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
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Аудит изменений</h1>
        <p className="mt-1 text-sm text-text-muted">
          Append-only журнал событий Admin Panel. Записи не редактируются и не удаляются из UI.
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <label className="flex max-w-xs flex-1 items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm">
          <Filter className="h-4 w-4 text-text-muted" />
          <input
            value={action}
            onChange={(e) => {
              setAction(e.target.value)
              setPage(0)
            }}
            placeholder="action, например branding.published"
            className="min-w-0 flex-1 bg-transparent outline-none"
          />
        </label>
        <select
          value={entityType}
          onChange={(e) => {
            setEntityType(e.target.value)
            setPage(0)
          }}
          className="rounded-md border border-border bg-surface px-3 py-2 text-sm outline-none"
        >
          {ENTITY_TYPES.map((type) => (
            <option key={type} value={type}>
              {type === '' ? 'Все типы сущностей' : type}
            </option>
          ))}
        </select>
      </div>

      <div className="space-y-2">
        {audit.isLoading ? <div className="text-sm text-text-muted">Загрузка аудита...</div> : null}
        {events.map((event) => (
          <article key={event.id} className="rounded-lg border border-border bg-surface p-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="inline-flex items-center gap-2 font-mono text-sm">
                <History className="h-4 w-4 text-accent" />
                {event.action}
              </span>
              <div className="flex items-center gap-2">
                {event.actor_role ? (
                  <span
                    className={`rounded-full border px-2 py-0.5 text-[11px] font-medium ${ROLE_BADGE[event.actor_role] ?? ROLE_BADGE.platform_viewer}`}
                  >
                    {event.actor_role}
                  </span>
                ) : null}
                <time className="text-xs text-text-muted">{new Date(event.occurred_at).toLocaleString('ru-RU')}</time>
              </div>
            </div>
            <div className="mt-2 text-sm text-text-secondary">
              {event.entity_type} · {event.actor_subject ?? 'system'}
            </div>
            {event.metadata && Object.keys(event.metadata).length > 0 ? (
              <div className="mt-2 font-mono text-xs text-text-muted">{JSON.stringify(event.metadata)}</div>
            ) : null}
            <div className="mt-2 font-mono text-xs text-text-muted">request {event.request_id}</div>
          </article>
        ))}
        {audit.data && events.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border p-8 text-center text-sm text-text-muted">
            Нет событий по выбранным фильтрам.
          </div>
        ) : null}
      </div>

      <div className="flex items-center justify-between">
        <button
          onClick={() => setPage((p) => Math.max(0, p - 1))}
          disabled={page === 0 || audit.isFetching}
          className="inline-flex items-center gap-1 rounded-md border border-border px-3 py-1.5 text-sm disabled:opacity-40"
        >
          <ChevronLeft className="h-4 w-4" /> Назад
        </button>
        <span className="text-xs text-text-muted">
          Страница {page + 1} · показано {events.length}
        </span>
        <button
          onClick={() => setPage((p) => p + 1)}
          disabled={!hasMore || audit.isFetching}
          className="inline-flex items-center gap-1 rounded-md border border-border px-3 py-1.5 text-sm disabled:opacity-40"
        >
          Вперёд <ChevronRight className="h-4 w-4" />
        </button>
      </div>
    </div>
  )
}
