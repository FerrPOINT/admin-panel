import { useState, type ElementType } from 'react'
import { Link, Outlet, useLocation } from 'react-router'
import {
  History,
  Home,
  Menu,
  Palette,
  Server,
  Settings,
  SlidersHorizontal,
  Table2,
  Shield,
  X,
} from 'lucide-react'
import { Button } from '@sdlc/ui/ui'
import { ServiceSwitcher } from '@sdlc/ui/ui'
import { LogOut } from 'lucide-react'
import { useAuth } from '@/shared/auth/auth-context'

type NavItem = {
  to: string
  icon: ElementType
  label: string
}

const navItems: NavItem[] = [
  { to: '/', icon: Home, label: 'Обзор' },
  { to: '/branding', icon: Palette, label: 'Брендинг' },
  { to: '/services', icon: Server, label: 'Каталог сервисов' },
  { to: '/revisions', icon: Table2, label: 'Конфигурации' },
  { to: '/audit', icon: History, label: 'Аудит' },
  { to: '/runtime', icon: SlidersHorizontal, label: 'Runtime' },
  { to: '/settings', icon: Settings, label: 'Локальные настройки' },
  { to: '/role-bindings', icon: Shield, label: 'Привязки ролей' },
]

function SidebarLink({
  to,
  icon: Icon,
  label,
  active,
  onClick,
}: {
  to: string
  icon: ElementType
  label: string
  active: boolean
  onClick?: () => void
}) {
  return (
    <Link
      to={to}
      onClick={onClick}
      className={`flex items-center gap-3 rounded-md px-3 py-2 text-sm transition-colors ${
        active
          ? 'bg-surface-raised text-text-primary'
          : 'text-text-secondary hover:bg-surface-raised hover:text-text-primary'
      }`}
    >
      <Icon className="h-4 w-4 shrink-0" />
      <span className="truncate">{label}</span>
    </Link>
  )
}

export function AppShell() {
  const location = useLocation()
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const { session, logout } = useAuth()

  const isActive = (to: string) =>
    to === '/' ? location.pathname === '/' : location.pathname.startsWith(to)

  const sidebar = (
    <nav className="flex h-full w-60 flex-col gap-1 border-r border-border bg-surface p-3">
      <div className="mb-4 px-3 py-2 text-sm font-semibold tracking-wide text-text-primary">
        Admin Panel
      </div>
      {navItems.map((item) => (
        <SidebarLink
          key={item.to}
          {...item}
          active={isActive(item.to)}
          onClick={() => setMobileMenuOpen(false)}
        />
      ))}
      <div className="mt-auto space-y-2 pt-3">
        <ServiceSwitcher currentKey="admin-panel" />
        <div className="flex items-center justify-between gap-2 rounded-md border border-border bg-surface-raised px-3 py-2 text-xs">
          <div className="min-w-0">
            <p className="truncate text-text-primary">{session?.email ?? session?.subject}</p>
            <p className="text-text-muted">{session?.panelRole}</p>
          </div>
          <Button variant="ghost" size="sm" aria-label="Выйти" onClick={logout}>
            <LogOut className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </nav>
  )

  return (
    <div className="flex min-h-screen bg-background text-text-primary">
      <aside className="hidden md:flex">{sidebar}</aside>

      {mobileMenuOpen && (
        <div className="fixed inset-0 z-40 flex md:hidden">
          <div className="flex-1 bg-black/50" onClick={() => setMobileMenuOpen(false)} />
          <div className="flex">{sidebar}</div>
        </div>
      )}

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between gap-3 border-b border-border bg-surface px-4 py-3 md:hidden">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setMobileMenuOpen((v) => !v)}
            aria-label="Меню"
          >
            {mobileMenuOpen ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
          </Button>
          <span className="text-sm font-semibold">Admin Panel</span>
          <span className="w-9" />
        </header>

        <main className="min-w-0 flex-1 p-4 md:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
