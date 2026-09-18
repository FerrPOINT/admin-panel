import { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
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

/** Capability descriptions for the catalog card (ru). */
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
  const [body, setBody] = useState<string>('')
  const [etag, setEtag] = useState<string>('')
  const [status, setStatus] = useState<string>('Не запрашивалось')
  const [brandingState, setBrandingState] = useState<LoadState>('idle')
  const [services, setServices] = useState<CatalogService[]>([])
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
        setBody('')
        setEtag('')
        setStatus('Нет опубликованного документа')
        setBrandingState('empty')
        return
      }
      if (!response.ok && response.status !== 304) throw new Error(`HTTP ${response.status}`)
      setStatus(`${response.status} ${response.statusText}`)
      const nextEtag = response.headers.get('etag')
      if (nextEtag) setEtag(nextEtag)
      if (response.status !== 304) {
        const payload = brandingSchema.safeParse(await response.json())
        if (!payload.success) throw new Error('Некорректный ответ брендинга')
        setBody(JSON.stringify(payload.data, null, 2))
        setBrandingState('success')
      } else {
        setBrandingState(body ? 'success' : 'empty')
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
      const nextEtag = response.headers.get('etag')
      if (nextEtag) setServicesEtag(nextEtag)
      if (response.status === 304) {
        setServicesState(services.length > 0 ? 'success' : 'empty')
        return
      }
      const payload = catalogSchema.safeParse(await response.json())
      if (!payload.success) throw new Error('Некорректный ответ каталога')
      const nextServices: CatalogService[] = payload.data.services
      setServices(nextServices)
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

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Проверка runtime-конфигурации</h1>
        <p className="mt-1 text-sm text-text-muted">
          Проверка публичного read-only контракта, который потребляют приложения. Defaults остаются
          в каждом продукте.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Адрес API</div>
          <code className="mt-2 block break-all text-xs text-text-secondary">
            /api/v1/runtime/branding
          </code>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Статус запроса</div>
          <div className="mt-2 font-mono text-sm">{status}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">ETag</div>
          <div className="mt-2 break-all font-mono text-xs">{etag || '—'}</div>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface">
        <div className="flex items-center justify-between border-b border-border p-3">
          <span className="text-sm font-medium">Брендинг</span>
          <button
            onClick={() => void load()}
            disabled={brandingState === 'loading'}
            className="inline-flex items-center gap-2 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Обновить
          </button>
        </div>
        <pre
          tabIndex={0}
          aria-label="Ответ runtime-брендинга"
          className="max-h-[440px] overflow-auto p-4 text-xs leading-6 text-text-secondary"
        >
          {brandingState === 'loading' && !body
            ? 'Загрузка брендинга...'
            : brandingState === 'error'
              ? `Не удалось загрузить брендинг: ${status}${body ? '\nПоказан предыдущий ответ.' : ''}`
              : body ||
                'Нет опубликованного документа: приложения применят настройки по умолчанию.'}
        </pre>
      </div>

      <div className="rounded-lg border border-border bg-surface">
        <div className="flex items-center justify-between border-b border-border p-3">
          <div>
            <div className="text-sm font-medium">Каталог сервисов</div>
            <div className="mt-0.5 text-xs text-text-muted">
              Источник: Admin Panel runtime · {servicesStatus}
            </div>
          </div>
          <button
            onClick={() => void loadServices()}
            disabled={servicesState === 'loading'}
            className="inline-flex items-center gap-2 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Обновить
          </button>
        </div>
        <div className="divide-y divide-border">
          {services.map((service) => (
            <div key={service.key} className="p-4">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-2">
                  <span className="text-sm font-medium">{service.label}</span>
                  <span className="rounded border border-border px-1.5 py-0.5 text-[10px] uppercase text-text-muted">
                    {service.ui_url ? 'UI' : 'API'}
                  </span>
                  <span className="text-xs text-text-muted">{healthLabel[service.health]}</span>
                </div>
                <code className="max-w-full break-all text-xs text-text-muted">{service.key}</code>
              </div>
              <div className="mt-2 grid gap-1 text-xs sm:grid-cols-2">
                <div>
                  <span className="text-text-muted">API: </span>
                  <code className="break-all text-text-secondary">{service.url}</code>
                </div>
                <div>
                  <span className="text-text-muted">UI: </span>
                  <code className="break-all text-text-secondary">{service.ui_url ?? '—'}</code>
                </div>
              </div>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {service.capabilities.map((capability) => (
                  <span
                    key={capability}
                    title={capabilityHelp[capability] ?? 'декларированная capability'}
                    className="rounded-full border border-border bg-surface-raised px-2 py-0.5 font-mono text-[11px] text-text-secondary"
                  >
                    {capability}
                  </span>
                ))}
              </div>
            </div>
          ))}
          {servicesState === 'loading' && (
            <div className="p-4 text-sm text-text-muted">Загрузка каталога...</div>
          )}
          {servicesState === 'empty' && (
            <div className="p-4 text-sm text-text-muted">
              Каталог пуст. Проверьте bootstrap-конфигурацию и активные declarations.
            </div>
          )}
          {servicesState === 'error' && (
            <div className="p-4 text-sm text-danger">
              Не удалось загрузить каталог: {servicesStatus}
            </div>
          )}
        </div>
        {allCapabilities.length > 0 && (
          <div className="border-t border-border p-4">
            <div className="text-xs font-medium text-text-muted">
              Всего capabilities в каталоге: {allCapabilities.length}
            </div>
            <div className="mt-2 space-y-1">
              {allCapabilities.map((capability) => (
                <div key={capability} className="flex items-baseline gap-2 text-xs">
                  <code className="text-text-secondary">{capability}</code>
                  <span className="text-text-muted">
                    {capabilityHelp[capability] ?? '— описание не задано'}
                  </span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
