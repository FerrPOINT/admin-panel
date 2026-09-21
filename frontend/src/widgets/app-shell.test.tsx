import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { ThemeProvider } from '@sdlc/ui/lib'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { AppShell } from './app-shell'

const auth = vi.hoisted(() => ({ logout: vi.fn() }))

vi.mock('@/shared/auth/auth-context', () => ({
  useAuth: () => ({
    session: {
      subject: 'user-1',
      email: 'operator@example.test',
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
  })

  it('renders the approved navigation and keeps a direct detail route active', () => {
    renderShell()

    expect(screen.getByRole('heading', { name: 'Service content' })).toBeVisible()
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

  it('keeps global controls in the header and signs out through auth', () => {
    renderShell()
    const header = within(screen.getByRole('banner'))

    expect(header.getByRole('button', { name: 'Открыть список сервисов' })).toBeVisible()
    fireEvent.click(header.getByRole('button', { name: 'Выйти' }))
    expect(auth.logout).toHaveBeenCalledOnce()
  })
})
