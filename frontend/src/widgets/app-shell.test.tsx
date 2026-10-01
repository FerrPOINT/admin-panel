import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { ThemeProvider } from '@sdlc/ui/lib'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AppShell } from './app-shell'

const auth = vi.hoisted(() => ({
  logout: vi.fn(),
  email: 'operator@example.test' as string | null,
}))

vi.mock('@/shared/auth/auth-context', () => ({
  useAuth: () => ({
    session: {
      subject: 'user-1',
      email: auth.email,
    },
    logout: auth.logout,
  }),
}))

function renderShell(path = '/services/service-a') {
  return render(
    <ThemeProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route element={<AppShell />}>
            <Route path="/" element={<h1>Overview content</h1>} />
            <Route path="/services/:serviceKey" element={<h1>Service content</h1>} />
          </Route>
        </Routes>
      </MemoryRouter>
    </ThemeProvider>,
  )
}

describe('AppShell', () => {
  beforeEach(() => {
    auth.logout.mockReset()
    auth.email = 'operator@example.test'
  })

  it('renders the approved navigation and keeps a direct detail route active', () => {
    renderShell()

    expect(screen.getByRole('heading', { name: 'Service content' })).toBeVisible()
    expect(screen.getByRole('heading', { name: 'Service content' }).parentElement).toHaveAttribute(
      'data-page-layout',
      'detail-with-aside',
    )
    for (const label of [
      'Обзор',
      'Брендинг',
      'Каталог сервисов',
      'Конфигурации',
      'Аудит',
      'Runtime',
      'Локальные настройки',
      'Пользователи',
      'API-токены',
    ]) {
      expect(screen.getByRole('link', { name: label })).toBeInTheDocument()
    }
    expect(screen.getByRole('link', { name: 'Каталог сервисов' })).toHaveClass('bg-surface-raised')
  })

  it('closes the mobile drawer with Escape and returns focus to its trigger', async () => {
    renderShell()
    const trigger = screen.getByRole('button', { name: 'Открыть навигацию' })

    fireEvent.click(trigger)
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByRole('link', { name: 'Каталог сервисов' })).toHaveClass(
      'bg-surface-raised',
    )

    fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(trigger).toHaveFocus()
  })

  it('closes the mobile drawer after navigating to a section', async () => {
    renderShell()
    const trigger = screen.getByRole('button', { name: 'Открыть навигацию' })

    fireEvent.click(trigger)
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('link', { name: 'Обзор' }))

    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
    expect(screen.getByRole('heading', { name: 'Overview content' })).toBeVisible()
    expect(trigger).toHaveFocus()
  })

  it('keeps global controls in the header and signs out through auth', () => {
    renderShell()
    const header = within(screen.getByRole('banner'))

    expect(
      header.getByRole('button', { name: 'Открыть список сервисов: Admin Panel' }),
    ).toBeVisible()
    expect(screen.queryByText(auth.email!)).not.toBeInTheDocument()
    fireEvent.keyDown(header.getByRole('button', { name: 'Аккаунт' }), { key: 'ArrowDown' })
    const menu = screen.getByRole('menu', { hidden: true })
    expect(menu).toHaveAttribute('data-state', 'open')
    expect(within(menu).getAllByText(auth.email!)).toHaveLength(1)
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Выйти' }))
    expect(auth.logout).toHaveBeenCalledOnce()
  })

  it('uses one full-width platform header and preserves slot ownership', () => {
    const { container } = renderShell()
    const header = screen.getByRole('banner')
    expect(container.querySelectorAll('[data-platform-header]')).toHaveLength(1)
    expect(
      [...header.querySelectorAll('[data-platform-header-slot]')].map((slot) =>
        slot.getAttribute('data-platform-header-slot'),
      ),
    ).toEqual(['leading', 'services', 'actions'])
    expect(within(header).getAllByRole('button', { name: /Открыть список сервисов/ })).toHaveLength(
      1,
    )
    expect(container.querySelector('aside')).not.toContainElement(header)
    expect(container.querySelector('aside')).not.toHaveTextContent('operator@example.test')
    expect(within(header).getByRole('link', { name: 'Admin Panel' })).toHaveAttribute('href', '/')
  })

  it('shows a long identity only in the account menu and closes with Escape', () => {
    auth.email = 'long-operator-identity-that-must-not-expand-the-platform-header@example.test'
    renderShell()
    const trigger = screen.getByRole('button', { name: 'Аккаунт' })
    fireEvent.keyDown(trigger, { key: 'ArrowDown' })
    expect(
      within(screen.getByRole('menu', { hidden: true })).getByText(auth.email),
    ).toBeInTheDocument()
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('menu', { hidden: true })).not.toBeInTheDocument()
  })

  it('keeps account logout available when the identity has no email', () => {
    auth.email = null
    renderShell()
    fireEvent.keyDown(screen.getByRole('button', { name: 'Аккаунт' }), { key: 'ArrowDown' })
    expect(
      within(screen.getByRole('menu', { hidden: true })).getByText('user-1'),
    ).toBeInTheDocument()
    fireEvent.click(screen.getByRole('menuitem', { name: 'Выйти' }))
    expect(auth.logout).toHaveBeenCalledOnce()
  })
})
