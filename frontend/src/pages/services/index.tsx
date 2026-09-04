import { Link } from 'react-router'
import { Plus, Server } from 'lucide-react'
import { useServices } from '@/shared/api/hooks'

export function ServicesPage() {
  const services = useServices()
  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Каталог сервисов</h1>
          <p className="mt-1 text-sm text-text-muted">Реестр интеграций и разрешённых capabilities, не remote CRUD внешних систем.</p>
        </div>
        <button disabled className="inline-flex cursor-not-allowed items-center gap-2 rounded-md border border-border px-3 py-2 text-sm text-text-muted" title="Создание через API добавляется после JWKS/RBAC">
          <Plus className="h-4 w-4" /> Добавить сервис
        </button>
      </div>
      <div className="overflow-hidden rounded-lg border border-border bg-surface">
        <div className="hidden grid-cols-[1.4fr_1fr_1fr_auto] gap-4 border-b border-border px-4 py-3 text-xs font-medium text-text-muted md:grid">
          <span>Сервис</span><span>Команда</span><span>Обновлён</span><span>Состояние</span>
        </div>
        {services.isLoading ? <div className="p-5 text-sm text-text-muted">Загрузка реестра...</div> : null}
        {services.isError ? <div className="p-5 text-sm text-danger">Не удалось загрузить сервисы.</div> : null}
        {services.data?.services.map((service) => (
          <Link key={service.id} to={`/services/${service.service_key}`} className="grid gap-1 border-b border-border px-4 py-4 text-sm transition-colors last:border-0 hover:bg-surface-raised md:grid-cols-[1.4fr_1fr_1fr_auto] md:items-center md:gap-4">
            <span className="flex items-center gap-2 font-medium"><Server className="h-4 w-4 text-accent" />{service.display_name}</span>
            <span className="text-text-secondary">{service.owner_team}</span>
            <span className="text-text-muted">{new Date(service.updated_at).toLocaleString('ru-RU')}</span>
            <span className={service.status === 'active' ? 'text-success' : service.status === 'pending' ? 'text-warning' : 'text-text-muted'}>{service.status}</span>
          </Link>
        ))}
        {services.data?.services.length === 0 ? <div className="p-8 text-center text-sm text-text-muted">Каталог пуст. Первую декларацию сервиса можно создать через API v1.</div> : null}
      </div>
    </div>
  )
}
