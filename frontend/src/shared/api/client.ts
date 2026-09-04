const BASE = import.meta.env.VITE_API_BASE_URL ?? ''

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message)
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      ...init?.headers,
    },
  })
  if (!response.ok) {
    let code = 'HTTP_ERROR'
    let message = `HTTP ${response.status}`
    try {
      const body = await response.json()
      code = body?.error?.code ?? code
      message = body?.error?.message ?? message
    } catch {
      // non-JSON error body
    }
    throw new ApiError(response.status, code, message)
  }
  if (response.status === 204) return undefined as T
  return (await response.json()) as T
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) }),
  patch: <T>(path: string, body: unknown, etag?: string) =>
    request<T>(path, {
      method: 'PATCH',
      body: JSON.stringify(body),
      headers: etag ? { 'If-Match': etag } : undefined,
    }),
}
