import { afterEach, expect, spyOn, test } from 'bun:test'
import assert from 'node:assert/strict'
import * as core from '@actions/core'
import * as pi from './pi'
import * as context from './github-context'
import * as preflight from './preflight'
import { OceansClient } from './oceans-client'
import { GitHubPublisher } from './summary'
import { run } from './run-lifecycle'

const restores: (() => void)[] = []
afterEach(() => {
  for (const restore of restores.splice(0).reverse()) restore()
  core.summary.emptyBuffer()
})

function fixture(mode: 'direct' | 'oceans') {
  const inputs: Record<string, string> = {
    'report-to-oceans': 'false',
    'model-mode': mode,
    'model-id': 'openai/test-model',
    'github-token': 'test-token',
    ...(mode === 'oceans' ? { 'oceans-url': 'https://oceans.test', 'oceans-api-key': 'key' } : {}),
  }
  const input = spyOn(core, 'getInput').mockImplementation((name) => inputs[name] || '')
  const runtime = spyOn(context, 'readGitHubContext').mockReturnValue({
    eventName: 'pull_request',
    repository: 'test/repo',
    workspace: '/test',
    eventPayload: {
      repository: { full_name: 'test/repo' },
      pull_request: {
        number: 1,
        base: { sha: 'base', repo: { full_name: 'test/repo' } },
        head: { sha: 'head', repo: { full_name: 'test/repo' } },
      },
    },
  })
  const checkout = spyOn(preflight, 'validateCheckoutHead').mockReturnValue({ ok: true })
  const summary = spyOn(core.summary, 'write').mockResolvedValue(core.summary)
  const invoke = spyOn(pi, 'invokePi').mockResolvedValue({
    summary: 'Review complete',
    findings: [],
    metrics: {},
    degradedFeatures: [],
  })
  const publish = spyOn(GitHubPublisher.prototype, 'publish').mockResolvedValue({})
  const resolve = spyOn(OceansClient.prototype, 'resolveConfig').mockResolvedValue({
    effective_config: { model_id: 'gateway-model' },
    repository: {},
    pull_request_id: 'pr',
    overrides_applied: {},
    overrides_rejected: {},
    reporting: {},
  })
  const start = spyOn(OceansClient.prototype, 'startRun')
  const complete = spyOn(OceansClient.prototype, 'completeRun')
  const fail = spyOn(OceansClient.prototype, 'failRun')
  for (const spy of [
    input,
    runtime,
    checkout,
    summary,
    invoke,
    publish,
    resolve,
    start,
    complete,
    fail,
  ])
    restores.push(() => spy.mockRestore())
  return { invoke, publish, resolve, start, complete, fail }
}

for (const mode of ['direct', 'oceans'] as const) {
  test(`reporting disabled: ${mode} reviews publish without reporting success or failure`, async () => {
    const f = fixture(mode)
    await run()
    expect(f.publish).toHaveBeenCalledTimes(1)
    expect(f.resolve).toHaveBeenCalledTimes(mode === 'direct' ? 0 : 1)
    if (mode === 'direct') {
      expect(f.invoke.mock.calls[0]?.[0].effectiveConfig).toEqual({
        model_id: 'openai/test-model',
        model_execution_mode: 'direct',
      })
    }
    f.invoke.mockRejectedValue(new Error('provider failed'))
    await assert.rejects(run(), /provider failed/)
    expect(f.publish).toHaveBeenCalledTimes(1)
    expect(f.start).not.toHaveBeenCalled()
    expect(f.complete).not.toHaveBeenCalled()
    expect(f.fail).not.toHaveBeenCalled()
  })
}
