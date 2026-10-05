import { useCallback, useEffect, useState } from 'react'
import { useSearchParams } from 'react-router'
import { Bot, CheckCircle2, CircleHelp, RefreshCw } from 'lucide-react'
import { Button, Dialog, DialogContent, DialogHeader, DialogTitle, Input } from '@sdlc/ui/ui'
import { Heading, Section } from '@/shared/ai-ui/components'
import {
  ai,
  contextTokens,
  dollarAmount,
  modelContextText,
  type AcceptanceBudget,
  type Catalog,
  type Draft,
  type Login,
  type Profile,
  type ProviderId,
  type Registry,
  type Settings,
} from '@/shared/api/ai'
import { ApiError } from '@/shared/api/client'
import { useAiWorkspace } from '@/shared/ai-ui/workspace'
import '@/shared/ai-ui/style.css'

const names: Record<ProviderId, string> = { chatgpt: 'ChatGPT', openrouter: 'OpenRouter' }
const terminal = new Set(['completed', 'failed', 'cancelled', 'expired', 'interrupted', 'revoked'])
function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 412)
      return 'Настройки изменены другим пользователем. Ваш черновик сохранён; обновите ревизию перед сохранением.'
    if (error.status === 403) return 'Для этой сессии изменение настроек недоступно.'
    if (error.status === 429) return 'Квота провайдера исчерпана. Автоматического переключения нет.'
    if (error.status === 409)
      return 'Операция конфликтует с текущим состоянием. Проверьте её результат.'
    if (error.status === 424) return 'Подключение провайдера недоступно. Проверьте авторизацию.'
    if (error.status === 422) return 'Проверьте введённые настройки и состояние подключения.'
  }
  return 'Не удалось подтвердить результат запроса. Обновите состояние; повторный вход или запрос автоматически не запускается.'
}

