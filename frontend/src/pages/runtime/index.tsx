import { useEffect, useState } from 'react'
import { ChevronDown, RefreshCw } from 'lucide-react'
import { Button } from '@sdlc/ui/ui'
import { z } from 'zod'

const base = import.meta.env.VITE_API_BASE_URL ?? ''
const endpoint = `${base}/api/v1/runtime/branding`

interface CatalogService {
  key: string
  label: string
  url: string
  ui_url: string | null
  health: 'healthy' | 'unreachable' | 'unknown'
  contract_version: string
  capabilities: string[]
}

const catalogSchema = z.object({
  services: z.array(
    z.object({
      key: z.string().min(1),
      label: z.string(),
      url: z.string().url(),
      ui_url: z.string().url().nullable(),
      health: z.enum(['healthy', 'unreachable', 'unknown']),
      contract_version: z.string(),
      capabilities: z.array(z.string()),
    }),
  ),
})
const brandingSchema = z.object({
  revision: z.number().int(),
  updated_at: z.string(),
  branding: z.object({
    product_name: z.string(),
    product_short_name: z.string(),
    primary_color: z.string(),
    accent_color: z.string(),
  }),
})

type LoadState = 'idle' | 'loading' | 'success' | 'empty' | 'error'

const capabilityHelp: Record<string, string> = {
  'branding.runtime.read': 'UI читает брендинг платформы (цвета, название, логотип)',
  'health.read': 'Admin Panel периодически проверяет /health сервиса',
  'integration.status.read': 'Сервис отдаёт собственный статус интеграции для агрегации',
  'ui.render': 'Сервис имеет пользовательский веб-интерфейс',
}

const healthLabel: Record<CatalogService['health'], string> = {
  healthy: 'Работает',
  unreachable: 'Недоступен',
  unknown: 'Не проверен',
}

