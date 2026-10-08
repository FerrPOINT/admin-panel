import { useLocation } from 'react-router'
import { useState, type FormEvent } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Button, Input, Label, Textarea, usePlatformServices } from '@sdlc/ui/ui'
import { parseNamespaceLocation, useSessionCommand, withNamespaceLocation } from '@sdlc/ui/lib'
import { useAuth } from '@/shared/auth/auth-context'
import { api } from '@/shared/api/client'
import {
  useNamespace,
  useNamespaces,
  useNamespaceOwners,
  type Namespace,
  type NamespaceCommand,
  type Operation,
  type OwnerInstance,
} from '@/shared/api/namespaces'
import { ExistingResourcePicker, OwnerCounters } from './owner-resources'

const labels: Record<string, string> = {
  tracker_project: 'Задачи',
  wiki_space: 'Документы',
  git_group: 'Репозитории',
}
const states: Record<string, string> = {
  provisioning: 'Подключение ресурсов',
  active: 'Активен',
  archiving: 'Архивируется',
  archived: 'В архиве',
  restoring: 'Восстанавливается',
}
function errorText(error: unknown) {
  return error instanceof Error ? error.message : 'Не удалось выполнить операцию'
}
const inputClass = 'space-y-2'
function Field({
  label,
  name,
  defaultValue,
  required = true,
}: {
  label: string
  name: string
  defaultValue?: string
  required?: boolean
}) {
  return (
    <div className={inputClass}>
      <Label htmlFor={name}>{label}</Label>
      <Input id={name} name={name} defaultValue={defaultValue} required={required} />
    </div>
  )
}
export function NamespacesPage() {
  const [offset, setOffset] = useState(0)
  const catalog = useNamespaces(offset)
  const { session, canMutate } = useAuth()
  const navigate = useNavigate()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const pendingRef = useSessionCommand<Record<string, unknown>>(
    `admin:${session?.subject}:namespace-create`,
  )
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    // Keep the exact command on unknown outcome. A retry cannot mint another Namespace.
    pendingRef.current ??= {
      operation_id: crypto.randomUUID(),
      slug: data.get('slug'),
      name: data.get('name'),
      description: data.get('description') ?? '',
      responsible_subject: session?.subject,
    }
    setBusy(true)
    setError(undefined)
    try {
      const n = await api.post<Namespace>('/api/v1/namespaces', pendingRef.current)
      pendingRef.current = null
      navigate(
        withNamespaceLocation(`/namespaces/${n.id}`, {
          registry_instance_id: n.registry_instance_id,
          namespace_id: n.id,
        }),
      )
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Проекты</h1>
        <p className="mt-2 text-sm text-text-muted">
          Общий контекст задач, документов и репозиториев.
        </p>
      </div>
      {catalog.isPending ? (
        <p role="status">Загружаем проекты…</p>
      ) : catalog.isError ? (
        <p role="alert" className="text-danger">
          Реестр проектов недоступен.
        </p>
      ) : (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {catalog.data?.map((n) => (
            <Link
              key={n.id}
              to={withNamespaceLocation(`/namespaces/${n.id}`, {
                registry_instance_id: n.registry_instance_id,
                namespace_id: n.id,
              })}
              className="rounded-lg border border-border bg-surface p-4 hover:border-border-strong"
            >
              <h2 className="truncate font-medium">{n.name}</h2>
              <p className="mt-1 text-xs text-text-muted">
                {n.slug} · {states[n.state] ?? n.state}
              </p>
              <p className="mt-3 line-clamp-2 text-sm">{n.description}</p>
            </Link>
          ))}
          {catalog.data?.length === 0 && <p className="text-text-muted">Проектов пока нет.</p>}
        </div>
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - 50))}
        >
          Назад
        </Button>
        <Button
          variant="outline"
          disabled={(catalog.data?.length ?? 0) < 50}
          onClick={() => setOffset(offset + 50)}
        >
          Далее
        </Button>
      </div>
      {canMutate && (
        <form
          onSubmit={create}
          className="max-w-xl space-y-4 rounded-lg border border-border bg-surface p-4"
        >
          <h2 className="font-semibold">Создать проект</h2>
          <Field label="Название" name="name" />
          <Field label="Адрес проекта" name="slug" />
          <p className="text-xs text-text-muted">
            Латинские буквы, цифры и дефисы. Адрес сохраняется после создания.
          </p>
          <div className={inputClass}>
            <Label htmlFor="description">Описание</Label>
            <Textarea id="description" name="description" />
          </div>
          <p className="text-sm text-text-muted">
            Ответственный: {session?.email ?? session?.subject}
          </p>
          {error && (
            <p role="alert" className="text-danger">
              {error}. Повтор использует исходную операцию.
            </p>
          )}
          <Button disabled={busy}>
            {busy
              ? 'Создаём…'
              : pendingRef.current
                ? 'Повторить исходную операцию'
                : 'Далее: ресурсы'}
          </Button>
        </form>
      )}
    </div>
  )
}

