import { Link, useParams } from 'react-router'
import { ArrowLeft, CheckCircle2, ShieldCheck } from 'lucide-react'
import { useService } from '@/shared/api/hooks'

export function ServiceDetailPage() {
  const { serviceKey = '' } = useParams()
  const service = useService(serviceKey)
  if (service.isLoading) return <div className="text-sm text-text-muted">Загрузка карточки...</div>
  if (service.isError || !service.data) return <div className="text-sm text-danger">Сервис не найден или API недоступен.</div>
  const { service: entry, declarations } = service.data
  const active = declarations.find((d) => d.id === entry.active_declaration_id)
  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <Link to="/services" className="inline-flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary"><ArrowLeft className="h-4 w-4" /> Каталог сервисов</Link>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div><h1 className="text-xl font-semibold">{entry.display_name}</h1><p className="mt-1 text-sm text-text-muted">Ключ: {entry.service_key} · команда {entry.owner_team}</p></div>
        <span className="rounded-full border border-border px-3 py-1 text-sm text-text-secondary">{entry.status}</span>
      </div>
      <section className="rounded-lg border border-border bg-surface p-5">
        <h2 className="mb-4 flex items-center gap-2 text-sm font-medium"><ShieldCheck className="h-4 w-4 text-accent" /> Активный integration contract</h2>
        {active ? <div className="space-y-4 text-sm"><div><div className="text-xs text-text-muted">Base URL</div><code className="mt-1 block break-all text-text-secondary">{active.integration_base_url}</code></div><div><div className="text-xs text-text-muted">Capabilities</div><div className="mt-2 flex flex-wrap gap-2">{active.capabilities.map((cap) => <span key={cap} className="rounded bg-surface-raised px-2 py-1 font-mono text-xs">{cap}</span>)}</div></div><div className="flex items-center gap-2 text-xs text-success"><CheckCircle2 className="h-4 w-4" /> Contract v{active.service_contract_version}, approved</div></div> : <p className="text-sm text-text-muted">Активная декларация ещё не одобрена. До approval Admin Panel не выполняет внешние проверки.</p>}
      </section>
      <section className="rounded-lg border border-border bg-surface p-5"><h2 className="mb-4 text-sm font-medium">История деклараций</h2><div className="space-y-3">{declarations.map((declaration) => <div key={declaration.id} className="rounded-md border border-border p-3 text-sm"><div className="flex justify-between gap-3"><span>v{declaration.service_contract_version}</span><span className="text-text-muted">{declaration.approval_status}</span></div><div className="mt-1 break-all font-mono text-xs text-text-muted">{declaration.integration_base_url}</div></div>)}</div></section>
    </div>
  )
}
