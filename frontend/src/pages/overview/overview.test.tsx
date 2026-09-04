import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { OverviewPage } from './index'

function withProviders(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>{ui}</MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('OverviewPage', () => {
  it('renders summary cards and loading states without placeholders', () => {
    withProviders(<OverviewPage />)
    expect(screen.getByText('Обзор платформы')).toBeInTheDocument()
    expect(screen.getByText('Брендинг')).toBeInTheDocument()
    expect(screen.getByText('Сервисы')).toBeInTheDocument()
    expect(screen.getByText('Проблемы')).toBeInTheDocument()
  })
})
