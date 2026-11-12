import { afterEach, describe, expect, test } from 'bun:test'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { readGitHubContext } from './github-context'

const tempDirectories: string[] = []

afterEach(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true })
  }
})

describe('GitHub event trust boundary', () => {
  test('normalizes pull_request_target while preserving the separate review workspace', () => {
    const directory = mkdtempSync(join(tmpdir(), 'review-github-context-'))
    tempDirectories.push(directory)
    const eventPath = join(directory, 'event.json')
    writeFileSync(eventPath, JSON.stringify({ pull_request: { number: 42 } }))

    const context = readGitHubContext({
      GITHUB_EVENT_NAME: 'pull_request_target',
      GITHUB_EVENT_PATH: eventPath,
      GITHUB_REPOSITORY: 'owner/repository',
      GITHUB_WORKSPACE: '/trusted-base',
      OCEANS_REVIEW_WORKSPACE: '/trusted-base/.review-source',
    })

    expect(context.eventName).toBe('pull_request')
    expect(context.workspace).toBe('/trusted-base/.review-source')
    expect(context.eventPayload.pull_request).toEqual({ number: 42 })
  })

  test('keeps ordinary pull_request events unchanged', () => {
    const directory = mkdtempSync(join(tmpdir(), 'review-github-context-'))
    tempDirectories.push(directory)
    const eventPath = join(directory, 'event.json')
    writeFileSync(eventPath, '{}')

    const context = readGitHubContext({
      GITHUB_EVENT_NAME: 'pull_request',
      GITHUB_EVENT_PATH: eventPath,
      GITHUB_WORKSPACE: '/pull-request',
    })

    expect(context.eventName).toBe('pull_request')
    expect(context.workspace).toBe('/pull-request')
  })
})
