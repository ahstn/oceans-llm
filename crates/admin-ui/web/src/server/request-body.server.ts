// Leave room for the archive field, skill ID, and TanStack's serialized context.
export const MULTIPART_OVERHEAD_BYTES = 64 * 1024

/** Check a retained copy before the server-function dispatcher parses multipart fields. */
export async function checkMultipartBodySize(request: Request, limit: number) {
  const declaredLength = request.headers.get('content-length')
  if (declaredLength && Number(declaredLength) > limit) {
    void request.body?.cancel().catch(() => {})
    return payloadTooLarge()
  }
  if (!request.body) return null

  const retained = request.clone()
  const reader = retained.body!.getReader()
  let bytes = 0
  try {
    while (true) {
      // Read one chunk at a time so the limit stops the producer before parsing.
      // eslint-disable-next-line no-await-in-loop
      const chunk = await reader.read()
      if (chunk.done) return null
      bytes += chunk.value.byteLength
      if (bytes > limit) {
        // Neither tee branch can finish cancellation while the other remains unread.
        void Promise.all([reader.cancel(), request.body.cancel()]).catch(() => {})
        return payloadTooLarge()
      }
    }
  } catch {
    void Promise.all([reader.cancel(), request.body.cancel()]).catch(() => {})
    return new Response('Unable to read multipart upload', { status: 400 })
  } finally {
    reader.releaseLock()
  }
}

function payloadTooLarge() {
  return new Response('Multipart upload is too large', { status: 413 })
}
