import { FormEvent, useState } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router'
import { Button, Input } from '@sdlc/ui/ui'
import { useAuth } from '@/shared/auth/auth-context'

export function LoginPage() {
  const { status, session, login } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  if (status === 'authenticated' && session) {
    const destination = (location.state as { from?: string } | null)?.from ?? '/'
    return <Navigate to={destination} replace />
  }

  async function submit(event: FormEvent) {
    event.preventDefault()
    setError(null)
    setSubmitting(true)
    try {
      await login(email, password)
      const destination = (location.state as { from?: string } | null)?.from ?? '/'
      navigate(destination, { replace: true })
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Не удалось выполнить вход')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <main className="grid min-h-screen place-items-center bg-background p-4 text-text-primary">
      <form className="w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-sm" onSubmit={submit}>
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-text-muted">Base</p>
        <h1 className="mt-2 text-2xl font-semibold">Вход в Admin Panel</h1>
        <p className="mt-2 text-sm text-text-muted">Используйте учётные данные central auth. Пароль не сохраняется в панели.</p>
        <div className="mt-6 space-y-4">
          <label className="block text-sm font-medium">
            Email
            <Input className="mt-1" type="email" autoComplete="email" value={email} onChange={(event) => setEmail(event.target.value)} required />
          </label>
          <label className="block text-sm font-medium">
            Пароль
            <Input className="mt-1" type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} required />
          </label>
        </div>
        {error && <p className="mt-4 rounded-md bg-destructive/10 p-3 text-sm text-destructive" role="alert">{error}</p>}
        <Button className="mt-6 w-full" type="submit" disabled={submitting || status === 'loading'}>
          {submitting ? 'Входим...' : 'Войти'}
        </Button>
      </form>
    </main>
  )
}
