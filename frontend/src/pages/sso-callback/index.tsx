import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { completeSso } from '@sdlc/ui/sso'
import { Button } from '@sdlc/ui/ui'
import { ssoConfig, useAuth } from '@/shared/auth/auth-context'

let pendingCompletion: ReturnType<typeof completeSso> | null = null

function completion() {
  if (!pendingCompletion) {
    pendingCompletion = completeSso(ssoConfig)
    void pendingCompletion.finally(() => { pendingCompletion = null }).catch(() => undefined)
  }
  return pendingCompletion
}

export function SsoCallbackPage() {
  const { acceptSso } = useAuth()
  const navigate = useNavigate()
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let active = true
    void completion().then(async (session) => {
      if (!active) return
      await acceptSso(session)
      navigate(session.returnTo, { replace: true })
    }).catch((caught) => {
      if (active) setError(caught instanceof Error ? caught.message : 'Не удалось завершить вход')
    })
    return () => { active = false }
  }, [acceptSso, navigate])
  return <main className="grid min-h-screen place-items-center bg-background p-4 text-text-primary">
    {error ? <div className="space-y-4 text-center"><p role="alert">{error}</p><Button onClick={() => navigate('/login', { replace: true })}>Повторить вход</Button></div>
      : <p role="status">Завершаем вход...</p>}
  </main>
}