type ResourceChoice = {
  kind: OwnerInstance['kind']
  instance_id: string
  mode: 'create' | 'attach'
  id: string
  key: string
}
export function NamespacePage() {
  const { id } = useParams()
  const location = useLocation()
  const urlRef = parseNamespaceLocation(location.search)
  const context = useNamespace(id)
  const owners = useNamespaceOwners()
  const cache = useQueryClient()
  const { canMutate, session } = useAuth()
  const { services } = usePlatformServices()
  const [choices, setChoices] = useState<Record<string, Partial<ResourceChoice>>>({})
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string>()
  const [operation, setOperation] = useState<Operation>()
  const pendingRef = useSessionCommand<NamespaceCommand>(
    `admin:${session?.subject}:namespace-operation:${id}`,
  )
  const n = context.data?.namespace
  async function updateProperties(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!n) return
    const fields = new FormData(event.currentTarget)
    setBusy(true)
    setError(undefined)
    try {
      await api.patch<Namespace>(`/api/v1/namespaces/${n.id}`, {
        expected_revision: n.revision,
        name: fields.get('name'),
        description: fields.get('description'),
        responsible_subject: fields.get('responsible_subject'),
      })
      await refresh()
    } catch (error) {
      setError(`${errorText(error)}. Обновите данные проекта перед повтором.`)
    } finally {
      setBusy(false)
    }
  }
  async function refresh() {
    await cache.invalidateQueries({ queryKey: ['namespace', id] })
    await cache.invalidateQueries({ queryKey: ['namespaces'] })
  }
  async function run(command: NamespaceCommand) {
    pendingRef.current ??= command
    setBusy(true)
    setError(undefined)
    try {
      const op = await api.post<Operation>(
        `/api/v1/namespaces/${id}/operations`,
        pendingRef.current,
      )
      setOperation(op)
      if (op.state === 'completed') pendingRef.current = null
      await refresh()
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }
  async function reconcile(opId: string) {
    setBusy(true)
    setError(undefined)
    try {
      const op = await api.post<Operation>(`/api/v1/namespace-operations/${opId}/reconcile`)
      setOperation(op)
      if (op.state === 'completed') pendingRef.current = null
      await refresh()
    } catch (e) {
      setError(errorText(e))
    } finally {
      setBusy(false)
    }
  }
  async function provision(event: FormEvent) {
    event.preventDefault()
    if (!n || !owners.data) return
    if (pendingRef.current) return run(pendingRef.current)
    const resources = owners.data.map((owner) => {
      const choice = choices[owner.kind] ?? {}
      const mode = choice.mode ?? 'create'
      return {
        resource: {
          kind: owner.kind,
          instance_id: owner.instance_id,
          resource_id: mode === 'attach' ? (choice.id ?? '') : crypto.randomUUID(),
        },
        create_spec:
          mode === 'attach'
            ? null
            : owner.kind === 'git_group'
              ? { slug: n.slug, name: n.name }
              : {
                  key:
                    choice.key ??
                    (owner.kind === 'tracker_project' ? n.slug.slice(0, 10).toUpperCase() : n.slug),
                  name: n.name,
                  description: n.description,
                  owner_subject: n.responsible_subject,
                },
      }
    })
    await run({
      action: 'provision',
      operation_id: crypto.randomUUID(),
      expected_revision: n.revision,
      resources,
    })
  }
  if (context.isPending) return <p role="status">Загружаем проект…</p>
  if (
    context.isError ||
    !n ||
    (new URLSearchParams(location.search).has('namespace_id') &&
      (!urlRef ||
        urlRef.namespace_id !== n.id ||
        urlRef.registry_instance_id !== n.registry_instance_id))
  )
    return (
      <p role="alert" className="text-danger">
        Проект недоступен. Контекст не заменён другим проектом.
      </p>
    )
  const ref = { registry_instance_id: n.registry_instance_id, namespace_id: n.id }
  return (
    <div className="space-y-6">
      <Link to="/namespaces" className="text-sm text-accent hover:underline">
        Все проекты
      </Link>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">{n.name}</h1>
          <p className="mt-2 text-sm text-text-muted">
            {n.slug} · {states[n.state] ?? n.state}
          </p>
        </div>
        {canMutate && ['active', 'archived'].includes(n.state) && (
          <Button
            variant="outline"
            disabled={busy}
            onClick={() =>
              run({
                action: n.state === 'active' ? 'archive' : 'restore',
                operation_id: crypto.randomUUID(),
                expected_revision: n.revision,
              })
            }
          >
            {n.state === 'active' ? 'Архивировать' : 'Восстановить'}
          </Button>
        )}
      </div>
      <p className="text-sm">{n.description}</p>
      <p className="text-sm text-text-muted">Ответственный: {n.responsible_subject}</p>
      {canMutate && (
        <details className="rounded-lg border border-border p-4">
          <summary className="cursor-pointer font-medium">Настройки проекта</summary>
          <form key={n.revision} onSubmit={updateProperties} className="mt-4 max-w-xl space-y-4">
            <Field label="Название" name="name" defaultValue={n.name} />
            <div className={inputClass}>
              <Label htmlFor="description">Описание</Label>
              <Textarea id="description" name="description" defaultValue={n.description} />
            </div>
            <Field
              label="Ответственный"
              name="responsible_subject"
              defaultValue={n.responsible_subject}
            />
            <Button disabled={busy}>Сохранить настройки</Button>
          </form>
        </details>
      )}
      {error && (
        <p role="alert" className="text-danger">
          {error}
        </p>
      )}
      {pendingRef.current && error && (
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => reconcile(pendingRef.current!.operation_id)}
        >
          Проверить исходную операцию
        </Button>
      )}
      {operation?.state === 'pending' && (
        <div role="status" className="rounded-lg border border-border p-4">
          <p>Ожидаем подтверждения ресурсов. Данные частично созданного проекта сохраняются.</p>
          <Button className="mt-3" disabled={busy} onClick={() => reconcile(operation.id)}>
            Проверить подключение
          </Button>
        </div>
      )}
      <div className="grid gap-3 md:grid-cols-3">
        {(['tracker_project', 'wiki_space', 'git_group'] as const).map((kind) => {
          const binding = context.data.bindings.find((b) => b.resource.kind === kind)
          const service = services.find(
            (s) =>
              s.key ===
              { tracker_project: 'task-tracker', wiki_space: 'wiki', git_group: 'ci-cd' }[kind],
          )
          return (
            <section key={kind} className="rounded-lg border border-border bg-surface p-4">
              <h2 className="font-medium">{labels[kind]}</h2>
              <p className="mt-2 text-sm text-text-muted">
                {binding
                  ? binding.confirmed
                    ? 'Подключено'
                    : 'Ожидает подтверждения'
                  : 'Не подключено'}
              </p>
              {binding?.last_error && (
                <p role="alert" className="mt-2 text-sm text-danger">
                  {binding.last_error}
                </p>
              )}
              {binding?.confirmed && (
                <OwnerCounters
                  kind={kind}
                  ref={ref}
                  resourceId={binding.resource.resource_id}
                  generation={binding.generation}
                />
              )}
              {binding?.confirmed && service?.ui_url && (
                <a
                  className="mt-3 inline-block text-sm text-accent hover:underline"
                  href={withNamespaceLocation(service.ui_url + '/namespace', ref)}
                >
                  Открыть {labels[kind].toLowerCase()}
                </a>
              )}
            </section>
          )
        })}
      </div>
      {n.state === 'provisioning' && context.data.bindings.length === 0 && canMutate && (
        <form
          onSubmit={provision}
          className="space-y-4 rounded-lg border border-border bg-surface p-4"
        >
          <h2 className="font-semibold">Подключить ресурсы</h2>
          <p className="text-sm text-text-muted">
            Для каждого сервиса выберите создание или явное подключение существующего ресурса.
          </p>
          {owners.isError && (
            <p role="alert" className="text-danger">
              Владельцы ресурсов недоступны.
            </p>
          )}
          {owners.data?.map((owner) => (
            <fieldset key={owner.kind} className="space-y-3 rounded-md border border-border p-3">
              <legend className="px-1 text-sm font-medium">{labels[owner.kind]}</legend>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name={owner.kind}
                  checked={(choices[owner.kind]?.mode ?? 'create') === 'create'}
                  onChange={() =>
                    setChoices({
                      ...choices,
                      [owner.kind]: { ...choices[owner.kind], mode: 'create' },
                    })
                  }
                />
                Создать
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name={owner.kind}
                  checked={choices[owner.kind]?.mode === 'attach'}
                  onChange={() =>
                    setChoices({
                      ...choices,
                      [owner.kind]: { ...choices[owner.kind], mode: 'attach' },
                    })
                  }
                />
                Подключить существующий
              </label>
              {choices[owner.kind]?.mode === 'attach' ? (
                <ExistingResourcePicker
                  owner={owner}
                  namespaceId={n.id}
                  value={choices[owner.kind]?.id ?? ''}
                  onChange={(id) =>
                    setChoices({ ...choices, [owner.kind]: { ...choices[owner.kind], id } })
                  }
                />
              ) : (
                owner.kind !== 'git_group' && (
                  <div className={inputClass}>
                    <Label htmlFor={`${owner.kind}-key`}>Ключ проекта</Label>
                    <Input
                      id={`${owner.kind}-key`}
                      value={
                        choices[owner.kind]?.key ??
                        (owner.kind === 'tracker_project'
                          ? n.slug.slice(0, 10).toUpperCase()
                          : n.slug)
                      }
                      onChange={(e) =>
                        setChoices({
                          ...choices,
                          [owner.kind]: { ...choices[owner.kind], key: e.target.value },
                        })
                      }
                    />
                  </div>
                )
              )}
            </fieldset>
          ))}
          <Button disabled={busy || !owners.data}>
            {busy ? 'Подключаем…' : 'Подключить три ресурса'}
          </Button>
        </form>
      )}
      {context.data.bindings
        .filter((b) => !b.confirmed)
        .map((binding) => (
          <Button
            key={binding.resource.kind}
            variant="outline"
            disabled={busy}
            onClick={() => reconcile(binding.operation_id)}
          >
            Продолжить подключение: {labels[binding.resource.kind]}
          </Button>
        ))}
    </div>
  )
}
