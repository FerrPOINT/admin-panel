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
    ['Источник входа', 'Центральная авторизация / JWKS'],
    ['Текущая сессия', session?.email ?? session?.subject ?? '—'],
    ['Кеш runtime', 'ETag · 60 секунд'],
    ['Доставка конфигурации', 'Прямой API'],
    [
      'Готовность',
      ready.data
        ? `${ready.data.status}${ready.data.database ? ` · ${ready.data.database}` : ''}`
        : ready.isError
          ? 'Недоступна'
          : 'Загрузка…',
    ],
  ]

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Локальные настройки</h1>
        <p className="mt-1 text-sm text-text-muted">
          Настройки этого приложения; не заменяют central auth или конфигурации подключённых
          сервисов.
        </p>
      </div>
      <section className="rounded-lg border border-border bg-surface p-5">
        <h2 className="text-sm font-medium">Состояние интеграции</h2>
        <dl className="mt-4 space-y-3 text-sm">
          {rows.map(([term, value]) => (
            <div
              key={term}
              className="grid gap-1 border-b border-border pb-3 last:border-b-0 last:pb-0 sm:grid-cols-[minmax(140px,220px)_minmax(0,1fr)] sm:gap-4"
            >
              <dt className="text-text-muted">{term}</dt>
              <dd className="min-w-0 break-words">{value}</dd>
            </div>
          ))}
        </dl>
      </section>
    </div>
  )
}
