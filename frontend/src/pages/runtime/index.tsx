import { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'

const base = import.meta.env.VITE_API_BASE_URL ?? ''
const endpoint = `${base}/api/v1/runtime/branding`

interface CatalogService {
  key: string
  label: string
  url: string
  contract_version: string
  capabilities: string[]
}

/** Capability descriptions for the catalog card (ru). */
const capabilityHelp: Record<string, string> = {
  'branding.runtime.read': 'UI читает брендинг платформы (цвета, название, логотип)',
  'health.read': 'Admin Panel периодически проверяет /health/live сервиса',
  'integration.status.read': 'Сервис отдаёт собственный статус интеграции для агрегации',
}

export function RuntimePage() {
  const [body, setBody] = useState<string>('')
  const [etag, setEtag] = useState<string>('')
  const [status, setStatus] = useState<string>('Не запрашивалось')
  const [services, setServices] = useState<CatalogService[]>([])
  const [servicesEtag, setServicesEtag] = useState<string>('')
  const [servicesStatus, setServicesStatus] = useState<string>('Не запрашивалось')

  const load = async () => {
    setStatus('Загрузка...')
    const response = await fetch(endpoint, { headers: etag ? { 'If-None-Match': etag } : undefined })
    setStatus(`${response.status} ${response.statusText}`)
    const nextEtag = response.headers.get('etag')
    if (nextEtag) setEtag(nextEtag)
    if (response.status !== 304) setBody(await response.text())
  }

  const loadServices = async () => {
    setServicesStatus('Загрузка...')
    const response = await fetch(`${base}/api/v1/runtime/services`, {
      headers: servicesEtag ? { 'If-None-Match': servicesEtag } : undefined,
    })
    setServicesStatus(`${response.status} ${response.statusText}`)
    const nextEtag = response.headers.get('etag')
    if (nextEtag) setServicesEtag(nextEtag)
    if (response.status === 304) return
    const payload = (await response.json()) as { services?: CatalogService[] }
    setServices(payload.services ?? [])
  }

  useEffect(() => {
    void load()
    void loadServices()
  }, [])

  const allCapabilities = [...new Set(services.flatMap((s) => s.capabilities))].sort()

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Runtime config inspector</h1>
        <p className="mt-1 text-sm text-text-muted">
          Проверка публичного read-only контракта, который потребляют приложения. Defaults остаются в каждом продукте.
        </p>
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">Endpoint</div>
          <code className="mt-2 block break-all text-xs text-text-secondary">/api/v1/runtime/branding</code>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">HTTP status</div>
          <div className="mt-2 font-mono text-sm">{status}</div>
        </div>
        <div className="rounded-lg border border-border bg-surface p-4">
          <div className="text-xs text-text-muted">ETag</div>
          <div className="mt-2 break-all font-mono text-xs">{etag || '—'}</div>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface">
        <div className="flex items-center justify-between border-b border-border p-3">
          <span className="text-sm font-medium">Response</span>
          <button
            onClick={() => void load()}
            className="inline-flex items-center gap-2 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Conditional GET
          </button>
        </div>
        <pre className="max-h-[440px] overflow-auto p-4 text-xs leading-6 text-text-secondary">
          {body || 'Нет опубликованного документа: consumer применит встроенные defaults.'}
        </pre>
      </div>

      <div className="rounded-lg border border-border bg-surface">
        <div className="flex items-center justify-between border-b border-border p-3">
          <span className="text-sm font-medium">Каталог сервисов · capabilities</span>
          <button
            onClick={() => void loadServices()}
            className="inline-flex items-center gap-2 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Conditional GET · {servicesStatus}
          </button>
        </div>
        <div className="divide-y divide-border">
          {services.map((service) => (
            <div key={service.key} className="p-4">
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <span className="text-sm font-medium">{service.label}</span>
                <code className="text-xs text-text-muted">{service.url}</code>
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
          {services.length === 0 && (
            <div className="p-4 text-sm text-text-muted">Каталог пуст или ещё не запрашивался.</div>
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
                  <span className="text-text-muted">{capabilityHelp[capability] ?? '— описание не задано'}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
