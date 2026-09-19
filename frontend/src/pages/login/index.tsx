import { useEffect, useState } from 'react'
import { Navigate, useLocation } from 'react-router'
import { beginSso } from '@sdlc/ui/sso'
import { Button, PlatformMark } from '@sdlc/ui/ui'
import { useAuth, ssoConfig } from '@/shared/auth/auth-context'

export function LoginPage() {
  const { status } = useAuth()
  const location = useLocation()
  const [error, setError] = useState<string | null>(null)
  const loggedOut = new URLSearchParams(location.search).has('logged_out')
  const destination = (location.state as { from?: string } | null)?.from ?? '/'

  useEffect(() => {
    if (status !== 'anonymous' || loggedOut) return
    void beginSso(ssoConfig, destination).catch(() => setError('Central Auth временно недоступен.'))
  }, [status, loggedOut, destination])

  if (status === 'authenticated') return <Navigate to={destination} replace />
  return <main className="grid min-h-screen place-items-center bg-background p-4 text-text-primary">
    <div className="w-full max-w-md space-y-5">
      <PlatformMark />
      <h1 className="text-2xl font-semibold">Вход в Admin Panel</h1>
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <Button className="w-full" onClick={() => void beginSso(ssoConfig, destination).catch(() => setError('Central Auth временно недоступен.'))}>
        Войти через SDLC
      </Button>
    </div>
  </main>
}
