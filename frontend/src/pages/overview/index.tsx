import { Link } from 'react-router'
import { AlertTriangle, ArrowRight, CheckCircle2, Clock, Server, XCircle } from 'lucide-react'
import { defaultServices } from '@sdlc/ui/ui'
import type { RegistryEntry, ServiceStatus } from '@/shared/api/hooks'
import { useAuditEvents, useBrandingRevisions, useServices } from '@/shared/api/hooks'
import { auditActionLabel, auditDate } from '@/shared/ui/audit-format'

const serviceOrder = new Map(defaultServices.map((item, index) => [item.key, index]))

function StatusBadge({ status }: { status: ServiceStatus }) {
  const map: Record<ServiceStatus, { icon: typeof CheckCircle2; label: string; cls: string }> = {
    active: { icon: CheckCircle2, label: 'Активен', cls: 'text-success' },
    pending: { icon: Clock, label: 'Ожидает', cls: 'text-warning' },
    disabled: { icon: XCircle, label: 'Отключён', cls: 'text-danger' },
    retired: { icon: AlertTriangle, label: 'Выведен', cls: 'text-text-muted' },
  }
  const { icon: Icon, label, cls } = map[status]
  return (
    <span className={`inline-flex items-center gap-1.5 text-xs font-medium ${cls}`}>
      <Icon className="h-4 w-4" aria-hidden="true" />
      {label}
    </span>
  )
}

function serviceIssue(service: RegistryEntry) {
  if (service.health_status === 'unreachable') return 'Недоступен'
  if (service.status === 'pending') return 'Ожидает одобрения'
  if (service.status === 'disabled') return 'Отключён'
  if (service.status === 'retired') return 'Выведен из эксплуатации'
  return null
}

