import { useQuery } from '@tanstack/react-query'
import { api } from '@/shared/api/client'
import { useAuth } from '@/shared/auth/auth-context'

interface ReadyReport {
  status: string
  database?: string
  migrations?: number | string
  version?: string
  [key: string]: unknown
}

export function SettingsPage() {
  const { session } = useAuth()
  const ready = useQuery({
    queryKey: ['health-ready'],
    queryFn: () => api.get<ReadyReport>('/health/ready'),
    retry: false,
  })

  const rows: Array<[string, string]> = [
    ['Auth owner', 'Central auth / JWKS (fail-closed)'],
    ['Текущая сессия', session?.email ?? session?.subject ?? '—'],
    ['Panel-роль', session?.panelRole ?? '—'],
    ['Runtime cache', 'ETag + max-age 60 seconds'],
    ['Config delivery', 'Direct API, no CDN/gateway v1'],
    ['Readiness', ready.data ? `${ready.data.status}${ready.data.database ? ` · ${ready.data.database}` : ''}` : ready.isError ? 'недоступен' : 'загрузка...'],
  ]

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Локальные настройки</h1>
        <p className="mt-1 text-sm text-text-muted">Настройки этого приложения; не заменяют central auth или конфигурации подключённых сервисов.</p>
      </div>
      <section className="rounded-lg border border-border bg-surface p-5">
        <h2 className="text-sm font-medium">Состояние интеграции</h2>
        <dl className="mt-4 space-y-3 text-sm">
          {rows.map(([term, value]) => (
            <div key={term} className="flex justify-between gap-3">
              <dt className="text-text-muted">{term}</dt>
              <dd className="text-right">{value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  )
}
