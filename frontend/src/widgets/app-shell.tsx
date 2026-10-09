import { useEffect, useState, type ElementType } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router'
import {
  Bot,
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  PageFrame,
  PlatformMark,
  PlatformHeader,
  ThemeMenuItems,
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
  { to: '/ai', icon: Bot, label: 'AI-провайдеры' },
]

export function AppShell() {
  const { session, logout } = useAuth()
  const location = useLocation()
  const operatorName = session?.email ?? session?.subject ?? 'Пользователь'
  const pageLayout =
    location.pathname === '/branding' || location.pathname === '/settings'
      ? 'reading'
      : location.pathname.startsWith('/services/')
        ? 'detail-with-aside'
        : 'wide'

  return (
    <div className="min-h-screen bg-background text-text-primary">
      <PlatformHeader
        currentServiceKey="admin-panel"
        leading={
          <>
            <MobileNavigation items={navItems} />
            <NavLink
              to="/"
              aria-label="Admin Panel"
              className="flex items-center justify-center rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
            >
              <PlatformMark size="sm" withName={false} />
            </NavLink>
          </>
        }
        actions={
          <>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label="Аккаунт"
                  title="Аккаунт"
                >
                  <UserRound className="h-4 w-4" aria-hidden />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-64 max-w-[calc(100vw-2rem)]">
                <div className="break-words px-2 py-2 text-sm font-medium text-text-primary">
                  {operatorName}
                </div>
                <ThemeMenuItems />
                <DropdownMenuItem onSelect={logout} className="min-h-11 gap-2 md:min-h-10">
                  <LogOut className="h-4 w-4" aria-hidden />
                  Выйти
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        }
      />
      <aside className="fixed bottom-0 left-0 top-[var(--shell-header-height)] z-20 hidden w-[var(--shell-sidebar-compact)] flex-col border-r border-border bg-surface px-2 py-3 md:flex xl:w-[var(--shell-sidebar-expanded)] xl:px-3">
        <ShellNavigation items={navItems} compact />
      </aside>
      <div className="min-w-0 md:pl-[var(--shell-sidebar-compact)] xl:pl-[var(--shell-sidebar-expanded)]">
        <main className="shell-main min-h-[calc(100dvh-var(--shell-header-height))]">
          <PageFrame mode={pageLayout}>
            <Outlet />
          </PageFrame>
        </main>
      </div>
    </div>
  )
}

function ShellNavigation({
  items,
  compact = false,
  onNavigate,
}: {
  items: NavItem[]
  compact?: boolean
  onNavigate?: () => void
}) {
  return (
    <nav className="space-y-1" aria-label="Разделы">
      {items.map((item) => (
        <NavLink
          key={item.to}
          to={item.to}
          end={item.to === '/'}
          onClick={onNavigate}
          aria-label={compact ? item.label : undefined}
          title={compact ? item.label : undefined}
          className={({ isActive }) =>
            `flex min-h-11 items-center gap-3 rounded-md px-3 text-sm text-text-secondary transition-colors hover:bg-surface-raised hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus md:min-h-10 ${
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

function MobileNavigation({ items }: { items: NavItem[] }) {
  const [open, setOpen] = useState(false)
  useEffect(() => {
    if (!window.matchMedia) return
    const media = window.matchMedia('(min-width: 768px)')
    const closeOnDesktop = () => {
      if (media.matches) setOpen(false)
    }
    media.addEventListener('change', closeOnDesktop)
    return () => media.removeEventListener('change', closeOnDesktop)
  }, [])

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
      <DialogContent className="!left-0 !top-0 !flex !h-dvh !max-h-dvh !w-[min(320px,calc(100%-2rem))] !max-w-none !translate-x-0 !translate-y-0 !flex-col !gap-0 !rounded-none !border-y-0 !border-l-0 !p-0 [&>button]:h-11 [&>button]:min-h-11 [&>button]:w-11 [&>button]:min-w-11">
        <DialogHeader className="flex h-[var(--shell-header-height)] flex-row items-center gap-3 border-b border-border px-4 pr-14 text-left">
          <PlatformMark withName={false} />
          <div className="min-w-0">
            <DialogTitle className="truncate text-base">Admin Panel</DialogTitle>
            <p className="truncate text-xs text-text-muted">Управление платформой</p>
          </div>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-3 py-4">
          <ShellNavigation items={items} onNavigate={() => setOpen(false)} />
        </div>
      </DialogContent>
    </Dialog>
  )
}
