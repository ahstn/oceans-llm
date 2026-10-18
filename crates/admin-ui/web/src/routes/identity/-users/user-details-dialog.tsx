import type { CSSProperties } from 'react'
import { Cancel01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { SectionNavSidebar, SectionNavTabs } from '@/components/shared/section-nav'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import { SidebarProvider } from '@/components/ui/sidebar'
import { EntityTagBadges } from '@/routes/identity/-entity-tags'
import { ProviderConfiguration } from '@/routes/identity/-users/provider-configuration'
import { statusBadgeVariant } from '@/routes/identity/-status-badge'
import { useOnboardingReset } from '@/routes/identity/-users/use-onboarding-reset'
import type { ProviderCredentials } from '@/routes/identity/-users/use-provider-credentials'
import { useUserDetails, type UserDetails } from '@/routes/identity/-users/use-user-details'
import type { IdentityTransition } from '@/routes/identity/-use-identity-transition'
import { UserAuthSection } from '@/routes/identity/-users/user-auth-section'
import {
  UserConfigurationSection,
  UserOverviewSection,
  UserUsageSection,
} from '@/routes/identity/-users/user-details-sections'
import { isSsoProviderMissing, type UserFormOptions } from '@/routes/identity/-user-form'
import { userDetailsSections, type UserDetailsSection } from '@/routes/identity/-users/users-search'
import type { IdentityUsersPayload, UserView } from '@/types/api'

/** The `Manage user` dialog, open while the URL selects a user, with sidebar section navigation. */
export function UserDetailsDialog({
  user,
  section,
  teams,
  oidcProviders,
  oauthProviders,
  copilotUserProviders,
  transition,
}: UserFormOptions & {
  user: UserView | null
  section: UserDetailsSection
  copilotUserProviders: IdentityUsersPayload['copilot_user_providers']
  transition: IdentityTransition
}) {
  const details = useUserDetails({ user, oidcProviders, oauthProviders, transition })

  return (
    <Dialog
      open={Boolean(user)}
      onOpenChange={(open) => {
        if (!open) {
          details.close()
        }
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="overflow-hidden p-0 md:max-h-[680px] md:max-w-[920px]"
      >
        <DialogTitle className="sr-only">Manage user</DialogTitle>
        <DialogDescription className="sr-only">
          Review user status, configuration, auth settings, and usage.
        </DialogDescription>

        {user ? (
          <SidebarProvider
            className="min-h-0 items-start"
            style={{ '--sidebar-width': '14rem' } as CSSProperties}
          >
            <SectionNavSidebar
              sections={userDetailsSections}
              activeSection={section}
              onSectionChange={details.showSection}
            />

            <main className="flex max-h-[680px] min-h-[520px] flex-1 flex-col overflow-hidden">
              <UserDetailsHeader
                user={user}
                activeSection={section}
                onSectionChange={details.showSection}
              />

              {section === 'provider-configuration' ? (
                <UserProviderPanel
                  user={user}
                  providers={copilotUserProviders}
                  credentials={details.credentials}
                  isPending={transition.isPending}
                  onClose={details.close}
                />
              ) : null}

              <UserDetailsForm
                key={user.id}
                user={user}
                section={section}
                details={details}
                teams={teams}
                oidcProviders={oidcProviders}
                oauthProviders={oauthProviders}
                transition={transition}
              />
            </main>
          </SidebarProvider>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

interface SectionNavigationProps {
  activeSection: UserDetailsSection
  onSectionChange: (section: UserDetailsSection) => void
}

/** Name, email, tags, and status, with section tabs in place of the sidebar on small screens. */
function UserDetailsHeader({
  user,
  activeSection,
  onSectionChange,
}: SectionNavigationProps & { user: UserView }) {
  return (
    <header className="border-border flex shrink-0 flex-col gap-4 border-b px-6 py-5">
      <div className="flex items-start gap-3">
        <GeneratedAvatar kind="user" name={user.name || user.email} size={44} />
        <div className="min-w-0 flex-1 pt-0.5">
          <h2 className="text-foreground truncate text-lg leading-tight font-semibold">
            {user.name}
          </h2>
          <p className="text-subtle-foreground mt-1 truncate text-sm">{user.email}</p>
          <div className="mt-2 flex flex-wrap gap-2">
            <EntityTagBadges tags={user.tags} />
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Badge variant={statusBadgeVariant(user.status)}>{user.status}</Badge>
          <DialogClose asChild>
            <Button type="button" variant="ghost" size="icon-sm" aria-label="Close">
              <AppIcon icon={Cancel01Icon} stroke={1.5} aria-hidden />
            </Button>
          </DialogClose>
        </div>
      </div>

      <SectionNavTabs
        sections={userDetailsSections}
        activeSection={activeSection}
        onSectionChange={onSectionChange}
      />
    </header>
  )
}

/** Provider token forms, outside the user form so each token saves on its own. */
function UserProviderPanel({
  user,
  providers,
  credentials,
  isPending,
  onClose,
}: {
  user: UserView
  providers: IdentityUsersPayload['copilot_user_providers']
  credentials: ProviderCredentials
  isPending: boolean
  onClose: () => void
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex-1 overflow-y-auto p-6">
        <ProviderConfiguration
          userId={user.id}
          providers={providers}
          tokens={credentials.tokens}
          isPending={isPending}
          onTokenChange={(providerKey, token) =>
            credentials.updateToken(user.id, providerKey, token)
          }
          onSave={credentials.save}
          onRemove={credentials.remove}
        />
      </div>
      <DialogFooter className="border-border mx-0 mb-0 rounded-none border-t px-6 py-4">
        <Button type="button" variant="secondary" onClick={onClose}>
          Close
        </Button>
      </DialogFooter>
    </div>
  )
}

/**
 * The editable sections share one form. It stays mounted but hidden on the provider section, whose
 * token forms render outside it. It's keyed by user, so a reissued onboarding URL never outlives
 * the user it was generated for.
 */
function UserDetailsForm({
  user,
  section,
  details,
  teams,
  oidcProviders,
  oauthProviders,
  transition,
}: UserFormOptions & {
  user: UserView
  section: UserDetailsSection
  details: UserDetails
  transition: IdentityTransition
}) {
  const { isPending } = transition
  const onboarding = useOnboardingReset({
    user,
    updateForm: details.updateForm,
    oidcProviders,
    oauthProviders,
    transition,
  })

  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      hidden={section === 'provider-configuration'}
      onSubmit={details.saveUser}
    >
      <div className="flex-1 overflow-y-auto p-6">
        {section === 'overview' ? <UserOverviewSection user={user} /> : null}
        {section === 'configuration' ? (
          <UserConfigurationSection
            user={user}
            form={details.updateForm}
            setForm={details.setUpdateForm}
            teams={teams}
          />
        ) : null}
        {section === 'auth' ? (
          <UserAuthSection
            user={user}
            details={details}
            onboarding={onboarding}
            oidcProviders={oidcProviders}
            oauthProviders={oauthProviders}
            isPending={isPending}
          />
        ) : null}
        {section === 'usage' ? <UserUsageSection /> : null}
      </div>

      <DialogFooter className="border-border mx-0 mb-0 rounded-none border-t px-6 py-4">
        <Button type="button" variant="secondary" onClick={details.close}>
          Close
        </Button>
        {section === 'configuration' || section === 'auth' ? (
          <Button
            type="submit"
            disabled={
              isPending || isSsoProviderMissing(details.updateForm, oidcProviders, oauthProviders)
            }
          >
            {isPending ? 'Saving…' : 'Save changes'}
          </Button>
        ) : null}
      </DialogFooter>
    </form>
  )
}
