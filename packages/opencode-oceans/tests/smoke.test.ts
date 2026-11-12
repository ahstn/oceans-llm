import { expect, test } from 'bun:test'
import assert from 'node:assert/strict'
import { readReadyLine } from '../scripts/smoke'

test('readiness waits for a full UTF-8 line across byte chunks and ignores later records', async () => {
  const expected = JSON.stringify({ url: 'http://localhost:1234', label: 'café' })
  const chunks = [
    ...Array.from(new TextEncoder().encode(expected), (byte) => Uint8Array.of(byte)),
    new TextEncoder().encode('\n{"later":true}\n'),
  ]
  let index = 0
  const stream = new ReadableStream<Uint8Array>({
    pull(controller) {
      const chunk = chunks[index++]
      if (chunk) controller.enqueue(chunk)
      else controller.close()
    },
  })
  expect(await readReadyLine(stream)).toBe(expected)
  expect(stream.locked).toBe(false)
})

test('readiness rejects an incomplete record when the host closes stdout', async () => {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode('{"url":'))
      controller.close()
    },
  })
  await assert.rejects(readReadyLine(stream), Error)
  expect(stream.locked).toBe(false)
})
