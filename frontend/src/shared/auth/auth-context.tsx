import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { endSso, type SsoSession } from '@sdlc/ui/sso'

export type PanelRole = 'platform_viewer' | 'platform_operator' | 'platform_admin'

type AuthStatus = 'loading' | 'anonymous' | 'authenticated'

interface AuthSession {
  token: string
  subject: string
  email: string | null
  centralRole: string | null
  panelRole: PanelRole
}

interface AuthContextValue {
  status: AuthStatus
  session: AuthSession | null
  acceptSso: (session: SsoSession) => Promise<void>
  logout: () => void
  canMutate: boolean
  canManageBindings: boolean
}

export const ssoConfig = { issuer: import.meta.env.VITE_AUTH_ISSUER ?? 'http://localhost:7701', clientId: 'admin-panel' }
let accessToken: string | null = null
const AuthContext = createContext<AuthContextValue | null>(null)

function role(value: unknown): PanelRole {
  return value === 'platform_admin' || value === 'platform_operator' ? value : 'platform_viewer'
}

async function readMe(token: string): Promise<AuthSession> {
  const response = await fetch('/api/v1/auth/me', {
    headers: { Authorization: `Bearer ${token}` },
  })
  if (!response.ok) throw new Error('session is invalid')
  const body = await response.json()
  return {
    token,
    subject: body.subject,
    email: body.email ?? null,
    centralRole: body.central_role ?? null,
    panelRole: role(body.panel_role),
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>('loading')
  const [session, setSession] = useState<AuthSession | null>(null)

  const clearSession = useCallback(() => {
    accessToken = null
    setSession(null)
    setStatus('anonymous')
  }, [])

  const logout = useCallback(() => {
    clearSession()
    endSso(ssoConfig)
  }, [clearSession])

  useEffect(() => {
    sessionStorage.removeItem('base.admin.token')
    const token = accessToken
    if (!token) {
      setStatus('anonymous')
      return
    }
    void readMe(token)
      .then((next) => {
        setSession(next)
        setStatus('authenticated')
      })
      .catch(clearSession)
  }, [clearSession])

  useEffect(() => {
    const expire = () => clearSession()
    window.addEventListener('base-admin:unauthorized', expire)
    return () => window.removeEventListener('base-admin:unauthorized', expire)
  }, [clearSession])

  const acceptSso = useCallback(async (sso: SsoSession) => {
    const next = await readMe(sso.accessToken)
    if (next.subject !== sso.subject) throw new Error('Central Auth вернул несовпадающего пользователя')
    accessToken = sso.accessToken
    setSession(next)
    setStatus('authenticated')
  }, [])

  const value = useMemo<AuthContextValue>(() => ({
    status,
    session,
    acceptSso,
    logout,
    canMutate: Boolean(session),
    canManageBindings: false,
  }), [acceptSso, logout, session, status])

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const value = useContext(AuthContext)
  if (!value) throw new Error('useAuth must be used within AuthProvider')
  return value
}

export function authToken() {
  return accessToken
}
