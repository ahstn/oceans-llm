import { useTransition } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

export type IdentityTransition = ReturnType<typeof useIdentityTransition>

/**
 * Every create, save, and lifecycle action on an identity page (users or teams) runs in one
 * transition, so a pending action disables the others until the refreshed data arrives. `refresh`
 * reloads the page's loader: the user list, or the teams with their members.
 */
export function useIdentityTransition() {
  const router = useRouter()
  const [isPending, startTransition] = useTransition()

  /** Runs `action` in the transition. A failure becomes an error toast unless `onError` handles it. */
  function run(
    action: () => Promise<void>,
    onError: (error: unknown) => void | Promise<void> = toastError,
  ) {
    startTransition(async () => {
      try {
        await action()
      } catch (error) {
        await onError(error)
      }
    })
  }

  async function refresh() {
    await router.invalidate()
  }

  return { isPending, run, refresh }
}

export function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}

function toastError(error: unknown) {
  toast.error(getErrorMessage(error))
}
