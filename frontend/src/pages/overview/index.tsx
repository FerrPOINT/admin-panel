import { Link } from 'react-router'
import { AlertTriangle, CheckCircle2, Clock, XCircle } from 'lucide-react'
import type { ServiceStatus } from '@/shared/api/hooks'
import { useAuditEvents, useBrandingRevisions, useServices } from '@/shared/api/hooks'

function StatusBadge({ status }: { status: ServiceStatus }) {
  const map: Record<ServiceStatus, { icon: typeof CheckCircle2; label: string; cls: string }> = {
    active: { icon: CheckCircle2, label: 'OK', cls: 'text-success' },
    pending: { icon: Clock, label: 'Pending', cls: 'text-warning' },
    disabled: { icon: XCircle, label: 'Disabled', cls: 'text-danger' },
    retired: { icon: AlertTriangle, label: 'Retired', cls: 'text-text-muted' },
  }
  const { icon: Icon, label, cls } = map[status]
  return (
    <span className={`inline-flex items-center gap-1 text-xs font-medium ${cls}`}>
      <Icon className="h-3.5 w-3.5" />
      {label}
    </span>
  )
}

export function OverviewPage() {
  const services = useServices()
  const revisions = useBrandingRevisions()
  const audit = useAuditEvents()

  const published = revisions.data?.revisions.find((r) => r.state === 'published')
  const problems =
    services.data?.services.filter((s) => s.status === 'disabled' || s.status === 'retired') ?? []

  if (services.isError || revisions.isError) {
    return (
      <div className="rounded-lg border border-border bg-surface p-6 text-sm text-text-secondary">
        <div className="mb-1 font-medium text-text-primary">Не удалось загрузить данные</div>
        Повторите попытку позже. Последнее известное состояние недоступно.
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Обзор платформы</h1>
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Брендинг</div>
          <div className="mt-1 text-lg font-semibold">
            {published ? `опубликован v${published.revision}` : 'не опубликован'}
          </div>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Сервисы</div>
          <div className="mt-1 text-lg font-semibold">{services.data?.total ?? '…'}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Проблемы</div>
          <div className="mt-1 text-lg font-semibold">{problems.length}</div>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className="rounded-lg border border-border bg-surface p-4">
          <h2 className="mb-3 text-sm font-medium text-text-secondary">Последние изменения</h2>
          <ul className="space-y-2">
            {(audit.data?.events ?? []).slice(0, 6).map((event) => (
              <li key={event.id} className="flex items-center justify-between gap-2 text-sm">
                <span className="truncate text-text-secondary">
                  {new Date(event.occurred_at).toLocaleTimeString('ru-RU')}{' '}
                  <span className="text-text-muted">{event.action}</span>
                </span>
                <span className="shrink-0 text-xs text-text-muted">
                  {event.actor_subject ?? 'system'}
                </span>
              </li>
            ))}
            {audit.data?.events.length === 0 && (
              <li className="text-sm text-text-muted">Изменений ещё нет</li>
            )}
          </ul>
        </section>

        <section className="rounded-lg border border-border bg-surface p-4">
          <h2 className="mb-3 text-sm font-medium text-text-secondary">Состояние сервисов</h2>
          <ul className="space-y-2">
            {(services.data?.services ?? []).map((service) => (
              <li key={service.id} className="flex items-center justify-between gap-2 text-sm">
                <Link
                  to={`/services/${service.service_key}`}
                  className="truncate text-text-primary hover:underline"
                >
                  {service.display_name}
                </Link>
                <StatusBadge status={service.status} />
              </li>
            ))}
            {services.data?.services.length === 0 && (
              <li className="text-sm text-text-muted">
                Реестр пуст. Добавьте первый сервис в каталог.
              </li>
            )}
          </ul>
        </section>
      </div>
    </div>
  )
}
