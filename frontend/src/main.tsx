import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Toaster } from 'sonner'
import { PlatformProvider, PlatformServicesProvider } from '@sdlc/ui/lib'
import { RouterProvider } from 'react-router'
import { router } from './app/router'
import './index.css'

const queryClient = new QueryClient()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <PlatformProvider configUrl={import.meta.env.VITE_PLATFORM_BRANDING_URL ?? null}>
      <PlatformServicesProvider catalogUrl={import.meta.env.VITE_PLATFORM_SERVICES_URL ?? null}>
      <RouterProvider router={router} />
    </PlatformServicesProvider>
    </PlatformProvider>
      <Toaster theme="dark" />
    </QueryClientProvider>
  </StrictMode>,
)
