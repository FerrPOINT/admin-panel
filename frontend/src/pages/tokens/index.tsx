import { useState, type FormEvent } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronLeft, ChevronRight, Copy, KeyRound, Plus, Search, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, Input } from '@sdlc/ui/ui'
import { api } from '@/shared/api/client'

interface PersonalToken {
  id: string
  label: string
  scopes: string[]
  expires_at: string
  created_at: string
  last_used_at: string | null
  revoked_at: string | null
}
interface IssuedToken extends PersonalToken { secret: string }

const services = [
  ['admin-panel', 'Admin Panel'], ['ci-cd', 'CI/CD'], ['task-tracker', 'Task Tracker'],
  ['wiki', 'Wiki'], ['fleet-control', 'Fleet Control'], ['project-workflow', 'Project Workflow'],
] as const
const PAGE_SIZE = 10
type TokenState = 'all' | 'active' | 'expired' | 'revoked'

function tokenState(token: PersonalToken): Exclude<TokenState, 'all'> {
  if (token.revoked_at) return 'revoked'
  if (new Date(token.expires_at).getTime() <= Date.now()) return 'expired'
  return 'active'
}

const stateLabels: Record<Exclude<TokenState, 'all'>, string> = {
  active: 'Действует', expired: 'Истёк', revoked: 'Отозван',
}

