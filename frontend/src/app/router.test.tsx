import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/auth/auth-context', () => ({ useAuth: () => ({ status: 'anonymous' }) }))
vi.mock('@/widgets/app-shell', () => ({ AppShell: () => null }))
import { ProtectedApp } from './router'

function LoginTarget() {
  const location = useLocation()
  return <output data-testid="return-to">{(location.state as { from: string }).from}</output>
}

describe('Admin SSO return location', () => {
  it.each([
    '/namespaces/example?registry_instance_id=registry&namespace_id=project#connections',
    '/services?search=wiki',
  ])('preserves the complete protected URL %s', (target) => {
    render(
      <MemoryRouter initialEntries={[target]}>
        <Routes>
          <Route path="/login" element={<LoginTarget />} />
          <Route path="*" element={<ProtectedApp />} />
        </Routes>
      </MemoryRouter>,
    )
    expect(screen.getByTestId('return-to').textContent).toBe(target)
  })
})
