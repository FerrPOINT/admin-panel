import { useLocation } from 'react-router'
import { parseNamespaceLocation } from '@sdlc/ui/lib'
import { useQuery } from '@tanstack/react-query'
import type { components } from './schema'
import { api } from './client'
export type Namespace = components['schemas']['Namespace']
export type NamespaceContext = components['schemas']['NamespaceContext']
export type NamespaceCommand = components['schemas']['NamespaceCommand']
export type OwnerInstance = components['schemas']['OwnerInstance']
export type Operation = components['schemas']['Operation']
export function useNamespaces(offset = 0) {
  return useQuery({
    queryKey: ['namespaces', offset],
    queryFn: ({ signal }) =>
      api.get<Namespace[]>(`/api/v1/namespaces?limit=50&offset=${offset}`, signal),
  })
}
export function useNamespace(id?: string) {
  const location = useLocation()
  const ref = parseNamespaceLocation(location.search)
  return useQuery({
    queryKey: ['namespace', id, ref?.registry_instance_id],
    enabled: Boolean(id),
    queryFn: ({ signal }) => api.get<NamespaceContext>(`/api/v1/namespaces/${id}/context`, signal),
  })
}
export function useNamespaceOwners() {
  return useQuery({
    queryKey: ['namespace-owners'],
    queryFn: ({ signal }) => api.get<OwnerInstance[]>('/api/v1/namespace-owners', signal),
  })
}
