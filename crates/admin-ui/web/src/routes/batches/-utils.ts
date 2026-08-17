import type { BatchStatus } from '@/types/api'

export function isBatchActive(status: BatchStatus) {
  return !['completed', 'failed', 'expired', 'cancelled', 'submission_unknown'].includes(status)
}

export function isBatchCancellable(status: BatchStatus) {
  return ['queued', 'validating', 'in_progress', 'finalizing'].includes(status)
}

export function formatStatus(status: string) {
  return status
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ')
}
