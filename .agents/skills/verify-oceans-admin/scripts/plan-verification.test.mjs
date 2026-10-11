import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { globToRegExp, planFor } from './plan-verification.mjs'

const skillDir = join(dirname(fileURLToPath(import.meta.url)), '..')
const routing = JSON.parse(readFileSync(join(skillDir, 'features/routing.json'), 'utf8'))

test('globs match single segments, nested paths, and brace sets', () => {
  assert.ok(globToRegExp('*.md').test('README.md'))
  assert.ok(!globToRegExp('*.md').test('docs/README.md'))
  assert.ok(globToRegExp('docs/**').test('docs/adr/one.md'))
  assert.ok(globToRegExp('**/*.test.{ts,tsx}').test('a/b/c.test.tsx'))
  assert.ok(globToRegExp('**/*.test.{ts,tsx}').test('c.test.ts'))
  assert.ok(!globToRegExp('crates/gateway/src/http/mcp_*.rs').test('crates/gateway/src/http/mcp_gateway/aggregate.rs'))
})

test('documentation-only diffs report no runtime surface', () => {
  const plan = planFor(['docs/index.md', 'crates/gateway/src/http/request_body/tests.rs'], routing)
  assert.equal(plan.noRuntimeSurface, true)
  assert.deepEqual(plan.proofs, [])
})

test('a route change selects its feature and an unknown path stays unmapped', () => {
  const plan = planFor(
    ['crates/admin-ui/web/src/routes/models.tsx', 'crates/brand-new/src/lib.rs'],
    routing,
  )
  assert.deepEqual(plan.proofs.map((proof) => proof.id), ['models'])
  assert.deepEqual(plan.unmapped, ['crates/brand-new/src/lib.rs'])
  assert.equal(plan.noRuntimeSurface, false)
})

test('every feature entry points at an existing recipe and every recipe is routed', () => {
  const routed = new Set(routing.features.map((feature) => feature.file))
  for (const file of routed) assert.ok(existsSync(join(skillDir, 'features', file)), file)
  const recipes = readdirSync(join(skillDir, 'features')).filter(
    (file) => file.endsWith('.md') && file !== 'README.md',
  )
  assert.deepEqual(recipes.filter((file) => !routed.has(file)), [])
  const shared = routing.shared.flatMap((rule) => rule.features)
  const ids = new Set(routing.features.map((feature) => feature.id))
  assert.deepEqual(shared.filter((id) => !ids.has(id)), [])
})

test('every admin UI route file is routed to a feature', () => {
  const repoRoot = join(skillDir, '../../..')
  const routes = execFileSync('git', ['ls-files', 'crates/admin-ui/web/src/routes'], {
    cwd: repoRoot,
    encoding: 'utf8',
  })
    .split('\n')
    .filter(Boolean)
  const plan = planFor(routes, routing)
  assert.deepEqual(plan.unmapped, [], 'add these routes to features/routing.json')
})