export function RuntimePage() {
  const [branding, setBranding] = useState<z.infer<typeof brandingSchema> | null>(null)
  const [etag, setEtag] = useState<string>('')
  const [status, setStatus] = useState<string>('Не запрашивалось')
  const [brandingState, setBrandingState] = useState<LoadState>('idle')
  const [services, setServices] = useState<CatalogService[]>([])
  const [hasServicesSnapshot, setHasServicesSnapshot] = useState(false)
  const [servicesEtag, setServicesEtag] = useState<string>('')
  const [servicesStatus, setServicesStatus] = useState<string>('Не запрашивалось')
  const [servicesState, setServicesState] = useState<LoadState>('idle')

  const load = async () => {
    setBrandingState('loading')
    setStatus('Загрузка...')
    try {
      const response = await fetch(endpoint, {
        headers: etag ? { 'If-None-Match': etag } : undefined,
      })
      if (response.status === 404) {
        setBranding(null)
        setEtag('')
        setStatus('Нет опубликованного документа')
        setBrandingState('empty')
        return
      }
      if (!response.ok && response.status !== 304) throw new Error(`HTTP ${response.status}`)
      setStatus(`${response.status} ${response.statusText}`)
      if (response.status !== 304) {
        const payload = brandingSchema.safeParse(await response.json())
        if (!payload.success) throw new Error('Некорректный ответ брендинга')
        setBranding(payload.data)
        setEtag(response.headers.get('etag') ?? '')
        setBrandingState('success')
      } else {
        if (!branding) {
          setEtag('')
          throw new Error('304 без сохранённого ответа брендинга')
        }
        setBrandingState('success')
      }
    } catch (error) {
      setStatus(error instanceof Error ? error.message : 'Ошибка запроса')
      setBrandingState('error')
    }
  }

  const loadServices = async () => {
    setServicesState('loading')
    setServicesStatus('Загрузка...')
    try {
      const response = await fetch(`${base}/api/v1/runtime/services`, {
        headers: servicesEtag ? { 'If-None-Match': servicesEtag } : undefined,
      })
      setServicesStatus(`${response.status} ${response.statusText}`)
      if (!response.ok && response.status !== 304) throw new Error(`HTTP ${response.status}`)
      if (response.status === 304) {
        if (!hasServicesSnapshot) {
          setServicesEtag('')
          throw new Error('304 без сохранённого ответа каталога')
        }
        setServicesState(services.length > 0 ? 'success' : 'empty')
        return
      }
      const payload = catalogSchema.safeParse(await response.json())
      if (!payload.success) throw new Error('Некорректный ответ каталога')
      const nextServices: CatalogService[] = payload.data.services
      setServices(nextServices)
      setHasServicesSnapshot(true)
      setServicesEtag(response.headers.get('etag') ?? '')
      setServicesState(nextServices.length > 0 ? 'success' : 'empty')
    } catch (error) {
      setServicesStatus(error instanceof Error ? error.message : 'Ошибка запроса')
      setServicesState('error')
    }
  }

  useEffect(() => {
    void load()
    void loadServices()
  }, [])

  const allCapabilities = [...new Set(services.flatMap((s) => s.capabilities))].sort()
  const body = branding ? JSON.stringify(branding, null, 2) : ''

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-xl font-semibold">Проверка runtime-конфигурации</h1>
        <p className="mt-1 text-sm text-text-muted">
          Проверка публичной конфигурации, которую получают приложения.
        </p>
      </div>

      <div className="grid border-y border-border bg-surface sm:grid-cols-3">
        <div className="border-b border-border px-4 py-3 sm:border-b-0 sm:border-r">
          <div className="text-xs text-text-muted">Адрес API</div>
          <code className="mt-1 block break-all text-xs text-text-secondary">
            /api/v1/runtime/branding
          </code>
        </div>
        <div className="border-b border-border px-4 py-3 sm:border-b-0 sm:border-r">
          <div className="text-xs text-text-muted">Статус запроса</div>
          <div className="mt-1 font-mono text-sm">{status}</div>
        </div>
        <div className="min-w-0 px-4 py-3">
          <div className="text-xs text-text-muted">ETag</div>
          <div className="mt-1 break-all font-mono text-xs">{etag || '—'}</div>
        </div>
      </div>

      <section>
        <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
          <h2 className="text-sm font-semibold">Брендинг</h2>
          <Button
            type="button"
            variant="outline"
            className="h-10"
            onClick={() => void load()}
            disabled={brandingState === 'loading'}
          >
            <RefreshCw className="h-4 w-4" />
            Обновить
          </Button>
        </div>
        {brandingState === 'loading' && <p role="status" className="py-4 text-sm text-text-muted">{body ? 'Обновление брендинга. Показан предыдущий ответ.' : 'Загрузка брендинга...'}</p>}
        {brandingState === 'error' && <p role="alert" className="py-4 text-sm text-danger">Не удалось загрузить брендинг: {status}{body ? ' Показан предыдущий ответ.' : ''}</p>}
        {brandingState === 'empty' && <p className="py-4 text-sm text-text-muted">Нет опубликованного документа: приложения применят настройки по умолчанию.</p>}
        {body && <details className="group border-b border-border">
          <summary className="flex min-h-11 cursor-pointer list-none items-center justify-between gap-3 text-sm focus-visible:outline-2 focus-visible:outline-accent [&::-webkit-details-marker]:hidden">
            <span><span className="font-medium">{branding?.branding.product_name}</span><span className="ml-2 text-xs text-text-muted">Ревизия {branding?.revision}</span></span>
            <span className="flex shrink-0 items-center gap-2 text-xs text-text-muted">Ответ API <ChevronDown className="h-4 w-4 transition-transform group-open:rotate-180" aria-hidden="true" /></span>
          </summary>
          <pre aria-label="Ответ runtime-брендинга" className="max-h-80 overflow-auto whitespace-pre-wrap break-all border-t border-border bg-surface p-4 text-xs leading-6 text-text-secondary">{body}</pre>
        </details>}
      </section>

      <section>
        <div className="flex items-center justify-between gap-3 border-b border-border pb-2">
          <div>
            <h2 className="text-sm font-semibold">Каталог сервисов</h2>
            <p className="mt-0.5 text-xs text-text-muted">Источник: Admin Panel runtime · {servicesStatus}</p>
          </div>
          <Button type="button" variant="outline" className="h-10" onClick={() => void loadServices()} disabled={servicesState === 'loading'}>
            <RefreshCw className="h-4 w-4" /> Обновить
          </Button>
        </div>
        <div className="divide-y divide-border border-b border-border">
          {servicesState === 'success' && services.map((service) => (
            <details key={service.key} className="group">
              <summary className="flex min-h-11 cursor-pointer list-none items-center justify-between gap-3 py-2 text-sm hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent [&::-webkit-details-marker]:hidden">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate font-medium">{service.label}</span>
                  <code className="hidden truncate text-xs text-text-muted sm:inline">{service.key}</code>
                </span>
                <span className="flex shrink-0 items-center gap-3">
                  <span className={service.health === 'healthy' ? 'text-xs text-success' : service.health === 'unreachable' ? 'text-xs text-danger' : 'text-xs text-text-muted'}>{healthLabel[service.health]}</span>
                  <ChevronDown className="h-4 w-4 text-text-muted transition-transform group-open:rotate-180" aria-hidden="true" />
                </span>
              </summary>
              <div className="grid gap-2 border-t border-border bg-surface-raised px-3 py-3 text-xs sm:grid-cols-2">
                <div><span className="text-text-muted">API: </span><code className="break-all text-text-secondary">{service.url}</code></div>
                <div><span className="text-text-muted">Веб: </span><code className="break-all text-text-secondary">{service.ui_url ?? 'Нет интерфейса'}</code></div>
                <div className="sm:col-span-2"><span className="text-text-muted">Контракт v{service.contract_version} · Возможности: </span>{service.capabilities.length ? service.capabilities.map((capability, index) => <code key={capability} title={capabilityHelp[capability] ?? 'Возможность интеграции'} className="text-text-secondary">{index ? ', ' : ''}{capability}</code>) : <span className="text-text-muted">не указаны</span>}</div>
              </div>
            </details>
          ))}
          {servicesState === 'loading' && <p role="status" className="py-4 text-sm text-text-muted">Загрузка каталога...</p>}
          {servicesState === 'empty' && <p className="py-4 text-sm text-text-muted">Каталог пуст. Проверьте активные декларации.</p>}
          {servicesState === 'error' && <p role="alert" className="py-4 text-sm text-danger">Не удалось загрузить каталог: {servicesStatus}</p>}
        </div>
        {servicesState === 'success' && allCapabilities.length > 0 && <details className="group border-b border-border">
          <summary className="flex min-h-11 cursor-pointer list-none items-center justify-between gap-2 text-xs text-text-muted focus-visible:outline-2 focus-visible:outline-accent [&::-webkit-details-marker]:hidden">
            Возможности каталога: {allCapabilities.length}
            <ChevronDown className="h-4 w-4 transition-transform group-open:rotate-180" aria-hidden="true" />
          </summary>
          <div className="space-y-1 pb-3">
            {allCapabilities.map((capability) => <div key={capability} className="flex flex-wrap items-baseline gap-2 text-xs"><code className="text-text-secondary">{capability}</code><span className="text-text-muted">{capabilityHelp[capability] ?? 'Описание не задано'}</span></div>)}
          </div>
        </details>}
      </section>
    </div>
  )
}
