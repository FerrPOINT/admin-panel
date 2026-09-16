import { FormEvent, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, CheckCircle2, ShieldCheck } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Input } from '@sdlc/ui/ui'
import { useApproveService, useChangeServiceStatus, usePatchService, useService } from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'

const KNOWN_CAPABILITIES = ['health.read', 'integration.status.read', 'branding.runtime.read']

export function ServiceDetailPage() {
  const { serviceKey = '' } = useParams()
  const service = useService(serviceKey)
  const { canMutate } = useAuth()
  const approve = useApproveService(serviceKey)
  const patch = usePatchService(serviceKey)
  const changeStatus = useChangeServiceStatus(serviceKey)
  const [newBaseUrl, setNewBaseUrl] = useState('')
  const [newCaps, setNewCaps] = useState<string[]>([])

  if (service.isLoading) return <div className="text-sm text-text-muted">Загрузка карточки...</div>
  if (service.isError || !service.data) return <div className="text-sm text-danger">Сервис не найден или API недоступен.</div>
  const { service: entry, declarations } = service.data
  const active = declarations.find((d) => d.id === entry.active_declaration_id)
  const pending = declarations.find((d) => d.approval_status === 'pending')
  const version = entry.version

  function submitDeclaration(event: FormEvent) {
    event.preventDefault()
    const nextVersion = declarations.reduce((max, d) => Math.max(max, d.declaration_version), 0) + 1
    patch.mutate(
      {
        version,
        body: {
          declaration: {
            declaration_version: nextVersion,
            integration_base_url: newBaseUrl,
            service_contract_version: active?.service_contract_version ?? '1.0.0',
            capabilities: newCaps,
          },
        },
      },
      {
        onSuccess: () => {
          toast.success('Новая декларация отправлена; ожидает одобрения')
          setNewBaseUrl('')
          setNewCaps([])
        },
        onError: (error) => toast.error(error instanceof Error ? error.message : 'Не удалось отправить'),
      },
    )
  }

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <Link to="/services" className="inline-flex items-center gap-2 text-sm text-text-secondary hover:text-text-primary"><ArrowLeft className="h-4 w-4" /> Каталог сервисов</Link>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div><h1 className="text-xl font-semibold">{entry.display_name}</h1><p className="mt-1 text-sm text-text-muted">Ключ: {entry.service_key} · команда {entry.owner_team} · v{entry.version}</p></div>
        <span className="rounded-full border border-border px-3 py-1 text-sm text-text-secondary">{entry.status}</span>
      </div>

      {canMutate && (
        <div className="flex flex-wrap gap-2">
          {pending && (
            <Button
              disabled={approve.isPending}
              onClick={() => approve.mutate({ declarationId: pending.id, version }, {
                onSuccess: () => toast.success('Декларация одобрена'),
                onError: (error) => toast.error(error instanceof Error ? error.message : 'Не удалось одобрить'),
              })}
            >
              <CheckCircle2 className="h-4 w-4" /> Одобрить декларацию v{pending.declaration_version}
            </Button>
          )}
          {entry.status !== 'disabled' && entry.status !== 'retired' && (
            <>
              <Button variant="outline" disabled={changeStatus.isPending} onClick={() => changeStatus.mutate({ action: 'disable', version }, { onSuccess: () => toast.success('Сервис отключён'), onError: (e) => toast.error(e.message) })}>Disable</Button>
              <Button variant="outline" disabled={changeStatus.isPending} onClick={() => changeStatus.mutate({ action: 'retire', version }, { onSuccess: () => toast.success('Сервис выведен из эксплуатации'), onError: (e) => toast.error(e.message) })}>Retire</Button>
            </>
          )}
        </div>
      )}

      <section className="rounded-lg border border-border bg-surface p-5">
        <h2 className="mb-4 flex items-center gap-2 text-sm font-medium"><ShieldCheck className="h-4 w-4 text-accent" /> Активный integration contract</h2>
        {active ? <div className="space-y-4 text-sm"><div><div className="text-xs text-text-muted">Base URL</div><code className="mt-1 block break-all text-text-secondary">{active.integration_base_url}</code></div><div><div className="text-xs text-text-muted">Capabilities</div><div className="mt-2 flex flex-wrap gap-2">{active.capabilities.map((cap) => <span key={cap} className="rounded bg-surface-raised px-2 py-1 font-mono text-xs">{cap}</span>)}</div></div><div className="flex items-center gap-2 text-xs text-success"><CheckCircle2 className="h-4 w-4" /> Contract v{active.service_contract_version}, approved</div></div> : <p className="text-sm text-text-muted">Активная декларация ещё не одобрена.</p>}
      </section>

      {canMutate && (
        <form className="space-y-4 rounded-lg border border-border bg-surface p-5" onSubmit={submitDeclaration}>
          <h2 className="text-sm font-medium">Новая декларация</h2>
          <label className="block text-sm font-medium">
            Base URL
            <Input className="mt-1 max-w-md" value={newBaseUrl} onChange={(e) => setNewBaseUrl(e.target.value)} placeholder="http://localhost:7801" required />
          </label>
          <fieldset className="text-sm font-medium">
            Capabilities
            <div className="mt-2 flex flex-wrap gap-2">
              {KNOWN_CAPABILITIES.map((cap) => (
                <label key={cap} className={`cursor-pointer rounded-md border px-3 py-1.5 font-mono text-xs ${newCaps.includes(cap) ? 'border-accent bg-accent/10 text-accent' : 'border-border text-text-secondary'}`}>
                  <input type="checkbox" className="sr-only" checked={newCaps.includes(cap)} onChange={() => setNewCaps((prev) => prev.includes(cap) ? prev.filter((c) => c !== cap) : [...prev, cap])} />
                  {cap}
                </label>
              ))}
            </div>
          </fieldset>
          <Button type="submit" disabled={patch.isPending || !newBaseUrl.trim()}>{patch.isPending ? 'Отправляем...' : 'Отправить декларацию'}</Button>
        </form>
      )}

      <section className="rounded-lg border border-border bg-surface p-5"><h2 className="mb-4 text-sm font-medium">История деклараций</h2><div className="space-y-3">{declarations.map((declaration) => <div key={declaration.id} className="rounded-md border border-border p-3 text-sm"><div className="flex justify-between gap-3"><span>v{declaration.service_contract_version} · декларация {declaration.declaration_version}</span><span className={declaration.approval_status === 'approved' ? 'text-success' : declaration.approval_status === 'pending' ? 'text-warning' : 'text-text-muted'}>{declaration.approval_status}</span></div><div className="mt-1 break-all font-mono text-xs text-text-muted">{declaration.integration_base_url}</div></div>)}</div></section>
    </div>
  )
}
