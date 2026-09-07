import { describe, expect, test } from 'bun:test'
import { reviewEnvironment } from './pi'
import { parseDiffAnchors } from './review-diff'

describe('Pi review boundary', () => {
  test('does not forward action publishing credentials or ambient Pi configuration', () => {
    const env = reviewEnvironment('/tmp/isolated-review', 'gateway-test-key')
    expect(env.GITHUB_TOKEN).toBeUndefined()
    expect(env.NODE_OPTIONS).toBeUndefined()
    expect(env.HOME).toBe('/tmp/isolated-review')
    expect(env.PI_CODING_AGENT_DIR).toBe('/tmp/isolated-review/agent')
    expect(env.PI_MCP_CONFIG_MODE).toBe('exclusive')
    expect(env.OCEANS_REVIEW_API_KEY).toBe('gateway-test-key')
  })

  test('anchors only changed right-side lines, including multi-line hunks', () => {
    const diff =
      'diff --git a/a.ts b/a.ts\n+++ b/a.ts\n@@ -1 +1,2 @@\n+x\n+y\n@@ -8,2 +9,0 @@\n-old\n-old\n'
    expect([...parseDiffAnchors(diff).get('a.ts')!]).toEqual([1, 2])
  })
})