export function TokensPage() {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [label, setLabel] = useState('')
  const [days, setDays] = useState(30)
  const [scopes, setScopes] = useState<string[]>([])
  const [issued, setIssued] = useState<IssuedToken | null>(null)
  const [revokeTarget, setRevokeTarget] = useState<PersonalToken | null>(null)
  const [search, setSearch] = useState('')
  const [stateFilter, setStateFilter] = useState<TokenState>('all')
  const [page, setPage] = useState(0)
  const tokens = useQuery({ queryKey: ['personal-tokens'], queryFn: () => api.get<PersonalToken[]>('/api/v1/tokens') })
  const filteredTokens = (tokens.data ?? []).filter((token) =>
    token.label.toLocaleLowerCase('ru-RU').includes(search.trim().toLocaleLowerCase('ru-RU'))
    && (stateFilter === 'all' || tokenState(token) === stateFilter),
  )
  const visibleTokens = filteredTokens.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE)
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ['personal-tokens'] })
  const create = useMutation({
    mutationFn: () => api.post<IssuedToken>('/api/v1/tokens', { label: label.trim(), scopes, expires_in_days: days }),
    onSuccess: (token) => { setIssued(token); setOpen(false); setLabel(''); setScopes([]); refresh() },
    onError: () => toast.error('Не удалось создать токен. Проверьте название, срок и доступы.'),
  })
  const revoke = useMutation({
    mutationFn: (id: string) => api.delete<void>(`/api/v1/tokens/${id}`),
    onSuccess: () => { setRevokeTarget(null); refresh(); toast.success('Токен отозван') },
    onError: () => toast.error('Не удалось отозвать токен'),
  })
  function toggle(scope: string) {
    setScopes((current) => current.includes(scope) ? current.filter((item) => item !== scope) : [...current, scope])
  }
  function submit(event: FormEvent) {
    event.preventDefault()
    if (label.trim() && scopes.length && days >= 1 && days <= 365) create.mutate()
  }
  return <div className="space-y-5">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h1 className="text-xl font-semibold">Личные API-токены</h1>
      <Button onClick={() => setOpen(true)}><Plus className="h-4 w-4" /> Создать</Button>
    </div>
    <div className="flex flex-wrap items-center gap-2">
      <label className="flex min-h-10 max-w-sm flex-1 items-center gap-2 rounded-md border border-border bg-surface px-3 focus-within:ring-2 focus-within:ring-accent">
        <Search className="h-4 w-4 shrink-0 text-text-muted" aria-hidden="true" />
        <span className="sr-only">Поиск токенов</span>
        <Input className="h-10 border-0 bg-transparent shadow-none" value={search} onChange={(event) => { setSearch(event.target.value); setPage(0) }} placeholder="Название токена" />
      </label>
      <select aria-label="Статус токена" className="min-h-10 rounded-md border border-border bg-surface px-3 text-sm" value={stateFilter} onChange={(event) => { setStateFilter(event.target.value as TokenState); setPage(0) }}>
        <option value="all">Все статусы</option>
        <option value="active">Действующие</option>
        <option value="expired">Истёкшие</option>
        <option value="revoked">Отозванные</option>
      </select>
    </div>
    {tokens.isPending && <p role="status" className="text-sm text-text-muted">Загружаем токены...</p>}
    {tokens.isError && <p role="alert" className="text-sm text-destructive">Не удалось загрузить токены. <Button variant="ghost" onClick={() => void tokens.refetch()}>Повторить</Button></p>}
    {tokens.data?.length === 0 && <p className="py-8 text-sm text-text-muted">Токенов пока нет.</p>}
    {tokens.data && tokens.data.length > 0 && filteredTokens.length === 0 && <p className="py-8 text-sm text-text-muted">Токены по выбранным фильтрам не найдены.</p>}
    <div className="divide-y divide-border border-y border-border">
      {visibleTokens.map((token) => <div key={token.id} className="flex flex-wrap items-center justify-between gap-3 py-3">
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-2 text-sm font-medium"><KeyRound className="h-4 w-4" /> {token.label}</p>
          <p className="text-xs text-text-muted">{token.scopes.join(', ')} · До {new Date(token.expires_at).toLocaleDateString('ru-RU')} · {stateLabels[tokenState(token)]}</p>
        </div>
        {!token.revoked_at && <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Отозвать ${token.label}`} title="Отозвать" onClick={() => setRevokeTarget(token)}><Trash2 className="h-4 w-4" /></Button>}
      </div>)}
    </div>
    {filteredTokens.length > PAGE_SIZE && <div className="flex items-center justify-between gap-2">
      <Button variant="outline" className="h-10" disabled={page === 0} onClick={() => setPage((current) => Math.max(0, current - 1))}><ChevronLeft className="h-4 w-4" /> Назад</Button>
      <span className="text-center text-xs text-text-muted">{page * PAGE_SIZE + 1}–{Math.min((page + 1) * PAGE_SIZE, filteredTokens.length)} из {filteredTokens.length}</span>
      <Button variant="outline" className="h-10" disabled={(page + 1) * PAGE_SIZE >= filteredTokens.length} onClick={() => setPage((current) => current + 1)}>Далее <ChevronRight className="h-4 w-4" /></Button>
    </div>}
    <Dialog open={open} onOpenChange={(next) => { if (!create.isPending) setOpen(next) }}><DialogContent>
      <DialogHeader><DialogTitle>Новый токен</DialogTitle></DialogHeader>
      <form className="space-y-4" onSubmit={submit}>
        <label className="block text-sm">Название<Input className="mt-1" required maxLength={100} value={label} onChange={(event) => setLabel(event.target.value)} /></label>
        <label className="block text-sm">Срок действия, дней<Input className="mt-1" type="number" min={1} max={365} required value={days} onChange={(event) => setDays(Number(event.target.value))} /></label>
        <fieldset className="space-y-2"><legend className="text-sm font-medium">Доступы</legend>
          {services.map(([key, name]) => <div key={key} className="flex flex-wrap items-center gap-4 text-sm"><span className="w-36">{name}</span>
            {(['read', 'write'] as const).map((action) => <label key={action} className="flex items-center gap-2"><input type="checkbox" checked={scopes.includes(`${key}:${action}`)} onChange={() => toggle(`${key}:${action}`)} />{action === 'read' ? 'Чтение' : 'Запись'}</label>)}
          </div>)}
        </fieldset>
        <DialogFooter><Button type="submit" disabled={create.isPending || !scopes.length}>{create.isPending ? 'Создаём...' : 'Создать'}</Button></DialogFooter>
      </form>
    </DialogContent></Dialog>
    <Dialog open={Boolean(issued)} onOpenChange={(next) => { if (!next) setIssued(null) }}><DialogContent>
      <DialogHeader><DialogTitle>Секрет токена</DialogTitle></DialogHeader>
      <p className="text-sm text-text-muted">Секрет показывается только сейчас. Он не будет доступен после закрытия.</p>
      <code className="block break-all rounded border border-border bg-surface-raised p-3 text-xs select-all">{issued?.secret}</code>
      <DialogFooter><Button variant="outline" onClick={() => issued && void navigator.clipboard.writeText(issued.secret).then(() => toast.success('Скопировано')).catch(() => toast.error('Не удалось скопировать'))}><Copy className="h-4 w-4" /> Скопировать</Button><Button onClick={() => setIssued(null)}>Готово</Button></DialogFooter>
    </DialogContent></Dialog>
    <Dialog open={Boolean(revokeTarget)} onOpenChange={(next) => { if (!next && !revoke.isPending) setRevokeTarget(null) }}><DialogContent>
      <DialogHeader><DialogTitle>Отозвать токен?</DialogTitle></DialogHeader>
      <p className="text-sm text-text-muted">{revokeTarget?.label} перестанет работать сразу.</p>
      <DialogFooter><Button variant="outline" onClick={() => setRevokeTarget(null)}>Отмена</Button><Button variant="destructive" disabled={revoke.isPending} onClick={() => revokeTarget && revoke.mutate(revokeTarget.id)}>{revoke.isPending ? 'Отзываем...' : 'Отозвать'}</Button></DialogFooter>
    </DialogContent></Dialog>
  </div>
}