export function OverviewPage() {
  const services = useServices()
  const revisions = useBrandingRevisions()
  const audit = useAuditEvents()

  const registry = [...(services.data?.services ?? [])].sort((left, right) =>
    (serviceOrder.get(left.service_key) ?? Infinity) - (serviceOrder.get(right.service_key) ?? Infinity)
    || left.service_key.localeCompare(right.service_key),
  )
  const attention = registry.filter((service) => serviceIssue(service))
  const published = revisions.data?.revisions.find((revision) => revision.state === 'published')
  const events = audit.data?.events.slice(0, 6) ?? []

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold">Обзор платформы</h1>
        <Link to="/services" className="inline-flex min-h-10 items-center gap-2 text-sm font-medium text-accent hover:underline">
          Каталог сервисов <ArrowRight className="h-4 w-4" aria-hidden="true" />
        </Link>
      </div>

      <div className="grid grid-cols-2 border-y border-border bg-surface sm:grid-cols-4">
        <div className="min-w-0 border-b border-r border-border px-4 py-3 sm:border-b-0">
          <div className="text-xs text-text-muted">Сервисов</div>
          <div className="mt-1 text-lg font-semibold">{services.isPending ? '…' : services.isError ? '—' : services.data?.total}</div>
        </div>
        <div className="min-w-0 border-b border-border px-4 py-3 sm:border-b-0 sm:border-r">
          <div className="text-xs text-text-muted">Требуют внимания</div>
          <div className={`mt-1 text-lg font-semibold ${attention.length ? 'text-warning' : ''}`}>
            {services.isPending ? '…' : services.isError ? '—' : attention.length}
          </div>
        </div>
        <div className="min-w-0 border-r border-border px-4 py-3">
          <div className="text-xs text-text-muted">Работают</div>
          <div className="mt-1 text-lg font-semibold">
            {services.isPending ? '…' : services.isError ? '—' : registry.filter((service) => service.status === 'active' && service.health_status !== 'unreachable').length}
          </div>
        </div>
        <div className="min-w-0 px-4 py-3">
          <div className="text-xs text-text-muted">Брендинг</div>
          <div className="mt-1 truncate text-sm font-semibold">
            {revisions.isPending ? 'Загрузка…' : revisions.isError ? 'Недоступен' : published ? `Версия ${published.revision}` : 'Не опубликован'}
          </div>
        </div>
      </div>

      <div className="grid gap-5 xl:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)]">
        <section aria-labelledby="services-title" className="min-w-0">
          <div className="mb-2 flex items-center justify-between gap-2">
            <h2 id="services-title" className="text-sm font-semibold">Сервисы</h2>
            <span className="text-xs text-text-muted">Состояние каталога</span>
          </div>
          <div className="divide-y divide-border border-y border-border">
            {services.isPending && <p role="status" className="py-4 text-sm text-text-muted">Загрузка сервисов…</p>}
            {services.isError && <p role="alert" className="py-4 text-sm text-danger">Не удалось загрузить сервисы. <button type="button" className="underline" onClick={() => void services.refetch()}>Повторить</button></p>}
            {registry.map((service) => (
              <Link key={service.id} to={`/services/${service.service_key}`} className="flex min-h-12 items-center justify-between gap-3 py-2 text-sm hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent">
                <span className="flex min-w-0 items-center gap-2">
                  <Server className="h-4 w-4 shrink-0 text-text-muted" aria-hidden="true" />
                  <span className="truncate font-medium" title={service.display_name}>{service.display_name}</span>
                </span>
                <span className="flex shrink-0 items-center gap-3">
                  {service.health_status === 'unreachable' && <span className="text-xs text-danger">Недоступен</span>}
                  <StatusBadge status={service.status} />
                </span>
              </Link>
            ))}
            {services.data?.services.length === 0 && <p className="py-5 text-sm text-text-muted">В каталоге пока нет сервисов.</p>}
          </div>
        </section>

        <div className="min-w-0 space-y-5">
          <section aria-labelledby="attention-title">
            <div className="mb-2 flex items-center justify-between gap-2">
              <h2 id="attention-title" className="text-sm font-semibold">Требуют внимания</h2>
              <span className="text-xs text-text-muted">{services.isError ? '—' : attention.length}</span>
            </div>
            <div className="divide-y divide-border border-y border-border">
              {attention.slice(0, 5).map((service) => (
                <Link key={service.id} to={`/services/${service.service_key}`} className="flex min-h-11 items-center justify-between gap-3 py-2 text-sm hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent">
                  <span className="min-w-0 truncate">{service.display_name}</span>
                  <span className="shrink-0 text-xs text-warning">{serviceIssue(service)}</span>
                </Link>
              ))}
              {services.data && attention.length === 0 && <p className="py-4 text-sm text-text-muted">Сервисов с проблемным состоянием нет.</p>}
              {services.isPending && <p role="status" className="py-4 text-sm text-text-muted">Проверяем состояние…</p>}
              {services.isError && <p className="py-4 text-sm text-text-muted">Состояние недоступно.</p>}
            </div>
          </section>

          <section aria-labelledby="audit-title">
            <div className="mb-2 flex items-center justify-between gap-2">
              <h2 id="audit-title" className="text-sm font-semibold">Последние изменения</h2>
              <Link to="/audit" className="inline-flex min-h-10 items-center text-xs font-medium text-accent hover:underline">Весь журнал</Link>
            </div>
            <div className="divide-y divide-border border-y border-border">
              {audit.isPending && <p role="status" className="py-4 text-sm text-text-muted">Загрузка изменений…</p>}
              {audit.isError && <p role="alert" className="py-4 text-sm text-danger">Не удалось загрузить изменения. <button type="button" className="underline" onClick={() => void audit.refetch()}>Повторить</button></p>}
              {events.map((event) => (
                <div key={event.id} className="flex min-h-11 items-center justify-between gap-3 py-2 text-sm">
                  <span className="min-w-0 truncate" title={event.action}>{auditActionLabel(event.action)}</span>
                  <time className="shrink-0 text-xs text-text-muted" dateTime={event.occurred_at}>{auditDate(event.occurred_at)}</time>
                </div>
              ))}
              {audit.data?.events.length === 0 && <p className="py-4 text-sm text-text-muted">Изменений пока нет.</p>}
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}
