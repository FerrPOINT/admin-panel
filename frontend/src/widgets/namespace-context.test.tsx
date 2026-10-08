import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
vi.mock('@/shared/api/namespaces', () => ({
  useNamespaces: () => ({
    data: [
      { registry_instance_id: 'registry', id: 'first', name: 'Same name', slug: 'first' },
      { registry_instance_id: 'registry', id: 'second', name: 'Same name', slug: 'second' },
    ],
    isPending: false,
    isError: false,
  }),
}))
import { NamespaceShellContext } from './namespace-context'
function Location() {
  const l = useLocation()
  return <output data-testid="location">{l.pathname + l.search}</output>
}
describe('Namespace picker route ownership', () => {
  it('moves the path and ref together and clears resource paths for all projects', () => {
    render(
      <MemoryRouter
        initialEntries={['/namespaces/first?registry_instance_id=registry&namespace_id=first']}
      >
        <NamespaceShellContext />
        <Location />
      </MemoryRouter>,
    )
    fireEvent.change(screen.getByRole('combobox', { name: 'Namespace' }), {
      target: { value: 'registry/second' },
    })
    expect(screen.getByTestId('location').textContent).toBe(
      '/namespaces/second?registry_instance_id=registry&namespace_id=second',
    )
    fireEvent.change(screen.getByRole('combobox', { name: 'Namespace' }), { target: { value: '' } })
    expect(screen.getByTestId('location').textContent).toBe('/namespaces')
  })
})
