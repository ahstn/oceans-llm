/** Message to show a user for a failed request; falls back when the error carries none. */
export function getErrorMessage(error: unknown, fallback = 'Request failed') {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message
  }
  return fallback
}