export function AiPage() {
  const { setDirty } = useAiWorkspace()
  const [params, setParams] = useSearchParams()
  const provider: ProviderId = params.get('provider') === 'openrouter' ? 'openrouter' : 'chatgpt'
  const [registry, setRegistry] = useState<Registry | null>(null)
  const [drafts, setDrafts] = useState<Partial<Record<ProviderId, Settings>>>({})
  const [context, setContext] = useState<Partial<Record<ProviderId, Record<string, string>>>>({})
  const [profile, setProfile] = useState<Profile | null>(null)
  const [budget, setBudget] = useState<AcceptanceBudget | null>(null)
  const [budgetState, setBudgetState] = useState<'loading' | 'ready' | 'unavailable'>('loading')
  const [catalogs, setCatalogs] = useState<Partial<Record<ProviderId, Catalog>>>({})
  const [key, setKey] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [login, setLogin] = useState<Login | null>(null)
  const [loginOpen, setLoginOpen] = useState(false)
  const [pending, setPending] = useState<{
    provider: ProviderId
    id: string
    kind: 'login' | 'connection'
  } | null>(null)
  const [disconnectOpen, setDisconnectOpen] = useState(false)
  const [search, setSearch] = useState('')
  const saved: Draft | undefined = registry?.providers.find((p) => p.settings.provider === provider)
  const selected = drafts[provider] ?? saved?.settings
  const runtime = registry?.runtime?.providers.find((p) => p.id === provider)
  const catalog = catalogs[provider]
  const model = catalog?.models.find((m) => (m.model ?? m.id) === selected?.model)
  const contextText = modelContextText(saved, selected?.model ?? '', context[provider])
  const tokens = contextTokens(contextText)
  const contextError =
    tokens === null
      ? 'Введите целое число тысяч токенов, не меньше 64.'
      : model?.context_limit_tokens && tokens > model.context_limit_tokens
        ? 'Выбранный бюджет превышает объявленный предел модели.'
        : null

  const refresh = useCallback(async () => {
    setBudgetState('loading')
    const budgetRequest = ai
      .budget()
      .then((value) => {
        setBudget(value)
        setBudgetState('ready')
      })
      .catch(() => {
        setBudget(null)
        setBudgetState('unavailable')
      })
    const [current, selection] = await Promise.all([ai.registry(), ai.selection(), budgetRequest])
    setRegistry(current)
    setProfile(selection.profile)
    const activeLogin = current.runtime?.providers.find(
      (p) => p.id === 'chatgpt',
    )?.pending_login_operation
    if (activeLogin) {
      setPending({ provider: 'chatgpt', id: activeLogin, kind: 'login' })
      setLogin(await ai.loginStatus(activeLogin))
    }
  }, [])
  useEffect(() => {
    void refresh().catch((cause) => setError(errorMessage(cause)))
  }, [refresh])
  useEffect(() => {
    const changed = (['chatgpt', 'openrouter'] as ProviderId[]).some((id) => {
      const storedDraft = registry?.providers.find((p) => p.settings.provider === id)
      const stored = storedDraft?.settings
      const draft = drafts[id]
      return (
        (draft && stored && draft.model !== stored.model) ||
        Object.entries(context[id] ?? {}).some(
          ([model, text]) => text !== modelContextText(storedDraft, model),
        )
      )
    })
    setDirty(!!changed || !!key || !!pending)
    return () => setDirty(false)
  }, [registry, drafts, context, key, pending, setDirty])
  useEffect(() => {
    if (!login || terminal.has(login.status)) return
    const operation = login.operation_id
    let disposed = false
    let inFlight = false
    const interval = window.setInterval(() => {
      if (inFlight) return
      inFlight = true
      void ai
        .loginStatus(operation)
        .then(async (next) => {
          if (disposed) return
          setLogin(next)
          if (terminal.has(next.status)) {
            setPending(null)
            await refresh()
          }
        })
        .catch((cause) => {
          if (!disposed) setError(errorMessage(cause))
        })
        .finally(() => {
          inFlight = false
        })
    }, 2000)
    return () => {
      disposed = true
      window.clearInterval(interval)
    }
  }, [login, refresh])

  async function run(action: () => Promise<void>) {
    setBusy(true)
    setError(null)
    setMessage(null)
    try {
      await action()
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setBusy(false)
    }
  }
  function choose(next: ProviderId) {
    const query = new URLSearchParams(params)
    query.set('provider', next)
    setParams(query)
    setKey('')
    setSearch('')
    setError(null)
    setMessage(null)
  }
  function patch(update: Partial<Settings>) {
    if (selected) setDrafts((current) => ({ ...current, [provider]: { ...selected, ...update } }))
  }
  function chooseModel(model: string) {
    const text = modelContextText(saved, model, context[provider])
    setContext((current) => ({
      ...current,
      [provider]: { ...current[provider], [model]: text },
    }))
    patch({
      model,
      context_window_tokens: contextTokens(text) ?? 256000,
    })
  }
  async function save() {
    if (!selected || !saved || tokens === null || contextError) return
    const next = await ai.save({ ...selected, context_window_tokens: tokens }, saved.draft_revision)
    setRegistry((current) =>
      current
        ? {
            ...current,
            providers: current.providers.map((p) => (p.settings.provider === provider ? next : p)),
          }
        : current,
    )
    setDrafts((current) => ({ ...current, [provider]: next.settings }))
    setContext((current) => {
      const remaining = { ...current[provider] }
      delete remaining[next.settings.model]
      return { ...current, [provider]: remaining }
    })
    setMessage('Черновик сохранён. Перед публикацией нужна проверка модели и контекста.')
  }
  async function connectOpenRouter() {
    const operation = crypto.randomUUID()
    const credential = key
    setKey('')
    setPending({ provider: 'openrouter', id: operation, kind: 'connection' })
    await ai.credentials(credential, operation)
    setPending(null)
    await refresh()
    setMessage('Подключение сохранено. Доступ к выбранной модели ещё не проверен.')
  }
  async function startLogin() {
    const operation = crypto.randomUUID()
    setPending({ provider: 'chatgpt', id: operation, kind: 'login' })
    const attempt = await ai.login(operation)
    setLogin(attempt)
    setLoginOpen(true)
    if (terminal.has(attempt.status)) setPending(null)
  }
  async function readback() {
    if (!pending) return
    if (pending.kind === 'login') {
      const attempt = await ai.loginStatus(pending.id)
      setLogin(attempt)
      setLoginOpen(true)
      if (terminal.has(attempt.status)) setPending(null)
    } else {
      const operation = await ai.operation(pending.provider, pending.id)
      if (operation.status === 'completed') setPending(null)
      setMessage(`Состояние операции: ${operation.status}`)
    }
    await refresh()
  }

  return (
    <>
      <Heading
        title="AI-провайдеры"
        subtitle="Общий профиль для агентов и проверки Workflow в SDLC2."
      />
      <section className="ai-active" aria-label="Активный AI-профиль">
        <div>
          <span className="muted">Активный профиль</span>
          <strong>
            {profile ? `${names[profile.provider]} · ${profile.model}` : 'Не опубликован'}
          </strong>
          <p>
            {profile
              ? `${profile.context_window_tokens / 1000}K контекст · ревизия ${profile.revision}`
              : 'Подключите провайдера, проверьте модель и опубликуйте профиль.'}
          </p>
        </div>
        <span className="ai-status muted">
          <CircleHelp size={16} aria-hidden />
          {profile ? 'Требуется проверка доступности' : 'Не настроен'}
        </span>
      </section>
      <Section
        title="Бюджет платной приёмки"
        actions={
          <Button
            variant="outline"
            disabled={busy || budgetState === 'loading'}
            onClick={() => void run(refresh)}
          >
            Обновить бюджет
          </Button>
        }
      >
        {budgetState === 'loading' ? (
          <p className="ai-note">Загрузка бюджета…</p>
        ) : budgetState === 'unavailable' || !budget ? (
          <p role="status" className="ai-note">
            Бюджет недоступен. Остаток не подтверждён.
          </p>
        ) : (
          <>
            <dl className="ai-facts">
              <dt>Общий лимит</dt>
              <dd>{dollarAmount(budget.limit_microdollars)}</dd>
              <dt>Подтверждённые расходы</dt>
              <dd>{dollarAmount(budget.settled_microdollars)}</dd>
              <dt>Удержано для незавершённых запросов</dt>
              <dd>{dollarAmount(budget.reserved_microdollars)}</dd>
              <dt>Доступно</dt>
              <dd>{dollarAmount(budget.available_microdollars)}</dd>
            </dl>
            {budget.uncertain_requests > 0 && (
              <p role="status" className="ai-note">
                Исход {budget.uncertain_requests} запросов не подтверждён. Резерв{' '}
                {dollarAmount(budget.uncertain_microdollars)} удерживается до проверки провайдера.
              </p>
            )}
            {budget.blocked_reason && (
              <p role="alert" className="ai-error-box">
                {budget.blocked_reason === 'provider_cost_exceeds_reservation'
                  ? 'Провайдер превысил зарезервированную стоимость. Новые платные запросы заблокированы.'
                  : 'Бюджет приёмки исчерпан. Новые платные запросы заблокированы.'}
              </p>
            )}
          </>
        )}
        <p className="ai-note">
          Квота подписки ChatGPT учитывается отдельно. Автоматического переключения провайдера нет.
        </p>
      </Section>
      {error && (
        <p role="alert" className="ai-error-box">
          {error}
        </p>
      )}
      {message && (
        <p role="status" className="ai-message">
          <CheckCircle2 size={16} aria-hidden />
          {message}
        </p>
      )}
      {pending && (
        <div className="ai-actions">
          <p className="ai-note">Есть незавершённая операция.</p>
          <Button disabled={busy} variant="outline" onClick={() => void run(readback)}>
            Проверить состояние операции
          </Button>
        </div>
      )}
      {!registry ? (
        <Section title="Подключения">
          <p className="ai-note">Настройки недоступны.</p>
          <Button variant="outline" disabled={busy} onClick={() => void run(refresh)}>
            Обновить
          </Button>
        </Section>
      ) : (
        <>
          <div className="ai-provider-list" role="group" aria-label="Подключения">
            {registry.providers.map((p) => {
              const id = p.settings.provider
              const status = registry.runtime?.providers.find((r) => r.id === id)
              return (
                <button
                  key={id}
                  className={`ai-provider-row ${provider === id ? 'selected' : ''}`}
                  aria-pressed={provider === id}
                  onClick={() => choose(id)}
                >
                  <Bot size={22} aria-hidden />
                  <div>
                    <strong>{names[id]}</strong>
                    <span>{p.settings.model}</span>
                  </div>
                  <span className="ai-provider-context">
                    {p.settings.context_window_tokens / 1000}K
                  </span>
                  <span className={`ai-status ${status?.connected ? 'good' : 'muted'}`}>
                    {status
                      ? status.connected
                        ? 'Подключён'
                        : 'Не подключён'
                      : 'Состояние недоступно'}
                  </span>
                  {profile?.provider === id && <span className="ai-active-mark">Активный</span>}
                </button>
              )
            })}
          </div>
          <div className="ai-detail-header">
            <h2>{names[provider]}</h2>
          </div>
          <div className="ai-settings-grid">
            <Section title="Подключение">
              <p className="ai-note">
                {provider === 'chatgpt'
                  ? 'Вход через подписку ChatGPT. API-ключ OpenAI не нужен.'
                  : 'Ключ хранится в AI-runtime и доступен только на запись.'}
              </p>
              <dl className="ai-facts">
                <dt>Состояние</dt>
                <dd>
                  {runtime ? (runtime.connected ? 'Подключён' : 'Не подключён') : 'Недоступно'}
                </dd>
                <dt>{provider === 'chatgpt' ? 'Авторизация' : 'Адрес API'}</dt>
                <dd>
                  {provider === 'chatgpt' ? 'ChatGPT subscription' : 'https://openrouter.ai/api/v1'}
                </dd>
              </dl>
              {runtime?.connected ? (
                <Button
                  variant="outline"
                  disabled={busy || !!pending}
                  onClick={() => setDisconnectOpen(true)}
                >
                  Отключить подключение
                </Button>
              ) : provider === 'chatgpt' ? (
                <Button
                  disabled={busy || !!pending || !runtime?.runtime_available}
                  onClick={() => void run(startLogin)}
                >
                  Войти через ChatGPT
                </Button>
              ) : (
                <form
                  className="ai-form"
                  onSubmit={(event) => {
                    event.preventDefault()
                    void run(connectOpenRouter)
                  }}
                >
                  <label htmlFor="ai-key">Ключ OpenRouter</label>
                  <Input
                    id="ai-key"
                    type="password"
                    autoComplete="off"
                    value={key}
                    disabled={busy || !!pending || !runtime}
                    onChange={(event) => setKey(event.target.value)}
                  />
                  <Button type="submit" disabled={busy || !!pending || !runtime || !key.trim()}>
                    Сохранить подключение
                  </Button>
                </form>
              )}
              {login && !terminal.has(login.status) && (
                <Button variant="outline" onClick={() => setLoginOpen(true)}>
                  Продолжить вход
                </Button>
              )}
              {registry.runtime_error && (
                <p className="ai-help">Подключения временно недоступны.</p>
              )}
            </Section>
            <Section title="Модель и контекст">
              <div className="ai-form">
                <Button
                  variant="outline"
                  disabled={busy || !runtime?.connected}
                  onClick={() =>
                    void run(async () => {
                      const result = await ai.models(provider)
                      setCatalogs((current) => ({ ...current, [provider]: result }))
                    })
                  }
                >
                  <RefreshCw size={16} aria-hidden />
                  Обновить каталог
                </Button>
                <label htmlFor="ai-model-search">Поиск модели</label>
                <Input
                  id="ai-model-search"
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                />
                <label htmlFor="ai-model">Модель</label>
                <select
                  id="ai-model"
                  value={selected?.model ?? ''}
                  disabled={busy || !catalog}
                  onChange={(event) => chooseModel(event.target.value)}
                >
                  {selected &&
                    !catalog?.models.some((m) => (m.model ?? m.id) === selected.model) && (
                      <option value={selected.model}>
                        {selected.model} · доступ не подтверждён
                      </option>
                    )}
                  {catalog?.models
                    .filter(
                      (m) =>
                        (m.model ?? m.id) === selected?.model ||
                        `${m.id} ${m.name}`.toLowerCase().includes(search.toLowerCase()),
                    )
                    .map((m) => (
                      <option key={m.id} value={m.model ?? m.id}>
                        {m.name} · {m.model ?? m.id}
                      </option>
                    ))}
                </select>
                <label htmlFor="ai-context">Контекст, тыс. токенов</label>
                <Input
                  id="ai-context"
                  type="number"
                  min="64"
                  step="1"
                  disabled={busy}
                  value={contextText}
                  aria-invalid={!!contextError}
                  aria-describedby="ai-context-help"
                  onChange={(event) =>
                    selected &&
                    setContext((current) => ({
                      ...current,
                      [provider]: {
                        ...current[provider],
                        [selected.model]: event.target.value,
                      },
                    }))
                  }
                />
                <p className="ai-help" id="ai-context-help">
                  256 = 256 000 токенов. Бюджет включает вход и резерв ответа.
                  {model?.context_limit_tokens
                    ? ` Объявленный предел модели: ${model.context_limit_tokens.toLocaleString('ru-RU')}.`
                    : ' Предел модели пока не подтверждён.'}
                </p>
                {contextError && (
                  <p className="ai-error" role="alert">
                    {contextError}
                  </p>
                )}
                <div className="ai-actions">
                  <Button
                    variant="outline"
                    disabled={busy || !selected || !!contextError}
                    onClick={() => void run(save)}
                  >
                    Сохранить черновик
                  </Button>
                  <Button variant="outline" disabled>
                    Проверить модель
                  </Button>
                  <Button disabled>Сделать активным</Button>
                </div>
                <p className="ai-help">
                  Проверка модели пока недоступна. Публикация остаётся заблокированной до успешной
                  проверки.
                </p>
                <Button variant="outline" disabled={busy} onClick={() => void run(refresh)}>
                  Обновить ревизию, сохранив черновик
                </Button>
              </div>
            </Section>
          </div>
        </>
      )}
      <Dialog open={loginOpen} onOpenChange={setLoginOpen}>
        <DialogContent aria-describedby="ai-login-description">
          <DialogHeader>
            <DialogTitle>Подключить ChatGPT</DialogTitle>
          </DialogHeader>
          <p id="ai-login-description">
            Подтвердите вход в своём аккаунте. Не передавайте код другим людям.
          </p>
          <p>Состояние: {login?.status ?? 'Неизвестно'}</p>
          {login?.status === 'pending' &&
            login.verification_url === 'https://auth.openai.com/codex/device' && (
              <>
                <p>
                  <a href={login.verification_url} target="_blank" rel="noopener noreferrer">
                    Открыть подтверждение ChatGPT
                  </a>
                </p>
                <strong>{login.user_code}</strong>
                <p className="ai-help">
                  Действует до {new Date(login.expires_at).toLocaleTimeString('ru-RU')}.
                </p>
              </>
            )}
          {login && !terminal.has(login.status) && (
            <Button
              variant="outline"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  await ai.cancelLogin(login.operation_id)
                  setLogin(await ai.loginStatus(login.operation_id))
                  setPending(null)
                })
              }
            >
              Отменить вход
            </Button>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={disconnectOpen} onOpenChange={setDisconnectOpen}>
        <DialogContent aria-describedby="ai-disconnect-description">
          <DialogHeader>
            <DialogTitle>Отключить {names[provider]}</DialogTitle>
          </DialogHeader>
          <p id="ai-disconnect-description">
            Новые AI-запросы этого подключения будут недоступны. Автоматического переключения на
            другой провайдер нет.
          </p>
          <Button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                const id = crypto.randomUUID()
                setPending({ provider, id, kind: 'connection' })
                setDisconnectOpen(false)
                await ai.disconnect(provider, id)
                setPending(null)
                await refresh()
              })
            }
          >
            Отключить подключение
          </Button>
        </DialogContent>
      </Dialog>
    </>
  )
}
