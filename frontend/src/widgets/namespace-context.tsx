import { useLocation, useNavigate } from 'react-router'
import { NamespacePicker } from '@sdlc/ui/ui'
import { parseNamespaceLocation, withNamespaceLocation } from '@sdlc/ui/lib'
import { useNamespaces } from '@/shared/api/namespaces'
export function NamespaceShellContext() {
  const location = useLocation()
  const navigate = useNavigate()
  const catalog = useNamespaces()
  const ref = parseNamespaceLocation(location.search)
  const value = ref ? `${ref.registry_instance_id}/${ref.namespace_id}` : ''
  return (
    <NamespacePicker
      value={value}
      loading={catalog.isPending}
      unavailable={catalog.isError}
      options={(catalog.data ?? []).map((n) => ({
        value: `${n.registry_instance_id}/${n.id}`,
        label: `${n.name} · ${n.slug}`,
      }))}
      manageUrl={
        ref ? withNamespaceLocation(`/namespaces/${ref.namespace_id}`, ref) : '/namespaces'
      }
      onChange={(next) => {
        const [registry_instance_id, namespace_id] = next.split('/')
        navigate(
          withNamespaceLocation(
            next ? `/namespaces/${namespace_id}` : '/namespaces',
            next ? { registry_instance_id, namespace_id } : null,
          ),
        )
      }}
    />
  )
}
