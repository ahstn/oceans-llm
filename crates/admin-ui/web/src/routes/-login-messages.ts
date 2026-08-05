export function ssoErrorMessage(code: string | undefined) {
  switch (code) {
    case 'access_denied':
    case 'denied':
      return 'Access was denied for this SSO account.'
    case 'unmatched_identity':
      return 'This SSO account is not allowed to sign in.'
    case 'github_unverified_email':
      return 'GitHub did not return a primary verified email for this account. Verify your primary email at https://github.com/settings/emails, then try signing in again.'
    case 'state_expired':
      return 'The SSO sign-in request expired. Start sign-in again.'
    case 'state_invalid':
      return 'The SSO sign-in request could not be verified. Start sign-in again.'
    case 'provider_failure':
      return 'The identity provider did not complete sign-in.'
    case 'identity_conflict':
      return 'A password account already exists for this email address.'
    default:
      return code ? 'SSO sign-in did not complete. Start sign-in again.' : undefined
  }
}
