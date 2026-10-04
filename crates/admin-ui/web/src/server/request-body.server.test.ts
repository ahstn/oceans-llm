import { describe, expect, it, vi } from 'vitest'

import { checkMultipartBodySize } from './request-body.server'

function streamedRequest(chunks: string[], cancel = vi.fn()) {
  const encoder = new TextEncoder()
  return new Request('http://localhost/admin/_serverFn/upload', {
    method: 'POST',
    headers: { 'content-type': 'multipart/form-data; boundary=test' },
    body: new ReadableStream({
      pull(controller) {
        const chunk = chunks.shift()
        if (chunk === undefined) controller.close()
        else controller.enqueue(encoder.encode(chunk))
      },
      cancel,
    }),
    duplex: 'half',
  } as RequestInit)
}

describe('multipart request limit before dispatch', () => {
  it('rejects a declared oversized body without pulling it', async () => {
    const pull = vi.fn()
    const request = new Request('http://localhost/admin/_serverFn/upload', {
      method: 'POST',
      headers: { 'content-length': '101' },
      body: new ReadableStream({ pull }, { highWaterMark: 0 }),
      duplex: 'half',
    } as RequestInit)

    expect((await checkMultipartBodySize(request, 100))?.status).toBe(413)
    expect(pull).not.toHaveBeenCalled()
  })

  it('bounds chunked bodies without Content-Length and cancels both branches', async () => {
    const cancel = vi.fn()
    const request = streamedRequest(['1234', '5678', '90', 'unread', 'unread'], cancel)
    const rejection = await checkMultipartBodySize(request, 7)

    expect(rejection?.status).toBe(413)
    await expect.poll(() => cancel.mock.calls.length).toBe(1)
  })

  it('retains an accepted multipart body for the dispatcher to parse', async () => {
    const body =
      '--test\r\nContent-Disposition: form-data; name="id"\r\n\r\nskill-id\r\n--test--\r\n'
    const request = streamedRequest([body.slice(0, 25), body.slice(25)])

    expect(await checkMultipartBodySize(request, new TextEncoder().encode(body).length)).toBeNull()
    expect((await request.formData()).get('id')).toBe('skill-id')
  })

  it('counts actual bytes even when a smaller Content-Length is declared', async () => {
    const request = streamedRequest(['1234', '5678'])
    request.headers.set('content-length', '1')

    expect((await checkMultipartBodySize(request, 7))?.status).toBe(413)
  })
})
