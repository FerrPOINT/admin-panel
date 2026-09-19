import { Navigate, createBrowserRouter, useLocation } from 'react-router'
import { AppShell } from '@/widgets/app-shell'
import { OverviewPage } from '@/pages/overview'
import { BrandingPage } from '@/pages/branding'
import { ServicesPage } from '@/pages/services'
import { ServiceDetailPage } from '@/pages/service-detail'
import { RevisionsPage } from '@/pages/revisions'
import { AuditPage } from '@/pages/audit'
import { RuntimePage } from '@/pages/runtime'
import { SettingsPage } from '@/pages/settings'
import { UsersPage } from '@/pages/users'
import { TokensPage } from '@/pages/tokens'
import { LoginPage } from '@/pages/login'
import { SsoCallbackPage } from '@/pages/sso-callback'
import { useAuth } from '@/shared/auth/auth-context'

function ProtectedApp() {
  const { status } = useAuth()
  const location = useLocation()

  if (status === 'loading') {
    return <main className="grid min-h-screen place-items-center text-sm text-text-muted">Проверяем сессию...</main>
  }
  if (status === 'anonymous') {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />
  }
  return <AppShell />
}

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  { path: '/sso/callback', element: <SsoCallbackPage /> },
  {
    element: <ProtectedApp />,
    children: [
      { path: '/', element: <OverviewPage /> },
      { path: '/branding', element: <BrandingPage /> },
      { path: '/services', element: <ServicesPage /> },
      { path: '/services/:serviceKey', element: <ServiceDetailPage /> },
      { path: '/revisions', element: <RevisionsPage /> },
      { path: '/audit', element: <AuditPage /> },
      { path: '/runtime', element: <RuntimePage /> },
      { path: '/settings', element: <SettingsPage /> },
      { path: '/users', element: <UsersPage /> },
      { path: '/tokens', element: <TokensPage /> },
    ],
  },
])
