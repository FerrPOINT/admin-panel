import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { Button, Input } from '@sdlc/ui/ui'
import { api } from '@/shared/api/client'
import { useAuth } from '@/shared/auth/auth-context'

interface RoleBinding {
  id: string
  claim_name: string
  claim_value: string
  panel_role: string
  created_by_subject: string
  created_at: string
}

const ROLES = ['platform_viewer', 'platform_operator', 'platform_admin'] as const
const CLAIMS = ['user_id', 'email', 'role'] as const

export function RoleBindingsPage() {
  const { canManageBindings } = useAuth()
  const queryClient = useQueryClient()
  const bindings = useQuery({
    queryKey: ['role-bindings'],
    queryFn: () => api.get<{ bindings: RoleBinding[] }>('/api/v1/role-bindings'),
  })
  const [claimName, setClaimName] = useState<string>('user_id')
  const [claimValue, setClaimValue] = useState('')
  const [panelRole, setPanelRole] = useState<string>('platform_operator')

  const create = useMutation({
    mutationFn: (body: { claim_name: string; claim_value: string; panel_role: string }) =>
      api.post('/api/v1/role-bindings', body),
    onSuccess: () => {
      toast.success('Биндинг создан')
      setClaimValue('')
      void queryClient.invalidateQueries({ queryKey: ['role-bindings'] })
    },
    onError: (error) => toast.error(error instanceof Error ? error.message : 'Не удалось создать'),
  })

  const remove = useMutation({
    mutationFn: (id: string) => api.delete(`/api/v1/role-bindings/${id}`),
    onSuccess: () => {
      toast.success('Биндинг удалён')
      void queryClient.invalidateQueries({ queryKey: ['role-bindings'] })
    },
    onError: (error) => toast.error(error instanceof Error ? error.message : 'Не удалось удалить'),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    create.mutate({ claim_name: claimName, claim_value: claimValue, panel_role: panelRole })
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Привязки ролей</h1>
        <p className="mt-1 text-sm text-text-muted">
          Локальные биндинги повышают panel-роль central-пользователей. Доступно только администраторам.
        </p>
      </div>

      {canManageBindings && (
        <form className="grid gap-3 rounded-lg border border-border bg-surface p-5 sm:grid-cols-[1fr_2fr_1fr_auto]" onSubmit={submit}>
          <label className="text-sm font-medium">
            Claim
            <select
              className="mt-1 w-full rounded-md border border-border bg-input px-3 py-2 text-sm"
              value={claimName}
              onChange={(e) => setClaimName(e.target.value)}
            >
              {CLAIMS.map((claim) => <option key={claim} value={claim}>{claim}</option>)}
            </select>
          </label>
          <label className="text-sm font-medium">
            Значение
            <Input className="mt-1" value={claimValue} onChange={(e) => setClaimValue(e.target.value)} placeholder="user uuid / email / role" required />
          </label>
          <label className="text-sm font-medium">
            Роль
            <select
              className="mt-1 w-full rounded-md border border-border bg-input px-3 py-2 text-sm"
              value={panelRole}
              onChange={(e) => setPanelRole(e.target.value)}
            >
              {ROLES.map((role) => <option key={role} value={role}>{role}</option>)}
            </select>
          </label>
          <div className="flex items-end">
            <Button type="submit" disabled={create.isPending || !claimValue.trim()}>
              {create.isPending ? 'Создаём...' : 'Создать'}
            </Button>
          </div>
        </form>
      )}

      <section className="overflow-hidden rounded-lg border border-border">
        {bindings.isPending ? (
          <p className="p-5 text-sm text-text-muted">Загрузка...</p>
        ) : bindings.isError ? (
          <p className="p-5 text-sm text-destructive">Не удалось загрузить биндинги: {bindings.error.message}</p>
        ) : (
          <table className="w-full text-sm">
            <thead className="bg-surface-raised text-left text-xs uppercase tracking-wide text-text-muted">
              <tr>
                <th className="px-4 py-3">Claim</th>
                <th className="px-4 py-3">Значение</th>
                <th className="px-4 py-3">Роль</th>
                <th className="px-4 py-3">Создан</th>
                {canManageBindings && <th className="px-4 py-3" />}
              </tr>
            </thead>
            <tbody>
              {(bindings.data?.bindings ?? []).map((binding) => (
                <tr key={binding.id} className="border-t border-border">
                  <td className="px-4 py-3 font-mono text-xs">{binding.claim_name}</td>
                  <td className="max-w-[22rem] truncate px-4 py-3 font-mono text-xs" title={binding.claim_value}>{binding.claim_value}</td>
                  <td className="px-4 py-3">{binding.panel_role}</td>
                  <td className="px-4 py-3 text-text-muted">{new Date(binding.created_at).toLocaleString('ru-RU')}</td>
                  {canManageBindings && (
                    <td className="px-4 py-3 text-right">
                      <Button variant="ghost" size="sm" aria-label="Удалить" disabled={remove.isPending} onClick={() => remove.mutate(binding.id)}>
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </td>
                  )}
                </tr>
              ))}
              {bindings.data?.bindings.length === 0 && (
                <tr><td colSpan={5} className="px-4 py-6 text-center text-text-muted">Биндингов пока нет</td></tr>
              )}
            </tbody>
          </table>
        )}
      </section>
    </div>
  )
}
