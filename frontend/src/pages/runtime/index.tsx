import { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'

const endpoint = `${import.meta.env.VITE_API_BASE_URL ?? ''}/api/v1/runtime/branding`

export function RuntimePage() {
  const [body, setBody] = useState<string>('')
  const [etag, setEtag] = useState<string>('')
  const [status, setStatus] = useState<string>('Не запрашивалось')
  const load = async () => {
    setStatus('Загрузка...')
    const response = await fetch(endpoint, { headers: etag ? { 'If-None-Match': etag } : undefined })
    setStatus(`${response.status} ${response.statusText}`)
    const nextEtag = response.headers.get('etag')
    if (nextEtag) setEtag(nextEtag)
    if (response.status !== 304) setBody(await response.text())
  }
  useEffect(() => { void load() }, [])
  return <div className="mx-auto max-w-4xl space-y-6"><div><h1 className="text-xl font-semibold">Runtime config inspector</h1><p className="mt-1 text-sm text-text-muted">Проверка публичного read-only контракта, который потребляют приложения. Defaults остаются в каждом продукте.</p></div>
    <div className="grid gap-4 sm:grid-cols-3"><div className="rounded-lg border border-border bg-surface p-4"><div className="text-xs text-text-muted">Endpoint</div><code className="mt-2 block break-all text-xs text-text-secondary">/api/v1/runtime/branding</code></div><div className="rounded-lg border border-border bg-surface p-4"><div className="text-xs text-text-muted">HTTP status</div><div className="mt-2 font-mono text-sm">{status}</div></div><div className="rounded-lg border border-border bg-surface p-4"><div className="text-xs text-text-muted">ETag</div><div className="mt-2 break-all font-mono text-xs">{etag || '—'}</div></div></div>
    <div className="rounded-lg border border-border bg-surface"><div className="flex items-center justify-between border-b border-border p-3"><span className="text-sm font-medium">Response</span><button onClick={() => void load()} className="inline-flex items-center gap-2 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"><RefreshCw className="h-3.5 w-3.5" />Conditional GET</button></div><pre className="max-h-[440px] overflow-auto p-4 text-xs leading-6 text-text-secondary">{body || 'Нет опубликованного документа: consumer применит встроенные defaults.'}</pre></div>
  </div>
}
