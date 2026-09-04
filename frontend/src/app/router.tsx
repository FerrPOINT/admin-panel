import { createBrowserRouter } from 'react-router'
import { AppShell } from '@/widgets/app-shell'
import { OverviewPage } from '@/pages/overview'
import { BrandingPage } from '@/pages/branding'
import { ServicesPage } from '@/pages/services'
import { ServiceDetailPage } from '@/pages/service-detail'
import { RevisionsPage } from '@/pages/revisions'
import { AuditPage } from '@/pages/audit'
import { RuntimePage } from '@/pages/runtime'
import { SettingsPage } from '@/pages/settings'

export const router = createBrowserRouter([
  {
    element: <AppShell />,
    children: [
      { path: '/', element: <OverviewPage /> },
      { path: '/branding', element: <BrandingPage /> },
      { path: '/services', element: <ServicesPage /> },
      { path: '/services/:serviceKey', element: <ServiceDetailPage /> },
      { path: '/revisions', element: <RevisionsPage /> },
      { path: '/audit', element: <AuditPage /> },
      { path: '/runtime', element: <RuntimePage /> },
      { path: '/settings', element: <SettingsPage /> },
    ],
  },
])
