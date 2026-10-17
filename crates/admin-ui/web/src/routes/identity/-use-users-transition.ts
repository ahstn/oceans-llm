import { useTransition } from 'react'
import { useRouter } from '@tanstack/react-router'
import { toast } from 'sonner'

export type UsersTransition = ReturnType<typeof useUsersTransition>

/**
 * Every create, save, and lifecycle action on the users page runs in one transition, so a pending
 * action disables the others until the refreshed user list arrives. The teams page uses it the
 * same way, and `refreshUsers` reloads its teams along with their users.
 */
export function useUsersTransition() {
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

  async function refreshUsers() {
    await router.invalidate()
  }

  return { isPending, run, refreshUsers }
}

export function getErrorMessage(error: unknown) {
  return error instanceof Error ? error.message : 'Something went wrong'
}

function toastError(error: unknown) {
  toast.error(getErrorMessage(error))
}
