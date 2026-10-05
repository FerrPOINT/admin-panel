import { requestResponse } from './client'

export type ProviderId = 'chatgpt' | 'openrouter'
export type Settings = { provider: ProviderId; model: string; context_window_tokens: number }
export type Draft = {
  settings: Settings
  draft_revision: number
  updated_at: string
  model_contexts?: { model: string; context_window_tokens: number }[]
}
export type Registry = {
  schema_version: 1
  providers: Draft[]
  runtime: null | {
    providers: {
      id: ProviderId
      connected: boolean
      runtime_available?: boolean
      pending_login_operation?: string | null
      capabilities_verified: boolean
    }[]
  }
  runtime_error: string | null
}
export type Profile = Settings & { revision: number; workspace: string }
export type Login = {
  operation_id: string
  login_id: string | null
  status: string
  user_code: string | null
  verification_url: string | null
  expires_at: string
}
export type Catalog = {
  provider: ProviderId
  access_verified: false
  models: {
    id: string
    model?: string
    name: string
    context_limit_tokens: number | null
    max_output_tokens: number | null
  }[]
}
export type Operation = {
  operation_id: string
  provider: ProviderId
  kind: 'credentials' | 'disconnect'
  status: 'pending' | 'uncertain' | 'completed'
}

export type AcceptanceBudget = {
  schema_version: 1
  workspace: string
  currency: 'USD'
  limit_microdollars: string
  settled_microdollars: string
  reserved_microdollars: string
  uncertain_microdollars: string
  available_microdollars: string
  unsettled_requests: number
  uncertain_requests: number
  blocked_reason: 'ai_acceptance_budget_exhausted' | 'provider_cost_exceeds_reservation' | null
}

export function dollarAmount(microdollars: string): string {
  if (!/^(0|[1-9]\d{0,19})$/.test(microdollars)) return 'Недоступно'
  const amount = BigInt(microdollars)
  if (amount > 18446744073709551615n) return 'Недоступно'
  const fraction = (amount % 1000000n).toString().padStart(6, '0').replace(/0+$/, '')
  return `$${amount / 1000000n}${fraction ? `.${fraction}` : '.00'}`
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await requestResponse(path, { ...init, cache: 'no-store' })
  return response.status === 204 ? (undefined as T) : (response.json() as Promise<T>)
}

export const ai = {
  budget: () => call<AcceptanceBudget>('/api/v1/ai/budget'),
  registry: () => call<Registry>('/api/v1/ai/providers'),
  selection: () => call<{ configured: boolean; profile: Profile | null }>('/api/v1/ai/selection'),
  models: (provider: ProviderId) => call<Catalog>(`/api/v1/ai/providers/${provider}/models`),
  save: (settings: Settings, revision: number) =>
    call<Draft>(`/api/v1/ai/providers/${settings.provider}`, {
      method: 'PUT',
      headers: { 'If-Match': `"${revision}"` },
      body: JSON.stringify(settings),
    }),
  credentials: (credential: string, operation: string) =>
    call('/api/v1/ai/providers/openrouter/credentials', {
      method: 'PUT',
      body: JSON.stringify({ credential, operation_id: operation }),
    }),
  login: (operation: string) =>
    call<Login>('/api/v1/ai/providers/chatgpt/login', {
      method: 'POST',
      headers: { 'Idempotency-Key': operation },
    }),
  loginStatus: (operation: string) =>
    call<Login>(`/api/v1/ai/providers/chatgpt/login/${operation}`),
  cancelLogin: (operation: string) =>
    call<void>(`/api/v1/ai/providers/chatgpt/login/${operation}`, { method: 'DELETE' }),
  disconnect: (provider: ProviderId, operation: string) =>
    call<void>(`/api/v1/ai/providers/${provider}/connection`, {
      method: 'DELETE',
      headers: { 'Idempotency-Key': operation },
    }),
  operation: (provider: ProviderId, operation: string) =>
    call<Operation>(`/api/v1/ai/providers/${provider}/operations/${operation}`),
}

export function contextTokens(thousands: string): number | null {
  if (!/^\d+$/.test(thousands)) return null
  const value = Number(thousands) * 1000
  return Number.isSafeInteger(value) && value >= 64000 && value <= 4294967295 ? value : null
}

export function modelContextText(
  draft: Draft | undefined,
  model: string,
  unsaved: Record<string, string> = {},
): string {
  if (Object.hasOwn(unsaved, model)) return unsaved[model]
  const remembered = draft?.model_contexts?.find((entry) => entry.model === model)
  const tokens =
    remembered?.context_window_tokens ??
    (draft?.settings.model === model ? draft.settings.context_window_tokens : 256000)
  return String(tokens / 1000)
}
