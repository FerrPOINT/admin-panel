import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Button, usePlatformServices } from '@sdlc/ui/ui'
import type { NamespaceLocation } from '@sdlc/ui/lib'
import type { components } from '@/shared/api/schema'
import { useAuth } from '@/shared/auth/auth-context'
import type { OwnerInstance } from '@/shared/api/namespaces'

type Item = components['schemas']['ResourceCatalogItem']
type Stats = components['schemas']['ResourceStats']
const serviceKeys = {
  tracker_project: 'task-tracker',
  wiki_space: 'wiki',
  git_group: 'ci-cd',
} as const
const counterLabels: Record<string, string> = {
  tasks: 'Задачи',
  team: 'Участники команды',
  documents: 'Документы',
  revisions: 'Опубликованные версии',
  repositories: 'Репозитории',
  hosted: 'Git в Forge',
  external: 'Внешний Git',
}

async function ownerRead<T>(
  url: string,
  token: string | undefined,
  signal: AbortSignal,
): Promise<T> {
  if (!token || token.startsWith('sdlc_pat_'))
    throw new Error('Для чтения соседнего продукта требуется SSO-сессия')
  const response = await fetch(url, { signal, headers: { Authorization: `Bearer ${token}` } })
  if (!response.ok) throw new Error('Источник недоступен')
  return response.json() as Promise<T>
}

export function ExistingResourcePicker({
  owner,
  namespaceId,
  value,
  onChange,
}: {
  owner: OwnerInstance
  namespaceId: string
  value: string
  onChange: (id: string) => void
}) {
  const { services } = usePlatformServices()
  const { session } = useAuth()
  const [offset, setOffset] = useState(0)
  const service = services.find((item) => item.key === serviceKeys[owner.kind])
  const origin = service?.ui_url ?? service?.url
  const query = useQuery({
    queryKey: [
      'available-owner-resources',
      namespaceId,
      owner.kind,
      owner.instance_id,
      offset,
      session?.subject,
    ],
    enabled: Boolean(origin),
    queryFn: async ({ signal }) => {
      const rows = await ownerRead<Item[]>(
        `${origin}/api/v1/namespace-available-resources?limit=50&offset=${offset}`,
        session?.token,
        signal,
      )
      if (
        rows.some(
          (item) =>
            item.resource.kind !== owner.kind || item.resource.instance_id !== owner.instance_id,
        )
      )
        throw new Error('Каталог относится к другому экземпляру продукта')
      return rows
    },
  })
  return (
    <div className="space-y-3">
      {query.isPending ? (
        <p role="status">Загружаем доступные ресурсы…</p>
      ) : query.isError ? (
        <p role="alert" className="text-danger">
          Каталог продукта недоступен. Проверьте SSO-сессию и подключение.
        </p>
      ) : (
        <label className="block text-sm">
          Существующий ресурс
          <select
            required
            value={value}
            onChange={(event) => onChange(event.target.value)}
            className="mt-2 min-h-10 w-full rounded-md border border-border bg-surface px-3"
          >
            <option value="">Выберите ресурс</option>
            {query.data?.map((item) => (
              <option key={item.resource.resource_id} value={item.resource.resource_id}>
                {item.label} · {item.resource_key}
              </option>
            ))}
          </select>
          {query.data?.length === 0 && (
            <p className="mt-2 text-text-muted">Свободных ресурсов на этой странице нет.</p>
          )}
        </label>
      )}
      <div className="flex gap-2">
        <Button
          type="button"
          variant="outline"
          disabled={offset === 0}
          onClick={() => {
            setOffset(Math.max(0, offset - 50))
            onChange('')
          }}
        >
          Назад
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={(query.data?.length ?? 0) < 50}
          onClick={() => {
            setOffset(offset + 50)
            onChange('')
          }}
        >
          Далее
        </Button>
        <Button type="button" variant="outline" onClick={() => void query.refetch()}>
          Обновить
        </Button>
      </div>
    </div>
  )
}

export function OwnerCounters({
  kind,
  ref,
  resourceId,
  generation,
}: {
  kind: OwnerInstance['kind']
  ref: NamespaceLocation
  resourceId: string
  generation: number
}) {
  const { services } = usePlatformServices()
  const { session } = useAuth()
  const service = services.find((item) => item.key === serviceKeys[kind])
  const origin = service?.ui_url ?? service?.url
  const query = useQuery({
    queryKey: [
      'namespace-live-counters',
      ref.registry_instance_id,
      ref.namespace_id,
      kind,
      resourceId,
      generation,
      session?.subject,
    ],
    enabled: Boolean(origin),
    queryFn: async ({ signal }) => {
      const result = await ownerRead<Stats>(
        `${origin}/api/v1/namespace-stats/${ref.registry_instance_id}/${ref.namespace_id}`,
        session?.token,
        signal,
      )
      if (
        result.binding.namespace.registry_instance_id !== ref.registry_instance_id ||
        result.binding.namespace.namespace_id !== ref.namespace_id ||
        result.binding.resource.resource_id !== resourceId ||
        result.binding.generation !== generation
      )
        throw new Error('Привязка продукта требует сверки')
      return result
    },
  })
  if (query.isPending)
    return (
      <p role="status" className="mt-3 text-sm text-text-muted">
        Читаем показатели…
      </p>
    )
  if (query.isError || !query.data)
    return (
      <div className="mt-3 space-y-2">
        <p role="alert" className="text-sm text-danger">
          Показатели источника недоступны.
        </p>
        <Button variant="outline" size="sm" onClick={() => void query.refetch()}>
          Обновить
        </Button>
      </div>
    )
  return (
    <dl className="mt-3 space-y-1 text-sm">
      {Object.entries(query.data.counters).map(([key, value]) => (
        <div key={key} className="flex justify-between gap-3">
          <dt className="text-text-muted">{counterLabels[key] ?? key}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  )
}
