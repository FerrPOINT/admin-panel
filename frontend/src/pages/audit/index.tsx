import { useState } from 'react'
import { Filter, History } from 'lucide-react'
import { useAuditEvents } from '@/shared/api/hooks'

export function AuditPage() {
  const [action, setAction] = useState('')
  const audit = useAuditEvents(action || undefined)
  return <div className="space-y-6"><div><h1 className="text-xl font-semibold">Аудит изменений</h1><p className="mt-1 text-sm text-text-muted">Append-only журнал событий Admin Panel. Записи не редактируются и не удаляются из UI.</p></div>
    <label className="flex max-w-sm items-center gap-2 rounded-md border border-border bg-surface px-3 py-2 text-sm"><Filter className="h-4 w-4 text-text-muted" /><input value={action} onChange={(e) => setAction(e.target.value)} placeholder="Фильтр action, например branding.published" className="min-w-0 flex-1 bg-transparent outline-none" /></label>
    <div className="space-y-2">{audit.isLoading ? <div className="text-sm text-text-muted">Загрузка аудита...</div> : null}{audit.data?.events.map((event) => <article key={event.id} className="rounded-lg border border-border bg-surface p-4"><div className="flex flex-wrap items-center justify-between gap-2"><span className="inline-flex items-center gap-2 font-mono text-sm"><History className="h-4 w-4 text-accent" />{event.action}</span><time className="text-xs text-text-muted">{new Date(event.occurred_at).toLocaleString('ru-RU')}</time></div><div className="mt-2 text-sm text-text-secondary">{event.entity_type} · {event.actor_subject ?? 'system'}</div><div className="mt-2 font-mono text-xs text-text-muted">request {event.request_id}</div></article>)}{audit.data?.events.length === 0 ? <div className="rounded-lg border border-dashed border-border p-8 text-center text-sm text-text-muted">Аудит ещё пуст.</div> : null}</div>
  </div>
}
