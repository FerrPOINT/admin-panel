import { type FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { MoreHorizontal, Pencil, Plus, RotateCw, Search, UserRoundCheck, UserRoundX } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle, DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger, Input } from '@sdlc/ui/ui'
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
const PAGE_SIZE = 20
const BATCH_SIZE = 100

export function UsersPage() {
  const { session } = useAuth()
  const queryClient = useQueryClient()
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(0)
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<ManagedUser | null>(null)
  const [statusTarget, setStatusTarget] = useState<ManagedUser | null>(null)
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [deliveryFailedId, setDeliveryFailedId] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const batchOffset = Math.floor(page * PAGE_SIZE / BATCH_SIZE) * BATCH_SIZE
  const withinBatch = page * PAGE_SIZE - batchOffset
  const users = useQuery({
    queryKey: ['managed-users', search, batchOffset],
    queryFn: () => api.get<ManagedUser[]>(`/api/v1/users?q=${encodeURIComponent(search)}&offset=${batchOffset}`),
  })
  const needsNextBatch = users.isSuccess && users.data.length === BATCH_SIZE && withinBatch + PAGE_SIZE >= BATCH_SIZE
  const nextBatch = useQuery({
    queryKey: ['managed-users', search, batchOffset + BATCH_SIZE],
    queryFn: () => api.get<ManagedUser[]>(`/api/v1/users?q=${encodeURIComponent(search)}&offset=${batchOffset + BATCH_SIZE}`),
    enabled: needsNextBatch,
  })
  const visibleUsers = users.isSuccess ? users.data.slice(withinBatch, withinBatch + PAGE_SIZE) : []
  const hasNextPage = users.isSuccess && (
    withinBatch + PAGE_SIZE < users.data.length || (needsNextBatch && nextBatch.isSuccess && nextBatch.data.length > 0)
  )
  const refresh = () => void queryClient.invalidateQueries({ queryKey: ['managed-users'] })
  const save = useMutation({
    mutationFn: async () => {
      if (editing) {
        const user = await api.patch<ManagedUser>(`/api/v1/users/${editing.id}`, { display_name: displayName.trim() })
        return { kind: 'updated' as const, user }
      }
      if (deliveryFailedId) {
        await api.post<void>(`/api/v1/users/${deliveryFailedId}/password-link`)
        return { kind: 'resent' as const }
      }
      const user = await api.post<ManagedUser>('/api/v1/users', { email: email.trim(), display_name: displayName.trim() })
      return { kind: 'created' as const, user }
    },
    onMutate: () => setSaveError(null),
    onSuccess: (result) => {
      if (result.kind === 'created' && result.user.setup_delivery_status === 'failed') {
        setDeliveryFailedId(result.user.id)
        toast.error('Пользователь создан, но письмо не доставлено. Повторите отправку.')
        refresh()
        return
      }
      toast.success(result.kind === 'updated' ? 'Имя обновлено' : result.kind === 'resent' ? 'Ссылка отправлена' : 'Пользователь добавлен, письмо отправлено')
      setOpen(false); setEditing(null); setEmail(''); setDisplayName(''); setDeliveryFailedId(null); refresh()
    },
    onError: () => {
      setSaveError('Не удалось сохранить изменения. Проверьте данные и доставку письма.')
      toast.error('Не удалось сохранить пользователя. Проверьте данные и доставку письма.')
    },
  })
  const resend = useMutation({
    mutationFn: (id: string) => api.post<void>(`/api/v1/users/${id}/password-link`),
    onSuccess: () => { toast.success('Ссылка отправлена'); refresh() },
    onError: () => toast.error('Не удалось отправить ссылку'),
  })
  const changeStatus = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api.post<ManagedUser>(`/api/v1/users/${id}/status`, { enabled }),
    onSuccess: () => { toast.success('Статус изменён'); setStatusTarget(null); refresh() },
    onError: () => toast.error('Не удалось изменить статус'),
  })

  function openCreate() {
    setEditing(null); setEmail(''); setDisplayName(''); setDeliveryFailedId(null); setSaveError(null); setOpen(true)
  }
  function openEdit(user: ManagedUser) {
    setEditing(user); setEmail(user.email); setDisplayName(user.display_name); setDeliveryFailedId(null); setSaveError(null); setOpen(true)
  }
  function submit(event: FormEvent) {
    event.preventDefault()
    if (save.isPending) return
    if (!displayName.trim() || (!editing && !email.trim())) return
    save.mutate()
  }

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold">Пользователи</h1>
        <Button className="h-10" onClick={openCreate}><Plus className="h-4 w-4" /> Добавить</Button>
      </div>
      <label className="flex max-w-md items-center gap-2 rounded-md border border-border bg-surface px-3 focus-within:ring-2 focus-within:ring-focus">
        <Search className="h-4 w-4 shrink-0 text-text-muted" />
        <span className="sr-only">Поиск пользователей</span>
        <Input className="h-10 border-0 bg-transparent shadow-none" value={search} onChange={(event) => { setSearch(event.target.value); setPage(0) }} placeholder="Имя или email" />
      </label>
      {users.isPending && <p className="text-sm text-text-muted">Загружаем пользователей...</p>}
      {users.isError && <div role="alert" className="text-sm text-destructive">Не удалось загрузить пользователей. <Button variant="ghost" onClick={() => void users.refetch()}>Повторить</Button></div>}
      {users.isSuccess && users.data.length === 0 && <p className="py-8 text-center text-sm text-text-muted">{page > 0 ? 'На этой странице нет пользователей' : search ? 'Ничего не найдено' : 'Пользователей пока нет'}</p>}
      {visibleUsers.length > 0 && (
        <div className="divide-y divide-border border-y border-border">
          {visibleUsers.map((user) => (
            <div key={user.id} className="flex flex-wrap items-center justify-between gap-3 py-3">
              <div className="min-w-0 flex-1">
                <div className="flex min-w-0 items-center gap-2">
                  <p className="truncate text-sm font-medium">{user.display_name || user.email}</p>
                  <span className="shrink-0 text-xs text-text-muted">{statusLabels[user.status]}</span>
                </div>
                <p className="truncate text-xs text-text-muted">{user.email}{user.setup_delivery_status === 'failed' ? ' · Письмо не доставлено' : ''}</p>
              </div>
              <div className="hidden items-center gap-1 md:flex">
                <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Изменить имя: ${user.email}`} title="Изменить имя" onClick={() => openEdit(user)}><Pencil className="h-4 w-4" /></Button>
                {user.status !== 'disabled' && <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`Отправить ссылку: ${user.email}`} title="Отправить ссылку" disabled={resend.isPending} onClick={() => resend.mutate(user.id)}><RotateCw className="h-4 w-4" /></Button>}
                <Button variant="ghost" size="icon" className="h-10 w-10" aria-label={`${user.status === 'disabled' ? 'Восстановить' : 'Отключить'}: ${user.email}`} title={user.status === 'disabled' ? 'Восстановить' : 'Отключить'} disabled={user.id === session?.subject || changeStatus.isPending} onClick={() => setStatusTarget(user)}>
                  {user.status === 'disabled' ? <UserRoundCheck className="h-4 w-4" /> : <UserRoundX className="h-4 w-4" />}
                </Button>
              </div>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon" className="h-10 w-10 md:hidden" aria-label={`Действия с пользователем: ${user.email}`} title="Действия с пользователем"><MoreHorizontal className="h-4 w-4" /></Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem className="min-h-10" onSelect={() => openEdit(user)}><Pencil className="mr-2 h-4 w-4" />Изменить имя</DropdownMenuItem>
                  {user.status !== 'disabled' && <DropdownMenuItem className="min-h-10" disabled={resend.isPending} onSelect={() => resend.mutate(user.id)}><RotateCw className="mr-2 h-4 w-4" />Отправить ссылку</DropdownMenuItem>}
                  <DropdownMenuItem className="min-h-10" disabled={user.id === session?.subject || changeStatus.isPending} onSelect={() => setStatusTarget(user)}>{user.status === 'disabled' ? <UserRoundCheck className="mr-2 h-4 w-4" /> : <UserRoundX className="mr-2 h-4 w-4" />}{user.status === 'disabled' ? 'Восстановить' : 'Отключить'}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          ))}
        </div>
      )}
      <div className="flex items-center justify-end gap-3">
        <Button variant="outline" className="h-10" disabled={page === 0 || users.isPending} onClick={() => setPage((current) => Math.max(0, current - 1))}>Назад</Button>
        <span className="text-xs text-text-muted">Страница {page + 1}</span>
        <Button variant="outline" className="h-10" disabled={!hasNextPage} onClick={() => setPage((current) => current + 1)}>Далее</Button>
      </div>
      {needsNextBatch && nextBatch.isError && <p role="alert" className="text-sm text-destructive">Не удалось проверить следующую страницу. <Button variant="ghost" className="h-10" onClick={() => void nextBatch.refetch()}>Повторить</Button></p>}

      <Dialog open={open} onOpenChange={(next) => { if (!save.isPending) setOpen(next) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{editing ? 'Изменить имя' : 'Добавить пользователя'}</DialogTitle></DialogHeader>
          <form className="space-y-4" onSubmit={submit} aria-busy={save.isPending}>
            <label className="block text-sm">Email
              <Input className="mt-1" type="email" autoComplete="email" required disabled={Boolean(editing) || Boolean(deliveryFailedId) || save.isPending} value={email} onChange={(event) => setEmail(event.target.value)} />
            </label>
            <label className="block text-sm">Имя
              <Input className="mt-1" required disabled={Boolean(deliveryFailedId) || save.isPending} value={displayName} onChange={(event) => setDisplayName(event.target.value)} />
            </label>
            {!editing && !deliveryFailedId && <p className="text-sm text-text-muted">Письмо со ссылкой для установки пароля будет отправлено на этот адрес.</p>}
            {deliveryFailedId && <p role="alert" className="text-sm text-destructive">Учётка создана, письмо не доставлено. Проверьте SMTP и повторите отправку.</p>}
            {saveError && <p role="alert" className="text-sm text-destructive">{saveError}</p>}
            <DialogFooter><Button type="submit" disabled={save.isPending}>{save.isPending ? 'Сохраняем...' : editing ? 'Сохранить' : deliveryFailedId ? 'Повторить отправку' : 'Добавить'}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      <Dialog open={Boolean(statusTarget)} onOpenChange={(next) => { if (!next && !changeStatus.isPending) setStatusTarget(null) }}>
        <DialogContent>
          <DialogHeader><DialogTitle>{statusTarget?.status === 'disabled' ? 'Восстановить пользователя?' : 'Отключить пользователя?'}</DialogTitle></DialogHeader>
          <p className="text-sm text-text-muted">{statusTarget?.email}</p>
          <DialogFooter><Button variant="outline" disabled={changeStatus.isPending} onClick={() => setStatusTarget(null)}>Отмена</Button><Button disabled={changeStatus.isPending} onClick={() => statusTarget && changeStatus.mutate({ id: statusTarget.id, enabled: statusTarget.status === 'disabled' })}>{changeStatus.isPending ? 'Сохраняем...' : 'Подтвердить'}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
