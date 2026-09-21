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
import { ServiceChecks } from './service-checks'

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
const STATUS_CLASSES: Record<string, string> = {
  active: 'border-success/30 bg-success/10 text-success',
  pending: 'border-warning/30 bg-warning/10 text-warning',
  disabled: 'border-danger/30 bg-danger/10 text-danger',
  retired: 'border-border bg-surface-raised text-text-muted',
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
  const checkableCapabilities =
    active?.capabilities.filter((capability) => KNOWN_CAPABILITIES.includes(capability)) ?? []

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
    <div className="space-y-5">
      <Link
        to="/services"
        className="inline-flex min-h-10 items-center gap-2 text-sm text-text-secondary hover:text-text-primary"
      >
        <ArrowLeft className="h-4 w-4" aria-hidden /> Каталог сервисов
      </Link>
      <div>
        <h1 className="text-xl font-semibold">{entry.display_name}</h1>
        <p className="mt-1 text-sm text-text-muted">Контракт, проверки и история интеграции</p>
      </div>

      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_320px]">
        <aside className="space-y-4 xl:col-start-2 xl:row-start-1 xl:sticky xl:top-20">
          <section className="rounded-lg border border-border bg-surface p-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-sm font-semibold">Состояние сервиса</h2>
              <span
                className={`rounded-md border px-2.5 py-1 text-xs font-medium ${STATUS_CLASSES[entry.status] ?? 'border-border text-text-secondary'}`}
              >
                {STATUS_LABELS[entry.status] ?? entry.status}
              </span>
            </div>
            <dl className="mt-4 divide-y divide-border text-sm">
              <div className="grid grid-cols-[100px_minmax(0,1fr)] gap-3 py-2 first:pt-0">
                <dt className="text-text-muted">Ключ</dt>
                <dd className="break-all text-right font-mono text-xs text-text-primary">
                  {entry.service_key}
                </dd>
              </div>
              <div className="grid grid-cols-[100px_minmax(0,1fr)] gap-3 py-2">
                <dt className="text-text-muted">Команда</dt>
                <dd className="break-words text-right text-text-primary">{entry.owner_team}</dd>
              </div>
              <div className="grid grid-cols-[100px_minmax(0,1fr)] gap-3 py-2">
                <dt className="text-text-muted">Версия</dt>
                <dd className="text-right text-text-primary">{entry.version}</dd>
              </div>
              <div className="grid grid-cols-[100px_minmax(0,1fr)] gap-3 py-2">
                <dt className="text-text-muted">Декларации</dt>
                <dd className="text-right text-text-primary">{declarations.length}</dd>
              </div>
              <div className="grid grid-cols-[100px_minmax(0,1fr)] gap-3 py-2 last:pb-0">
                <dt className="text-text-muted">Активная</dt>
                <dd className="text-right text-text-primary">
                  {active ? `v${active.declaration_version}` : 'Нет'}
                </dd>
              </div>
            </dl>

            {canMutate ? (
              <div className="mt-4 space-y-2 border-t border-border pt-4">
                <h3 className="text-xs font-medium uppercase text-text-muted">Действия</h3>
                {pending ? (
                  <Button
                    className="h-10 w-full"
                    disabled={isMutating}
                    onClick={() =>
                      approve.mutate(
                        { declarationId: pending.id, version },
                        {
                          onSuccess: () => toast.success('Декларация одобрена'),
                          onError: (error) =>
                            toast.error(
                              error instanceof Error ? error.message : 'Не удалось одобрить',
                            ),
                        },
                      )
                    }
                  >
                    <CheckCircle2 className="h-4 w-4" aria-hidden /> Одобрить декларацию v
                    {pending.declaration_version}
                  </Button>
                ) : null}
                {entry.status !== 'disabled' && entry.status !== 'retired' ? (
                  <div className="grid gap-2">
                    <Button
                      variant="outline"
                      className="h-10 w-full"
                      disabled={isMutating}
                      onClick={() => setStatusTarget('disable')}
                    >
                      Отключить
                    </Button>
                    <Button
                      variant="outline"
                      className="h-10 w-full"
                      disabled={isMutating}
                      onClick={() => setStatusTarget('retire')}
                    >
                      Вывести из эксплуатации
                    </Button>
                  </div>
                ) : null}
                {!pending && (entry.status === 'disabled' || entry.status === 'retired') ? (
                  <p className="text-xs text-text-muted">Доступных действий нет.</p>
                ) : null}
              </div>
            ) : null}
          </section>
        </aside>

        <div className="min-w-0 space-y-5 xl:col-start-1 xl:row-start-1">
          <section className="rounded-lg border border-border bg-surface p-4 sm:p-5">
            <h2 className="mb-4 flex items-center gap-2 text-sm font-medium">
              <ShieldCheck className="h-4 w-4 text-accent" aria-hidden /> Активный контракт
              интеграции
            </h2>
            {active ? (
              <div className="space-y-4 text-sm">
                <div>
                  <div className="text-xs text-text-muted">Базовый URL</div>
                  <code className="mt-1 block break-all text-text-secondary">
                    {active.integration_base_url}
                  </code>
                </div>
                {active.public_ui_url ? (
                  <div>
                    <div className="text-xs text-text-muted">Публичный URL веб-интерфейса</div>
                    <code className="mt-1 block break-all text-text-secondary">
                      {active.public_ui_url}
                    </code>
                  </div>
                ) : null}
                <div>
                  <div className="text-xs text-text-muted">Возможности</div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    {active.capabilities.map((cap) => (
                      <span
                        key={cap}
                        className="break-all rounded bg-surface-raised px-2 py-1 font-mono text-xs"
                      >
                        {cap}
                      </span>
                    ))}
                  </div>
                </div>
                <div className="flex items-center gap-2 text-xs text-success">
                  <CheckCircle2 className="h-4 w-4 shrink-0" aria-hidden /> Контракт v
                  {active.service_contract_version} одобрен
                </div>
              </div>
            ) : (
              <p className="text-sm text-text-muted">Активная декларация ещё не одобрена.</p>
            )}
          </section>

          <ServiceChecks
            serviceKey={serviceKey}
            serviceStatus={entry.status}
            capabilities={checkableCapabilities}
            canMutate={canMutate}
          />

          {canMutate ? (
            <form
              className="space-y-4 rounded-lg border border-border bg-surface p-4 sm:p-5"
              onSubmit={submitDeclaration}
            >
              <div>
                <h2 className="text-sm font-medium">Новая декларация</h2>
                <p className="mt-1 text-xs text-text-muted">
                  Создаёт новую версию контракта и отправляет её на одобрение
                </p>
              </div>
              <label className="block text-sm font-medium">
                Базовый URL
                <Input
                  className="mt-1 h-10 max-w-md"
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
              {newCaps.includes('ui.render') ? (
                <label className="block text-sm font-medium">
                  Публичный URL веб-интерфейса
                  <Input
                    className="mt-1 h-10 max-w-md"
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
              ) : null}
              <fieldset className="text-sm font-medium">
                <legend>Возможности интеграции</legend>
                <div className="mt-2 flex flex-wrap gap-2">
                  {KNOWN_CAPABILITIES.map((cap) => (
                    <label
                      key={cap}
                      className={`inline-flex min-h-10 cursor-pointer items-center rounded-md border px-3 font-mono text-xs focus-within:ring-2 focus-within:ring-accent ${newCaps.includes(cap) ? 'border-accent bg-accent/10 text-text-primary' : 'border-border text-text-secondary'}`}
                    >
                      <input
                        type="checkbox"
                        aria-label={cap}
                        className="sr-only"
                        checked={newCaps.includes(cap)}
                        onChange={() => {
                          setDraftDirty(true)
                          setNewCaps((prev) =>
                            prev.includes(cap)
                              ? prev.filter((capability) => capability !== cap)
                              : [...prev, cap],
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
              {patch.isError ? (
                <p role="alert" className="text-sm text-danger">
                  {patch.error instanceof Error
                    ? patch.error.message
                    : 'Не удалось отправить декларацию'}
                </p>
              ) : null}
            </form>
          ) : null}

          <section className="rounded-lg border border-border bg-surface p-4 sm:p-5">
            <h2 className="text-sm font-semibold">История деклараций</h2>
            <div className="mt-3 divide-y divide-border border-y border-border">
              {declarations.map((declaration) => (
                <div
                  key={declaration.id}
                  className="grid gap-1 py-3 text-sm sm:grid-cols-[minmax(0,1fr)_auto] sm:gap-x-4"
                >
                  <span>
                    v{declaration.service_contract_version} · декларация{' '}
                    {declaration.declaration_version}
                  </span>
                  <span
                    className={`text-xs font-medium sm:text-right ${
                      declaration.approval_status === 'approved'
                        ? 'text-success'
                        : declaration.approval_status === 'pending'
                          ? 'text-warning'
                          : 'text-text-muted'
                    }`}
                  >
                    {APPROVAL_LABELS[declaration.approval_status] ?? declaration.approval_status}
                  </span>
                  <code className="break-all text-xs text-text-muted sm:col-span-2">
                    {declaration.integration_base_url}
                  </code>
                  {declaration.declared_by_subject && declaration.declared_at ? (
                    <p className="break-all text-xs text-text-muted sm:col-span-2">
                      {declaration.declared_by_subject} ·{' '}
                      <time dateTime={declaration.declared_at}>
                        {new Date(declaration.declared_at).toLocaleString('ru-RU')}
                      </time>
                    </p>
                  ) : null}
                </div>
              ))}
              {declarations.length === 0 ? (
                <p className="py-4 text-sm text-text-muted">Деклараций пока нет.</p>
              ) : null}
            </div>
          </section>
        </div>
      </div>

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
