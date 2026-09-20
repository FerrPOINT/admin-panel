import { FormEvent, useEffect, useState } from 'react'
import { Link, useParams } from 'react-router'
import { ArrowLeft, CheckCircle2, ShieldCheck } from 'lucide-react'
import { toast } from 'sonner'
import {
  Button,
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
} from '@sdlc/ui/ui'
import {
  useApproveService,
  useChangeServiceStatus,
  usePatchService,
  useService,
} from '@/shared/api/hooks'
import { useAuth } from '@/shared/auth/auth-context'

const KNOWN_CAPABILITIES = [
  'health.read',
  'ui.render',
  'integration.status.read',
  'branding.runtime.read',
]
const STATUS_LABELS: Record<string, string> = {
  active: 'Активен',
  pending: 'Ожидает',
  disabled: 'Отключён',
  retired: 'Выведен',
}
const APPROVAL_LABELS: Record<string, string> = {
  pending: 'Ожидает',
  approved: 'Одобрена',
  rejected: 'Отклонена',
  superseded: 'Заменена',
}

export function ServiceDetailPage() {
  const { serviceKey = '' } = useParams()
  const service = useService(serviceKey)
  const { canMutate } = useAuth()
  const approve = useApproveService(serviceKey)
  const patch = usePatchService(serviceKey)
  const changeStatus = useChangeServiceStatus(serviceKey)
  const [newBaseUrl, setNewBaseUrl] = useState('')
  const [newPublicUiUrl, setNewPublicUiUrl] = useState('')
  const [newCaps, setNewCaps] = useState<string[]>([])
  const [initializedFor, setInitializedFor] = useState('')
  const [draftDirty, setDraftDirty] = useState(false)
  const [statusTarget, setStatusTarget] = useState<'disable' | 'retire' | null>(null)
  const activeDeclaration = service.data?.declarations.find(
    (declaration) => declaration.id === service.data?.service.active_declaration_id,
  )
  const activeSource = `${serviceKey}:${activeDeclaration?.id ?? ''}`

  useEffect(() => {
    if (
      !service.data ||
      service.data.service.service_key !== serviceKey ||
      initializedFor === activeSource ||
      (draftDirty && initializedFor.startsWith(`${serviceKey}:`))
    )
      return
    setNewBaseUrl(activeDeclaration?.integration_base_url ?? '')
    setNewPublicUiUrl(activeDeclaration?.public_ui_url ?? '')
    setNewCaps(activeDeclaration?.capabilities ?? ['health.read'])
    setInitializedFor(activeSource)
    setDraftDirty(false)
  }, [activeDeclaration, activeSource, draftDirty, initializedFor, service.data, serviceKey])

  if (service.isLoading) return <div className="text-sm text-text-muted">Загрузка карточки...</div>
  if (service.isError || !service.data)
    return (
      <div role="alert" className="space-y-3 text-sm text-danger">
        <p>Не удалось загрузить карточку сервиса.</p>
        <Button variant="outline" className="h-10" onClick={() => void service.refetch()}>
          Повторить
        </Button>
      </div>
    )
  const { service: entry, declarations } = service.data
  const active = activeDeclaration
  const pending = declarations.find((d) => d.approval_status === 'pending')
  const version = entry.version
  const isMutating = patch.isPending || approve.isPending || changeStatus.isPending

  function submitDeclaration(event: FormEvent) {
    event.preventDefault()
    if (isMutating) return
    const nextVersion = declarations.reduce((max, d) => Math.max(max, d.declaration_version), 0) + 1
    patch.mutate(
      {
        version,
        body: {
          declaration: {
            declaration_version: nextVersion,
            integration_base_url: newBaseUrl,
            public_ui_url: newCaps.includes('ui.render') ? newPublicUiUrl : null,
            service_contract_version: active?.service_contract_version ?? '1.0.0',
            capabilities: newCaps,
          },
        },
      },
      {
        onSuccess: () => {
          toast.success('Декларация обработана; актуальный статус указан в истории')
          setNewBaseUrl(active?.integration_base_url ?? '')
          setNewPublicUiUrl(active?.public_ui_url ?? '')
          setNewCaps(active?.capabilities ?? ['health.read'])
          setDraftDirty(false)
        },
      },
    )
  }

  return (
    <div className="mx-auto max-w-4xl space-y-6">
      <Link
        to="/services"
        className="inline-flex min-h-10 items-center gap-2 text-sm text-text-secondary hover:text-text-primary"
      >
        <ArrowLeft className="h-4 w-4" /> Каталог сервисов
      </Link>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">{entry.display_name}</h1>
          <p className="mt-1 text-sm text-text-muted">
            Ключ: {entry.service_key} · команда {entry.owner_team} · v{entry.version}
          </p>
        </div>
        <span className="rounded-md border border-border px-3 py-1 text-sm text-text-secondary">
          {STATUS_LABELS[entry.status] ?? entry.status}
        </span>
      </div>

      {canMutate && (
        <div className="flex flex-wrap gap-2">
          {pending && (
            <Button
              className="h-10"
              disabled={isMutating}
              onClick={() =>
                approve.mutate(
                  { declarationId: pending.id, version },
                  {
                    onSuccess: () => toast.success('Декларация одобрена'),
                    onError: (error) =>
                      toast.error(error instanceof Error ? error.message : 'Не удалось одобрить'),
                  },
                )
              }
            >
              <CheckCircle2 className="h-4 w-4" /> Одобрить декларацию v
              {pending.declaration_version}
            </Button>
          )}
          {entry.status !== 'disabled' && entry.status !== 'retired' && (
            <>
              <Button
                variant="outline"
                className="h-10"
                disabled={isMutating}
                onClick={() => setStatusTarget('disable')}
              >
                Отключить
              </Button>
              <Button
                variant="outline"
                className="h-10"
                disabled={isMutating}
                onClick={() => setStatusTarget('retire')}
              >
                Вывести из эксплуатации
              </Button>
            </>
          )}
        </div>
      )}

      <section className="rounded-lg border border-border bg-surface p-5">
        <h2 className="mb-4 flex items-center gap-2 text-sm font-medium">
          <ShieldCheck className="h-4 w-4 text-accent" /> Активный контракт интеграции
        </h2>
        {active ? (
          <div className="space-y-4 text-sm">
            <div>
              <div className="text-xs text-text-muted">Базовый URL</div>
              <code className="mt-1 block break-all text-text-secondary">
                {active.integration_base_url}
              </code>
            </div>
            {active.public_ui_url && (
              <div>
                <div className="text-xs text-text-muted">Публичный URL веб-интерфейса</div>
                <code className="mt-1 block break-all text-text-secondary">
                  {active.public_ui_url}
                </code>
              </div>
            )}
            <div>
              <div className="text-xs text-text-muted">Возможности</div>
              <div className="mt-2 flex flex-wrap gap-2">
                {active.capabilities.map((cap) => (
                  <span key={cap} className="rounded bg-surface-raised px-2 py-1 font-mono text-xs">
                    {cap}
                  </span>
                ))}
              </div>
            </div>
            <div className="flex items-center gap-2 text-xs text-success">
              <CheckCircle2 className="h-4 w-4" /> Контракт v{active.service_contract_version}{' '}
              одобрен
            </div>
          </div>
        ) : (
          <p className="text-sm text-text-muted">Активная декларация ещё не одобрена.</p>
        )}
      </section>

      {canMutate && (
        <form
          className="space-y-4 rounded-lg border border-border bg-surface p-5"
          onSubmit={submitDeclaration}
        >
          <h2 className="text-sm font-medium">Новая декларация</h2>
          <label className="block text-sm font-medium">
            Базовый URL
            <Input
              className="mt-1 max-w-md"
              value={newBaseUrl}
              onChange={(e) => {
                setNewBaseUrl(e.target.value)
                setDraftDirty(true)
              }}
              placeholder="http://localhost:7801"
              required
              disabled={isMutating}
            />
          </label>
          {newCaps.includes('ui.render') && (
            <label className="block text-sm font-medium">
              Публичный URL веб-интерфейса
              <Input
                className="mt-1 max-w-md"
                type="url"
                value={newPublicUiUrl}
                onChange={(e) => {
                  setNewPublicUiUrl(e.target.value)
                  setDraftDirty(true)
                }}
                placeholder="http://localhost:7802"
                required
                disabled={isMutating}
              />
            </label>
          )}
          <fieldset className="text-sm font-medium">
            <legend>Возможности интеграции</legend>
            <div className="mt-2 flex flex-wrap gap-2">
              {KNOWN_CAPABILITIES.map((cap) => (
                <label
                  key={cap}
                  className={`inline-flex min-h-10 cursor-pointer items-center rounded-md border px-3 font-mono text-xs focus-within:ring-2 focus-within:ring-accent ${newCaps.includes(cap) ? 'border-accent bg-accent/10 text-accent' : 'border-border text-text-secondary'}`}
                >
                  <input
                    type="checkbox"
                    aria-label={cap}
                    className="sr-only"
                    checked={newCaps.includes(cap)}
                    onChange={() => {
                      setDraftDirty(true)
                      setNewCaps((prev) =>
                        prev.includes(cap) ? prev.filter((c) => c !== cap) : [...prev, cap],
                      )
                    }}
                    disabled={isMutating}
                  />
                  {cap}
                </label>
              ))}
            </div>
          </fieldset>
          <Button type="submit" className="h-10" disabled={isMutating || !newBaseUrl.trim()}>
            {patch.isPending ? 'Отправляем...' : 'Отправить декларацию'}
          </Button>
          {patch.isError && (
            <p role="alert" className="text-sm text-danger">
              {patch.error instanceof Error
                ? patch.error.message
                : 'Не удалось отправить декларацию'}
            </p>
          )}
        </form>
      )}

      <section>
        <h2 className="mb-2 text-sm font-semibold">История деклараций</h2>
        <div className="divide-y divide-border border-y border-border">
          {declarations.map((declaration) => (
            <div key={declaration.id} className="py-3 text-sm">
              <div className="flex justify-between gap-3">
                <span>
                  v{declaration.service_contract_version} · декларация{' '}
                  {declaration.declaration_version}
                </span>
                <span
                  className={
                    declaration.approval_status === 'approved'
                      ? 'text-success'
                      : declaration.approval_status === 'pending'
                        ? 'text-warning'
                        : 'text-text-muted'
                  }
                >
                  {APPROVAL_LABELS[declaration.approval_status] ?? declaration.approval_status}
                </span>
              </div>
              <div className="mt-1 break-all font-mono text-xs text-text-muted">
                {declaration.integration_base_url}
              </div>
            </div>
          ))}
          {declarations.length === 0 && (
            <p className="py-4 text-sm text-text-muted">Деклараций пока нет.</p>
          )}
        </div>
      </section>

      <Dialog
        open={Boolean(statusTarget)}
        onOpenChange={(open) => {
          if (!open && !changeStatus.isPending) setStatusTarget(null)
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {statusTarget === 'retire' ? 'Вывести сервис из эксплуатации?' : 'Отключить сервис?'}
            </DialogTitle>
          </DialogHeader>
          <p className="text-sm text-text-secondary">
            {entry.display_name} · {entry.service_key}
          </p>
          <p className="text-sm text-text-muted">
            {statusTarget === 'retire'
              ? 'Сервис перестанет быть активным в каталоге.'
              : 'Сервис станет недоступен для новых интеграций.'}
          </p>
          <DialogFooter>
            <Button
              variant="outline"
              className="h-10"
              disabled={changeStatus.isPending}
              onClick={() => setStatusTarget(null)}
            >
              Отмена
            </Button>
            <Button
              variant="destructive"
              className="h-10"
              disabled={changeStatus.isPending}
              onClick={() =>
                statusTarget &&
                changeStatus.mutate(
                  { action: statusTarget, version },
                  {
                    onSuccess: () => {
                      toast.success(
                        statusTarget === 'retire'
                          ? 'Сервис выведен из эксплуатации'
                          : 'Сервис отключён',
                      )
                      setStatusTarget(null)
                    },
                    onError: (error) => toast.error(error.message),
                  },
                )
              }
            >
              {changeStatus.isPending
                ? 'Сохраняем…'
                : statusTarget === 'retire'
                  ? 'Вывести'
                  : 'Отключить'}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
