import { toast } from 'sonner'

import { AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import type { CreateUserResult } from '@/types/api'

/** Alert title and description for a generated password invite or SSO sign-in URL. */
export function OnboardingResultSummary({ result }: { result: CreateUserResult }) {
  return (
    <>
      <AlertTitle>
        {result.kind === 'password_invite' ? 'Password invite ready' : 'SSO sign-in URL ready'}
      </AlertTitle>
      <AlertDescription>
        {result.kind === 'password_invite'
          ? `Share this invite before ${result.expires_at}.`
          : `Share this URL with ${onboardingRecipient(result)} so they can finish SSO onboarding.`}
      </AlertDescription>
    </>
  )
}

/** The generated onboarding URL, read-only, with a button that copies it. */
export function GeneratedUrlInput({ id, result }: { id: string; result: CreateUserResult }) {
  const url = result.kind === 'password_invite' ? result.invite_url : result.sign_in_url

  return (
    <InputGroup>
      <InputGroupInput id={id} readOnly value={url} />
      <InputGroupAddon align="inline-end">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => copyToClipboard(url, 'URL copied')}
        >
          Copy
        </Button>
      </InputGroupAddon>
    </InputGroup>
  )
}

function onboardingRecipient(result: CreateUserResult) {
  return 'user' in result && result.user ? result.user.email : 'the user'
}

async function copyToClipboard(value: string, message: string) {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(message)
  } catch {
    toast.error('Clipboard access failed')
  }
}
