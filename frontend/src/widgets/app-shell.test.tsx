import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { AppShell } from './app-shell'

function withProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={['/services']}>{ui}</MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('AppShell', () => {
  it('renders all approved navigation sections', () => {
    withProviders(<AppShell />)
    for (const label of [
      'Обзор',
      'Брендинг',
      'Каталог сервисов',
      'Конфигурации',
      'Аудит',
      'Runtime',
    ]) {
      expect(screen.getByText(label)).toBeInTheDocument()
    }
  })

  it('marks the active section', () => {
    withProviders(<AppShell />)
    const active = screen.getByText('Каталог сервисов').closest('a')
    expect(active?.className).toContain('bg-surface-raised')
  })
})
