import { useState, type FormEvent } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Copy, KeyRound, Plus, Trash2 } from 'lucide-react'
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

export function TokensPage() {
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [label, setLabel] = useState('')
  const [days, setDays] = useState(30)
  const [scopes, setScopes] = useState<string[]>([])
  const [issued, setIssued] = useState<IssuedToken | null>(null)
  const [revokeTarget, setRevokeTarget] = useState<PersonalToken | null>(null)
  const tokens = useQuery({ queryKey: ['personal-tokens'], queryFn: () => api.get<PersonalToken[]>('/api/v1/tokens') })
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
    {tokens.isPending && <p role="status" className="text-sm text-text-muted">Загружаем токены...</p>}
    {tokens.isError && <p role="alert" className="text-sm text-destructive">Не удалось загрузить токены. <Button variant="ghost" onClick={() => void tokens.refetch()}>Повторить</Button></p>}
    {tokens.data?.length === 0 && <p className="py-8 text-sm text-text-muted">Токенов пока нет.</p>}
    <div className="divide-y divide-border border-y border-border">
      {tokens.data?.map((token) => <div key={token.id} className="flex flex-wrap items-center justify-between gap-3 py-3">
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-2 text-sm font-medium"><KeyRound className="h-4 w-4" /> {token.label}</p>
          <p className="text-xs text-text-muted">{token.scopes.join(', ')} · До {new Date(token.expires_at).toLocaleDateString('ru-RU')}{token.revoked_at ? ' · Отозван' : ''}</p>
        </div>
        {!token.revoked_at && <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Отозвать ${token.label}`} title="Отозвать" onClick={() => setRevokeTarget(token)}><Trash2 className="h-4 w-4" /></Button>}
      </div>)}
    </div>
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
