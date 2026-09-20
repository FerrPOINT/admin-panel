import { FormEvent, useState } from 'react'
import { Link } from 'react-router'
import { Plus, Search, Server, X } from 'lucide-react'
import { Activity } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Input, defaultServices } from '@sdlc/ui/ui'
import { useServices, useCreateService } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'

const KNOWN_CAPABILITIES = ['health.read', 'integration.status.read', 'branding.runtime.read']
const PAGE_SIZE = 20
const serviceOrder = new Map(defaultServices.map((item, index) => [item.key, index]))
const HEALTH_LABELS: Record<string, string> = {
  healthy: 'Работает',
  unreachable: 'Недоступен',
  unknown: 'Не проверен',
}
const STATUS_LABELS: Record<string, string> = {
  active: 'Активен',
  pending: 'Ожидает',
  disabled: 'Отключён',
  retired: 'Выведен',
}

export function ServicesPage() {
  const services = useServices()
  const { canMutate } = useAuth()
  const create = useCreateService()
  const [open, setOpen] = useState(false)
  const [serviceKey, setServiceKey] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [ownerTeam, setOwnerTeam] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [contractVersion, setContractVersion] = useState('1.0.0')
  const [capabilities, setCapabilities] = useState<string[]>(['health.read'])
  const [search, setSearch] = useState('')
  const [status, setStatus] = useState('all')
  const [page, setPage] = useState(1)
  const registry = services.data?.services ?? []
  const query = search.trim().toLocaleLowerCase('ru')
  const filtered = registry
    .filter((service) => status === 'all' || service.status === status)
    .filter(
      (service) =>
        !query ||
        [service.display_name, service.service_key, service.owner_team].some((value) =>
          value.toLocaleLowerCase('ru').includes(query),
        ),
    )
    .sort(
      (left, right) =>
        (serviceOrder.get(left.service_key) ?? Infinity) -
          (serviceOrder.get(right.service_key) ?? Infinity) ||
        left.service_key.localeCompare(right.service_key),
    )
  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const currentPage = Math.min(page, pageCount)
  const visible = filtered.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)

  function toggleCapability(cap: string) {
    setCapabilities((prev) => (prev.includes(cap) ? prev.filter((c) => c !== cap) : [...prev, cap]))
  }

  function submit(event: FormEvent) {
    event.preventDefault()
    if (create.isPending) return
    create.mutate(
      {
        service_key: serviceKey,
        display_name: displayName,
        owner_team: ownerTeam,
        declaration: {
          declaration_version: 1,
          integration_base_url: baseUrl,
          service_contract_version: contractVersion,
          capabilities,
        },
      },
      {
        onSuccess: () => {
          toast.success('Сервис создан; декларация ожидает одобрения')
          setOpen(false)
          setServiceKey('')
          setDisplayName('')
          setOwnerTeam('')
          setBaseUrl('')
          setContractVersion('1.0.0')
          setCapabilities(['health.read'])
        },
      },
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Каталог сервисов</h1>
        </div>
        {canMutate && (
          <Button
            className="h-10 shrink-0"
            disabled={create.isPending}
            onClick={() => {
              if (!open) create.reset()
              setOpen((value) => !value)
            }}
          >
            {open ? <X className="h-4 w-4" /> : <Plus className="h-4 w-4" />}
            {open ? 'Закрыть форму' : 'Добавить сервис'}
          </Button>
        )}
      </div>

      {open && canMutate && (
        <form
          className="grid gap-4 rounded-lg border border-border bg-surface p-5 md:grid-cols-2"
          onSubmit={submit}
        >
          <label className="text-sm font-medium">
            Ключ сервиса
            <Input
              className="mt-1"
              value={serviceKey}
              onChange={(e) => setServiceKey(e.target.value)}
              placeholder="my-service"
              pattern="[a-z0-9-]{2,40}"
              required
              disabled={create.isPending}
            />
          </label>
          <label className="text-sm font-medium">
            Название
            <Input
              className="mt-1"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              required
              disabled={create.isPending}
            />
          </label>
          <label className="text-sm font-medium">
            Команда-владелец
            <Input
              className="mt-1"
              value={ownerTeam}
              onChange={(e) => setOwnerTeam(e.target.value)}
              required
              disabled={create.isPending}
            />
          </label>
          <label className="text-sm font-medium">
            Базовый URL (HTTPS или localhost)
            <Input
              className="mt-1"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://localhost:7801"
              required
              disabled={create.isPending}
            />
          </label>
          <label className="text-sm font-medium">
            Версия контракта
            <Input
              className="mt-1"
              value={contractVersion}
              onChange={(e) => setContractVersion(e.target.value)}
              required
              disabled={create.isPending}
            />
          </label>
          <fieldset className="text-sm font-medium">
            <legend>Возможности интеграции</legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {KNOWN_CAPABILITIES.map((cap) => (
                <label
                  key={cap}
                  className={`inline-flex min-h-10 cursor-pointer items-center rounded-md border px-3 font-mono text-xs focus-within:ring-2 focus-within:ring-accent ${capabilities.includes(cap) ? 'border-accent bg-accent/10 text-accent' : 'border-border text-text-secondary'}`}
                >
                  <input
                    type="checkbox"
                    aria-label={cap}
                    className="sr-only"
                    checked={capabilities.includes(cap)}
                    onChange={() => toggleCapability(cap)}
                    disabled={create.isPending}
                  />
                  {cap}
                </label>
              ))}
            </div>
          </fieldset>
          <div className="flex items-end gap-2 md:col-span-2">
            <Button type="submit" className="h-10" disabled={create.isPending}>
              {create.isPending ? 'Создаём...' : 'Создать сервис'}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="h-10"
              disabled={create.isPending}
              onClick={() => setOpen(false)}
            >
              Отмена
            </Button>
          </div>
          {create.isError && (
            <p role="alert" className="text-sm text-danger md:col-span-2">
              {create.error instanceof Error ? create.error.message : 'Не удалось создать сервис'}
            </p>
          )}
        </form>
      )}

      {!services.isLoading && !services.isError && registry.length > 0 && (
        <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_12rem]">
          <div className="relative min-w-0">
            <Search
              aria-hidden="true"
              className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
            />
            <Input
              type="search"
              aria-label="Найти сервис"
              placeholder="Название, ключ или команда"
              className="min-h-10 pl-9"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value)
                setPage(1)
              }}
            />
          </div>
          <select
            aria-label="Состояние сервиса"
            className="min-h-10 rounded-md border border-border bg-surface px-3 text-sm text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
            value={status}
            onChange={(event) => {
              setStatus(event.target.value)
              setPage(1)
            }}
          >
            <option value="all">Все состояния</option>
            {Object.entries(STATUS_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </div>
      )}

      <div className="overflow-hidden rounded-lg border border-border bg-surface">
        <div className="hidden grid-cols-[1.4fr_1fr_1fr_auto_auto] gap-4 border-b border-border px-4 py-3 text-xs font-medium text-text-muted lg:grid">
          <span>Сервис</span>
          <span>Команда</span>
          <span>Обновлён</span>
          <span>Доступность</span>
          <span>Состояние</span>
        </div>
        {services.isLoading && (
          <div className="p-5 text-sm text-text-muted">Загрузка реестра...</div>
        )}
        {services.isError && (
          <div role="alert" className="flex flex-wrap items-center gap-3 p-5 text-sm text-danger">
            Не удалось загрузить сервисы.{' '}
            <Button
              type="button"
              variant="outline"
              className="h-10"
              onClick={() => void services.refetch()}
            >
              Повторить
            </Button>
          </div>
        )}
        {!services.isLoading && !services.isError && registry.length === 0 && (
          <div className="p-8 text-center text-sm text-text-muted">Каталог пуст.</div>
        )}
        {!services.isLoading &&
          !services.isError &&
          registry.length > 0 &&
          filtered.length === 0 && (
            <div className="p-8 text-center text-sm text-text-muted">
              По заданным условиям сервисы не найдены.
            </div>
          )}
        {!services.isLoading &&
          !services.isError &&
          visible.map((service) => (
            <Link
              key={service.id}
              to={`/services/${service.service_key}`}
              className="grid min-h-14 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 border-b border-border px-4 py-2 text-sm transition-colors last:border-0 hover:bg-surface-raised lg:grid-cols-[1.4fr_1fr_1fr_auto_auto] lg:gap-4"
            >
              <span className="col-start-1 row-start-1 flex min-w-0 items-center gap-2 font-medium lg:col-auto lg:row-auto">
                <Server className="h-4 w-4 shrink-0 text-accent" />
                <span className="min-w-0 break-words">{service.display_name}</span>
              </span>
              <span className="col-start-1 row-start-2 min-w-0 break-words text-xs text-text-secondary lg:col-auto lg:row-auto lg:text-sm">
                {service.owner_team}
              </span>
              <span className="hidden text-text-muted lg:block">
                {new Date(service.updated_at).toLocaleString('ru-RU')}
              </span>
              <span
                className={`col-start-2 row-start-2 inline-flex items-center gap-1.5 text-xs lg:col-auto lg:row-auto lg:text-sm ${service.health_status === 'healthy' ? 'text-success' : service.health_status === 'unreachable' ? 'text-danger' : 'text-text-muted'}`}
                title={service.health_detail ?? 'нет данных проверки'}
              >
                <Activity className="h-3.5 w-3.5" />
                {HEALTH_LABELS[service.health_status ?? 'unknown'] ?? 'Не проверен'}
              </span>
              <span
                className={`col-start-2 row-start-1 text-xs lg:col-auto lg:row-auto lg:text-sm ${service.status === 'active' ? 'text-success' : service.status === 'pending' ? 'text-warning' : 'text-text-muted'}`}
              >
                {STATUS_LABELS[service.status] ?? service.status}
              </span>
            </Link>
          ))}
      </div>
      {!services.isLoading && !services.isError && filtered.length > 0 && (
        <div className="flex flex-wrap items-center justify-between gap-3 text-sm text-text-secondary">
          <p role="status">
            Показано {visible.length} из {filtered.length} сервисов
          </p>
          {pageCount > 1 && (
            <nav aria-label="Страницы каталога" className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                className="h-10"
                disabled={currentPage === 1}
                onClick={() => setPage(currentPage - 1)}
              >
                Назад
              </Button>
              <span className="tabular-nums">
                {currentPage} / {pageCount}
              </span>
              <Button
                type="button"
                variant="outline"
                className="h-10"
                disabled={currentPage === pageCount}
                onClick={() => setPage(currentPage + 1)}
              >
                Далее
              </Button>
            </nav>
          )}
        </div>
      )}
    </div>
  )
}
