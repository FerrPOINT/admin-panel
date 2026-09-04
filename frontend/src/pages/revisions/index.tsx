import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Eye, Send } from 'lucide-react'
import { api } from '@/shared/api/client'
import { useBrandingRevisions } from '@/shared/api/hooks'

export function RevisionsPage() {
  const revisions = useBrandingRevisions()
  const queryClient = useQueryClient()
  const publish = useMutation({
    mutationFn: (revision: number) => api.post(`/api/v1/branding/revisions/${revision}/publish`),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['branding-revisions'] }),
  })
  return <div className="space-y-6"><div><h1 className="text-xl font-semibold">Конфигурации</h1><p className="mt-1 text-sm text-text-muted">Неизменяемая история брендинга. Откат создаёт новую публикацию, а не переписывает прошлое.</p></div>
    <div className="overflow-hidden rounded-lg border border-border bg-surface"><div className="hidden grid-cols-[80px_1fr_140px_180px_120px] gap-4 border-b border-border px-4 py-3 text-xs text-text-muted md:grid"><span>Версия</span><span>Название</span><span>Статус</span><span>Создана</span><span /></div>
    {revisions.isLoading ? <div className="p-5 text-sm text-text-muted">Загрузка версий...</div> : null}
    {revisions.data?.revisions.map((revision) => <div key={revision.id} className="grid gap-2 border-b border-border px-4 py-4 text-sm last:border-0 md:grid-cols-[80px_1fr_140px_180px_120px] md:items-center md:gap-4"><span className="font-mono text-text-secondary">v{revision.revision}</span><span>{revision.document.product_name}</span><span className={revision.state === 'published' ? 'text-success' : revision.state === 'draft' ? 'text-warning' : 'text-text-muted'}>{revision.state}</span><span className="text-text-muted">{new Date(revision.created_at).toLocaleString('ru-RU')}</span><span className="flex gap-2">{revision.state === 'draft' ? <button onClick={() => publish.mutate(revision.revision)} disabled={publish.isPending} className="inline-flex items-center gap-1 rounded border border-border px-2 py-1 text-xs hover:bg-surface-raised"><Send className="h-3.5 w-3.5" />Publish</button> : <span className="inline-flex items-center gap-1 text-xs text-text-muted"><Eye className="h-3.5 w-3.5" />Read-only</span>}</span></div>)}
    {revisions.data?.revisions.length === 0 ? <div className="p-8 text-center text-sm text-text-muted">Черновиков и опубликованных ревизий пока нет.</div> : null}</div>
  </div>
}
