import { useState, type ElementType } from 'react'
import { NavLink, Outlet } from 'react-router'
import {
  History,
  Home,
  KeyRound,
  LogOut,
  Menu,
  Palette,
  Server,
  Settings,
  SlidersHorizontal,
  Table2,
  UserRound,
  Users,
} from 'lucide-react'
import {
  Button,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  PlatformMark,
  ServiceSwitcher,
  ThemeToggle,
} from '@sdlc/ui/ui'
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
  { to: '/users', icon: Users, label: 'Пользователи' },
  { to: '/tokens', icon: KeyRound, label: 'API-токены' },
]

export function AppShell() {
  const { session, logout } = useAuth()
  const operatorName = session?.email ?? session?.subject ?? 'Пользователь'

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-[72px] flex-col border-r border-border bg-surface px-2 py-3 md:flex xl:w-[264px] xl:px-3">
        <div className="mb-4 flex h-10 items-center justify-center gap-3 px-1 xl:justify-start xl:px-2">
          <PlatformMark withName={false} />
          <div className="hidden min-w-0 xl:block">
            <p className="truncate text-sm font-semibold text-text-primary">Admin Panel</p>
            <p className="truncate text-xs text-text-muted">Управление платформой</p>
          </div>
        </div>
        <ShellNavigation items={navItems} compact />
        <div className="mt-auto border-t border-border px-1 pt-3 xl:px-2">
          <div
            className="flex min-h-10 items-center justify-center gap-3 text-text-secondary xl:justify-start"
            title={operatorName}
          >
            <UserRound className="h-4 w-4 shrink-0" aria-hidden />
            <p className="hidden min-w-0 truncate text-sm font-medium text-text-primary xl:block">
              {operatorName}
            </p>
            <span className="sr-only xl:hidden">{operatorName}</span>
          </div>
        </div>
      </aside>

      <div className="min-w-0 overflow-x-hidden md:pl-[72px] xl:pl-[264px]">
        <header className="sticky top-0 z-20 h-[60px] border-b border-border bg-background/95 backdrop-blur">
          <div className="flex h-full items-center gap-2 px-4 md:px-5 xl:px-6">
            <MobileNavigation items={navItems} operatorName={operatorName} />
            <div className="min-w-0 md:hidden">
              <p className="truncate text-sm font-semibold text-text-primary">Admin Panel</p>
              <p className="truncate text-xs text-text-muted">Управление платформой</p>
            </div>

            <p className="ml-auto hidden min-w-0 truncate text-xs font-medium text-text-primary lg:block">
              {operatorName}
            </p>
            <div className="ml-auto flex shrink-0 items-center gap-1 sm:ml-0 sm:gap-2 lg:ml-0">
              <ServiceSwitcher currentKey="admin-panel" />
              <div className="[&>button]:h-10 [&>button]:min-h-10 [&>button]:w-10 [&>button]:min-w-10">
                <ThemeToggle />
              </div>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="h-10 min-h-10 w-10 min-w-10 sm:w-auto sm:px-3"
                aria-label="Выйти"
                title="Выйти"
                onClick={logout}
              >
                <LogOut className="h-4 w-4" aria-hidden />
                <span className="hidden sm:inline">Выйти</span>
              </Button>
            </div>
          </div>
        </header>

        <main className="min-h-[calc(100dvh-60px)] min-w-0 px-4 py-5 md:px-5 xl:px-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}

function ShellNavigation({ items, compact = false }: { items: NavItem[]; compact?: boolean }) {
  return (
    <nav className="space-y-1" aria-label="Разделы">
      {items.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          end={item.to === '/'}
          aria-label={compact ? item.label : undefined}
          title={compact ? item.label : undefined}
          className={({ isActive }) =>
            `flex h-10 items-center gap-3 rounded-md px-3 text-sm text-text-secondary transition-colors hover:bg-surface-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus ${
              compact ? 'justify-center xl:justify-start' : ''
            } ${isActive ? 'bg-surface-raised text-text-primary' : ''}`
          }
        >
          <item.icon className="h-4 w-4 shrink-0" aria-hidden />
          <span className={compact ? 'hidden xl:inline' : undefined}>{item.label}</span>
        </NavLink>
      ))}
    </nav>
  )
}

function MobileNavigation({ items, operatorName }: { items: NavItem[]; operatorName: string }) {
  const [open, setOpen] = useState(false)

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-10 min-h-10 w-10 min-w-10 md:hidden"
          aria-label="Открыть навигацию"
          title="Открыть навигацию"
        >
          <Menu className="h-5 w-5" aria-hidden />
        </Button>
      </DialogTrigger>
      <DialogContent className="!left-0 !top-0 !flex !h-dvh !max-h-dvh !w-[min(320px,calc(100%-2rem))] !max-w-none !translate-x-0 !translate-y-0 !flex-col !gap-0 !rounded-none !border-y-0 !border-l-0 !p-0 [&>button]:h-10 [&>button]:min-h-10 [&>button]:w-10 [&>button]:min-w-10">
        <DialogHeader className="flex h-[60px] flex-row items-center gap-3 border-b border-border px-4 pr-14 text-left">
          <PlatformMark withName={false} />
          <div className="min-w-0">
            <DialogTitle className="truncate text-base">Admin Panel</DialogTitle>
            <p className="truncate text-xs text-text-muted">Управление платформой</p>
          </div>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-3 py-4">
          <ShellNavigation items={items} />
          <div className="mt-auto border-t border-border px-3 pt-4">
            <p className="truncate text-sm font-medium text-text-primary">{operatorName}</p>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
