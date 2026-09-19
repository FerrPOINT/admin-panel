import { type FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Pencil, Plus, RotateCw, Search, UserRoundCheck, UserRoundX } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, Input } from '@sdlc/ui/ui'
import { api } from '@/shared/api/client'
import { useAuth } from '@/shared/auth/auth-context'

interface ManagedUser {
  id: string
  email: string
  username: string
  display_name: string
  status: 'pending' | 'active' | 'disabled'
  setup_delivery_status: 'not_requested' | 'sent' | 'failed'
}

const statusLabels: Record<ManagedUser['status'], string> = {
  pending: 'Ожидает пароль', active: 'Активен', disabled: 'Отключён',
}

export function UsersPage() {
  const { session } = useAuth()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [offset, setOffset] = useState(0)
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<ManagedUser | null>(null)
  const [statusTarget, setStatusTarget] = useState<ManagedUser | null>(null)
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [deliveryFailedId, setDeliveryFailedId] = useState<string | null>(null)
  const users = useQuery({
    queryKey: ['managed-users', search, offset],
    queryFn: () => api.get<ManagedUser[]>(`/api/v1/users?q=${encodeURIComponent(search)}&offset=${offset}`),
  })
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ['managed-users'] })
  const save = useMutation({
    mutationFn: async () => {
      if (editing) return api.patch<ManagedUser>(`/api/v1/users/${editing.id}`, { display_name: displayName.trim() })
      if (deliveryFailedId) {
        await api.post<void>(`/api/v1/users/${deliveryFailedId}/password-link`)
        return null
      }
      return api.post<ManagedUser>('/api/v1/users', { email: email.trim(), display_name: displayName.trim() })
    },
    onSuccess: (result) => {
      if (result?.setup_delivery_status === 'failed') {
        setDeliveryFailedId(result.id)
        toast.error('Пользователь создан, но письмо не доставлено. Повторите отправку.')
        refresh()
        return
      }
      toast.success(editing ? 'Имя обновлено' : 'Пользователь добавлен, письмо отправлено')
      setOpen(false); setEditing(null); setEmail(''); setDisplayName(''); setDeliveryFailedId(null); refresh()
    },
    onError: () => toast.error('Не удалось сохранить пользователя. Проверьте данные и доставку письма.'),
  })
  const resend = useMutation({
    mutationFn: (id: string) => api.post<void>(`/api/v1/users/${id}/password-link`),
    onSuccess: () => toast.success('Ссылка отправлена'),
    onError: () => toast.error('Не удалось отправить ссылку'),
  })
  const changeStatus = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api.post<ManagedUser>(`/api/v1/users/${id}/status`, { enabled }),
    onSuccess: () => { toast.success('Статус изменён'); setStatusTarget(null); refresh() },
    onError: () => toast.error('Не удалось изменить статус'),
  })

  function openCreate() {
    setEditing(null); setEmail(''); setDisplayName(''); setDeliveryFailedId(null); setOpen(true)
  }
  function openEdit(user: ManagedUser) {
    setEditing(user); setEmail(user.email); setDisplayName(user.display_name); setDeliveryFailedId(null); setOpen(true)
  }
  function submit(event: FormEvent) {
    event.preventDefault()
    if (!displayName.trim() || (!editing && !email.trim())) return
    save.mutate()
  }

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold">Пользователи</h1>
        <Button onClick={openCreate}><Plus className="h-4 w-4" /> Добавить</Button>
      </div>
      <label className="flex max-w-md items-center gap-2 rounded-md border border-border bg-surface px-3 focus-within:ring-2 focus-within:ring-focus">
        <Search className="h-4 w-4 shrink-0 text-text-muted" />
        <span className="sr-only">Поиск пользователей</span>
        <Input className="border-0 bg-transparent shadow-none" value={search} onChange={(event) => { setSearch(event.target.value); setOffset(0) }} placeholder="Имя или email" />
      </label>
      {users.isPending && <p className="text-sm text-text-muted">Загружаем пользователей...</p>}
      {users.isError && <div role="alert" className="text-sm text-destructive">Не удалось загрузить пользователей. <Button variant="ghost" onClick={() => void users.refetch()}>Повторить</Button></div>}
      {users.data && users.data.length === 0 && <p className="py-8 text-center text-sm text-text-muted">{search ? 'Ничего не найдено' : 'Пользователей пока нет'}</p>}
      {users.data && users.data.length > 0 && (
        <div className="divide-y divide-border border-y border-border">
          {users.data.map((user) => (
            <div key={user.id} className="flex flex-wrap items-center justify-between gap-3 py-3">
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{user.display_name || user.email}</p>
                <p className="truncate text-xs text-text-muted">{user.email} · {statusLabels[user.status]}{user.setup_delivery_status === 'failed' ? ' · Письмо не доставлено' : ''}</p>
              </div>
              <div className="flex items-center gap-1">
                <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Изменить имя: ${user.email}`} title="Изменить имя" onClick={() => openEdit(user)}><Pencil className="h-4 w-4" /></Button>
                {user.status !== 'disabled' && <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Отправить ссылку: ${user.email}`} title="Отправить ссылку" disabled={resend.isPending} onClick={() => resend.mutate(user.id)}><RotateCw className="h-4 w-4" /></Button>}
                <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`${user.status === 'disabled' ? 'Восстановить' : 'Отключить'}: ${user.email}`} title={user.status === 'disabled' ? 'Восстановить' : 'Отключить'} disabled={user.id === session?.subject || changeStatus.isPending} onClick={() => setStatusTarget(user)}>
                  {user.status === 'disabled' ? <UserRoundCheck className="h-4 w-4" /> : <UserRoundX className="h-4 w-4" />}
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
      <div className="flex justify-end gap-2">
        <Button variant="outline" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - 100))}>Назад</Button>
        <Button variant="outline" disabled={(users.data?.length ?? 0) < 100} onClick={() => setOffset(offset + 100)}>Далее</Button>
      </div>

      <Dialog open={open} onOpenChange={(next) => { if (!save.isPending) setOpen(next) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{editing ? 'Изменить имя' : 'Добавить пользователя'}</DialogTitle></DialogHeader>
          <form className="space-y-4" onSubmit={submit}>
            <label className="block text-sm">Email
              <Input className="mt-1" type="email" autoComplete="email" required disabled={Boolean(editing)} value={email} onChange={(event) => setEmail(event.target.value)} />
            </label>
            <label className="block text-sm">Имя
              <Input className="mt-1" required value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
            </label>
            {!editing && <p className="text-sm text-text-muted">Письмо со ссылкой для установки пароля будет отправлено на этот адрес.</p>}
            {deliveryFailedId && <p role="alert" className="text-sm text-destructive">Учётка создана, письмо не доставлено. Проверьте SMTP и повторите отправку.</p>}
            <DialogFooter><Button type="submit" disabled={save.isPending}>{save.isPending ? 'Сохраняем...' : editing ? 'Сохранить' : deliveryFailedId ? 'Повторить отправку' : 'Добавить'}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={Boolean(statusTarget)} onOpenChange={(next) => { if (!next && !changeStatus.isPending) setStatusTarget(null) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{statusTarget?.status === 'disabled' ? 'Восстановить пользователя?' : 'Отключить пользователя?'}</DialogTitle></DialogHeader>
          <p className="text-sm text-text-muted">{statusTarget?.email}</p>
          <DialogFooter><Button variant="outline" onClick={() => setStatusTarget(null)}>Отмена</Button><Button disabled={changeStatus.isPending} onClick={() => statusTarget && changeStatus.mutate({ id: statusTarget.id, enabled: statusTarget.status === 'disabled' })}>{changeStatus.isPending ? 'Сохраняем...' : 'Подтвердить'}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
