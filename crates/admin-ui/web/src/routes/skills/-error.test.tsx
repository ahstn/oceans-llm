import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from '@tanstack/react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillsErrorPage } from './-error'

beforeEach(() => {
  vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
  // These cases deliberately throw loader errors to exercise the route boundary.
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

async function renderFailedSkillsRoute(loader: () => Promise<void>) {
  const root = createRootRoute({
    component: () => (
      <>
        <nav aria-label="App navigation">
          <a href="/admin/profile">Profile</a>
        </nav>
        <Outlet />
      </>
    ),
  })
  const skills = createRoute({
    getParentRoute: () => root,
    path: '/skills',
    loader,
    errorComponent: SkillsErrorPage,
    component: () => <h1>Skill catalog</h1>,
  })
  const router = createRouter({
    routeTree: root.addChildren([skills]),
    history: createMemoryHistory({ initialEntries: ['/skills'] }),
  })
  await router.load()
  render(<RouterProvider router={router} />)
}

describe('Skills route failures', () => {
  it('keeps navigation visible when skills are not configured', async () => {
    await renderFailedSkillsRoute(async () => {
      throw new Error('skills are not configured')
    })

    expect(await screen.findByRole('heading', { name: 'Skills are unavailable' })).toBeVisible()
    expect(screen.getByRole('alert')).toHaveTextContent('skills are not configured')
    expect(screen.getByRole('link', { name: 'Profile' })).toBeVisible()
    expect(screen.queryByText('The admin UI could not load')).not.toBeInTheDocument()
  })

  it('preserves a different gateway error and retries the loader successfully', async () => {
    const loader = vi
      .fn<() => Promise<void>>()
      .mockRejectedValueOnce(new Error('database is locked'))
      .mockResolvedValue(undefined)
    await renderFailedSkillsRoute(loader)

    expect(await screen.findByRole('alert')).toHaveTextContent('database is locked')
    expect(screen.queryByText(/not configured/)).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))

    expect(await screen.findByRole('heading', { name: 'Skill catalog' })).toBeVisible()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Profile' })).toBeVisible()
  })
})
