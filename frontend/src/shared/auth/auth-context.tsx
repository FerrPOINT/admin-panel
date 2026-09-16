import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'

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
  login: (email: string, password: string) => Promise<void>
  logout: () => void
  canMutate: boolean
  canManageBindings: boolean
}

const tokenKey = 'base.admin.token'
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

  const logout = useCallback(() => {
    sessionStorage.removeItem(tokenKey)
    setSession(null)
    setStatus('anonymous')
  }, [])

  useEffect(() => {
    const token = sessionStorage.getItem(tokenKey)
    if (!token) {
      setStatus('anonymous')
      return
    }
    void readMe(token)
      .then((next) => {
        setSession(next)
        setStatus('authenticated')
      })
      .catch(logout)
  }, [logout])

  useEffect(() => {
    const expire = () => logout()
    window.addEventListener('base-admin:unauthorized', expire)
    return () => window.removeEventListener('base-admin:unauthorized', expire)
  }, [logout])

  const login = useCallback(async (email: string, password: string) => {
    const response = await fetch('/api/v1/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password }),
    })
    if (!response.ok) {
      const body = await response.json().catch(() => null)
      throw new Error(body?.error?.message ?? 'Не удалось выполнить вход')
    }
    const body = await response.json()
    const next = await readMe(body.access_token)
    sessionStorage.setItem(tokenKey, body.access_token)
    setSession(next)
    setStatus('authenticated')
  }, [])

  const value = useMemo<AuthContextValue>(() => ({
    status,
    session,
    login,
    logout,
    canMutate: session?.panelRole === 'platform_operator' || session?.panelRole === 'platform_admin',
    canManageBindings: session?.panelRole === 'platform_admin',
  }), [login, logout, session, status])

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const value = useContext(AuthContext)
  if (!value) throw new Error('useAuth must be used within AuthProvider')
  return value
}

export function authToken() {
  return sessionStorage.getItem(tokenKey)
}
