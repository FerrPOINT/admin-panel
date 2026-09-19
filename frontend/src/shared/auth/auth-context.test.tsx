import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { AuthProvider, authToken, useAuth } from './auth-context'

function Probe() {
  const { status, session, canMutate, canManageBindings, acceptSso } = useAuth()
  return (
    <div>
      <span data-testid="status">{status}</span>
      <span data-testid="role">{session?.panelRole ?? 'none'}</span>
      <span data-testid="mutate">{String(canMutate)}</span>
      <span data-testid="bindings">{String(canManageBindings)}</span>
      <button onClick={() => void acceptSso({ accessToken: 'tok-1', expiresAt: Date.now() + 60_000, subject: 'u-1', email: 'admin@base.local', name: 'Admin', returnTo: '/' })}>Accept SSO</button>
    </div>
  )
}

function loginResponse(me = false) {
  const body = me
    ? { subject: 'u-1', email: 'admin@base.local', central_role: 'member', panel_role: 'platform_admin', capabilities: {} }
    : { access_token: 'tok-1', token_type: 'Bearer', expires_in: 900, subject: 'u-1', central_role: 'member', panel_role: 'platform_viewer' }
  return { ok: true, status: 200, json: async () => body }
}

describe('AuthProvider', () => {
  beforeEach(() => {
    sessionStorage.clear()
    vi.stubGlobal('fetch', vi.fn())
  })
  afterEach(() => {
    act(() => window.dispatchEvent(new Event('base-admin:unauthorized')))
    cleanup()
    vi.unstubAllGlobals()
  })

  it('starts anonymous without a stored token', async () => {
    render(<AuthProvider><Probe /></AuthProvider>)
    await waitFor(() => expect(screen.getByTestId('status').textContent).toBe('anonymous'))
    expect(screen.getByTestId('role').textContent).toBe('none')
    expect(screen.getByTestId('mutate').textContent).toBe('false')
  })

  it('accepts a verified SSO session via /auth/me without browser storage', async () => {
    vi.mocked(fetch).mockResolvedValue(loginResponse(true) as unknown as Response)
    render(<AuthProvider><Probe /></AuthProvider>)
    fireEvent.click(screen.getByRole('button', { name: 'Accept SSO' }))
    await waitFor(() => expect(screen.getByTestId('status').textContent).toBe('authenticated'))
    expect(screen.getByTestId('role').textContent).toBe('platform_admin')
    expect(screen.getByTestId('mutate').textContent).toBe('true')
    expect(screen.getByTestId('bindings').textContent).toBe('false')
    expect(authToken()).toBe('tok-1')
    expect(sessionStorage.getItem('base.admin.token')).toBeNull()
    const [url, init] = vi.mocked(fetch).mock.calls[0]
    expect(url).toContain('/api/v1/auth/me')
    expect((init?.headers as Record<string, string>).Authorization).toBe('Bearer tok-1')
  })

  it('discards a legacy stored token without using it', async () => {
    sessionStorage.setItem('base.admin.token', 'stale')
    render(<AuthProvider><Probe /></AuthProvider>)
    await waitFor(() => expect(screen.getByTestId('status').textContent).toBe('anonymous'))
    expect(sessionStorage.getItem('base.admin.token')).toBeNull()
    expect(fetch).not.toHaveBeenCalled()
  })
})
