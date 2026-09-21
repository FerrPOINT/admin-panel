import { useEffect, useState } from 'react'
import { Activity, RotateCw } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '@sdlc/ui/ui'
import { useRunServiceCheck, useServiceChecks, type ServiceStatus } from '@/shared/api/hooks'

const OUTCOME_LABELS: Record<string, string> = {
  success: 'Успешно',
  unreachable: 'Недоступен',
  timeout: 'Тайм-аут',
  rejected: 'Отклонено',
  invalid_response: 'Некорректный ответ',
  internal_error: 'Внутренняя ошибка',
}

function outcomeClass(outcome: string) {
  if (outcome === 'success') return 'text-success'
  if (outcome === 'timeout' || outcome === 'unreachable') return 'text-warning'
  return 'text-danger'
}

function checkDate(value: string) {
  return new Date(value).toLocaleString('ru-RU', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function ServiceChecks({
  serviceKey,
  serviceStatus,
  capabilities,
  canMutate,
}: {
  serviceKey: string
  serviceStatus: ServiceStatus
  capabilities: string[]
  canMutate: boolean
}) {
  const checks = useServiceChecks(serviceKey)
  const runCheck = useRunServiceCheck(serviceKey)
  const [selectedCapability, setSelectedCapability] = useState(capabilities[0] ?? '')

  useEffect(() => {
    if (!capabilities.includes(selectedCapability)) setSelectedCapability(capabilities[0] ?? '')
  }, [capabilities, selectedCapability])

  const canRun = serviceStatus === 'active' && Boolean(selectedCapability)
  const hasChecks = Boolean(checks.data?.checks.length)

  return (
    <section className="rounded-lg border border-border bg-surface p-4 sm:p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-medium">
            <Activity className="h-4 w-4 text-accent" aria-hidden /> Проверки интеграции
          </h2>
          <p className="mt-1 text-xs text-text-muted">
            Только разрешённые read-only проверки активного контракта
          </p>
        </div>
        {checks.isFetching && !checks.isLoading ? (
          <span className="text-xs text-text-muted">Обновляем...</span>
        ) : null}
      </div>

      {canMutate ? (
        <div className="mt-4 flex flex-col gap-2 sm:flex-row sm:items-end">
          <label className="min-w-0 flex-1 text-sm font-medium">
            Возможность
            <select
              className="mt-1 h-10 w-full rounded-md border border-border-strong bg-surface px-3 text-sm text-text-primary shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-50"
              value={selectedCapability}
              disabled={!capabilities.length || runCheck.isPending}
              onChange={(event) => setSelectedCapability(event.target.value)}
            >
              {!capabilities.length ? <option value="">Нет доступных проверок</option> : null}
              {capabilities.map((capability) => (
                <option key={capability} value={capability}>
                  {capability}
                </option>
              ))}
            </select>
          </label>
          <Button
            type="button"
            className="h-10 shrink-0"
            disabled={!canRun || runCheck.isPending}
            onClick={() =>
              runCheck.mutate(selectedCapability, {
                onSuccess: (result) =>
                  toast.success(
                    result.check_run.outcome === 'success'
                      ? 'Проверка завершена успешно'
                      : 'Проверка завершена; результат добавлен в историю',
                  ),
              })
            }
          >
            <RotateCw
              className={`h-4 w-4 ${runCheck.isPending ? 'animate-spin' : ''}`}
              aria-hidden
            />
            {runCheck.isPending ? 'Проверяем...' : 'Запустить проверку'}
          </Button>
        </div>
      ) : null}

      {canMutate && serviceStatus !== 'active' ? (
        <p className="mt-2 text-xs text-text-muted">
          Проверки доступны только для активного сервиса.
        </p>
      ) : null}
      {runCheck.isError ? (
        <p role="alert" className="mt-3 text-sm text-danger">
          {runCheck.error instanceof Error
            ? runCheck.error.message
            : 'Не удалось выполнить проверку'}
        </p>
      ) : null}

      {checks.isLoading ? (
        <p className="mt-4 text-sm text-text-muted">Загружаем историю проверок...</p>
      ) : null}
      {checks.isError ? (
        <div
          role="alert"
          className="mt-4 flex flex-wrap items-center justify-between gap-3 text-sm text-danger"
        >
          <span>
            {hasChecks
              ? 'Не удалось обновить историю проверок.'
              : 'Не удалось загрузить историю проверок.'}
          </span>
          <Button variant="outline" className="h-10" onClick={() => void checks.refetch()}>
            Повторить
          </Button>
        </div>
      ) : null}

      {hasChecks ? (
        <div className="mt-4 divide-y divide-border border-y border-border">
          {checks.data!.checks.map((check) => (
            <div
              key={check.id}
              className="grid gap-1 py-3 text-sm sm:grid-cols-[minmax(0,1fr)_auto] sm:gap-x-4"
            >
              <div className="min-w-0">
                <p className="break-all font-mono text-xs text-text-primary">
                  {check.capability_key}
                </p>
                {check.summary !== (check.http_status ? `HTTP ${check.http_status}` : '') ? (
                  <p className="mt-1 break-words text-xs text-text-muted">{check.summary}</p>
                ) : null}
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1 sm:justify-end">
                <span className={`text-xs font-medium ${outcomeClass(check.outcome)}`}>
                  {OUTCOME_LABELS[check.outcome] ?? check.outcome}
                </span>
                {check.http_status ? (
                  <span className="font-mono text-xs text-text-muted">
                    HTTP {check.http_status}
                  </span>
                ) : null}
              </div>
              <p className="break-all text-xs text-text-muted sm:col-span-2">
                {check.triggered_by_subject} ·{' '}
                <time dateTime={check.started_at}>{checkDate(check.started_at)}</time>
              </p>
            </div>
          ))}
        </div>
      ) : null}
      {!checks.isLoading && !checks.isError && !hasChecks ? (
        <p className="mt-4 text-sm text-text-muted">Проверок пока не запускали.</p>
      ) : null}
    </section>
  )
}
