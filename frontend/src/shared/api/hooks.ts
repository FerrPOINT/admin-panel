import { useQuery } from '@tanstack/react-query'
import { api } from './client'

export type ServiceStatus = 'pending' | 'active' | 'disabled' | 'retired'

export interface RegistryEntry {
  id: string
  service_key: string
  display_name: string
  owner_team: string
  status: ServiceStatus
  active_declaration_id: string | null
  created_at: string
  updated_at: string
  version: number
}

export interface Declaration {
  id: string
  registry_entry_id: string
  declaration_version: number
  integration_base_url: string
  capabilities: string[]
  service_contract_version: string
  declared_by_subject: string
  declared_at: string
  approval_status: 'pending' | 'approved' | 'rejected' | 'superseded'
  approved_by_subject: string | null
  approved_at: string | null
  content_hash: string
}

export interface BrandingDocument {
  product_name: string
  product_short_name: string
  logo_url: string | null
  favicon_url: string | null
  support_url: string | null
  primary_color: string
  accent_color: string
  surface_color: string | null
}

export interface BrandingRevision {
  id: string
  revision: number
  state: 'draft' | 'published' | 'superseded' | 'withdrawn'
  document: BrandingDocument
  document_hash: string
  etag: string
  created_by_subject: string
  created_at: string
  published_by_subject: string | null
  published_at: string | null
  based_on_revision: number | null
}

export interface AuditEvent {
  id: string
  occurred_at: string
  request_id: string
  actor_subject: string | null
  actor_role: string | null
  action: string
  entity_type: string
  entity_id: string | null
  metadata: Record<string, unknown>
}

export function useServices() {
  return useQuery({
    queryKey: ['services'],
    queryFn: () => api.get<{ services: RegistryEntry[]; total: number }>('/api/v1/services'),
  })
}

export function useService(serviceKey: string) {
  return useQuery({
    queryKey: ['service', serviceKey],
    queryFn: () =>
      api.get<{ service: RegistryEntry; declarations: Declaration[] }>(
        `/api/v1/services/${serviceKey}`,
      ),
  })
}

export function useBrandingRevisions() {
  return useQuery({
    queryKey: ['branding-revisions'],
    queryFn: () =>
      api.get<{ revisions: BrandingRevision[]; total: number }>('/api/v1/branding/revisions'),
  })
}

export function useAuditEvents(action?: string) {
  const params = action ? `?action=${encodeURIComponent(action)}` : ''
  return useQuery({
    queryKey: ['audit-events', action ?? 'all'],
    queryFn: () => api.get<{ events: AuditEvent[]; total: number }>(`/api/v1/audit-events${params}`),
  })
}
