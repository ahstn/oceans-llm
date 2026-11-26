import { readyFilterOptions, type FilterOption } from '@/components/data/filter-options'

/** Status codes the gateway and its providers commonly return; any other code can be typed. */
const statusCodeOptions: FilterOption[] = [
  { value: '200', label: '200', hint: 'OK' },
  { value: '400', label: '400', hint: 'Bad Request' },
  { value: '401', label: '401', hint: 'Unauthorized' },
  { value: '403', label: '403', hint: 'Forbidden' },
  { value: '404', label: '404', hint: 'Not Found' },
  { value: '408', label: '408', hint: 'Request Timeout' },
  { value: '413', label: '413', hint: 'Content Too Large' },
  { value: '422', label: '422', hint: 'Unprocessable Content' },
  { value: '429', label: '429', hint: 'Too Many Requests' },
  { value: '500', label: '500', hint: 'Internal Server Error' },
  { value: '502', label: '502', hint: 'Bad Gateway' },
  { value: '503', label: '503', hint: 'Service Unavailable' },
  { value: '504', label: '504', hint: 'Gateway Timeout' },
]

export const readyStatusCodeOptions = readyFilterOptions(statusCodeOptions)

/** Accepts any three-digit HTTP status typed into the status picker. */
export function customStatusCode(search: string) {
  const code = search.trim()
  return /^[1-5]\d\d$/.test(code) ? code : null
}

/** A dollar threshold the gateway accepts: non-negative, at most four decimals. */
export function costParam(value: unknown): string | undefined {
  const text =
    typeof value === 'number'
      ? String(value)
      : typeof value === 'string'
        ? value.trim() || undefined
        : undefined
  return text !== undefined && /^\d+(\.\d{1,4})?$/.test(text) ? text : undefined
}
