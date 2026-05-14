import { ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import { adminNavSections, matchesAdminPath } from '@/components/layout/admin-nav'
import { GeneratedAvatar } from '@/components/ui/generated-avatar'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from '@/components/ui/sidebar'
import type { AuthSessionView } from '@/types/api'

interface AppSidebarProps {
  currentPath: string
  session: AuthSessionView
  signOutPending: boolean
  onSignOut: () => void
}

export function AppSidebar({ currentPath, session, signOutPending, onSignOut }: AppSidebarProps) {
  return (
    <Sidebar collapsible="icon" variant="inset">
      <SidebarHeader className="gap-3 p-3 pb-2">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              size="lg"
              className="hover:text-sidebar-foreground active:text-sidebar-foreground h-auto cursor-default rounded-lg px-1 py-1 opacity-100 hover:bg-transparent active:bg-transparent"
            >
              <div>
                <span className="bg-sidebar-primary text-sidebar-primary-foreground flex size-8 items-center justify-center rounded-lg">
                  OC
                </span>
                <div className="grid min-w-0 flex-1 text-left leading-tight group-data-[collapsible=icon]:hidden">
                  <span className="text-sidebar-foreground truncate text-sm font-medium">
                    Oceans Gateway
                  </span>
                  <span className="text-sidebar-foreground/70 truncate text-xs">Control plane</span>
                </div>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent className="px-2 py-3">
        {adminNavSections.map((section) => (
          <SidebarGroup key={section.label} className="px-0 py-1">
            <SidebarGroupLabel className="px-2 text-xs font-medium">
              {section.label}
            </SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu className="gap-1">
                {section.items.map((item) => {
                  const active = matchesAdminPath(currentPath, item.to)

                  return (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton
                        asChild
                        tooltip={item.label}
                        isActive={active}
                        className="h-8 rounded-lg px-2 text-sm font-normal"
                      >
                        <Link to={item.to}>
                          <AppIcon icon={item.icon} size={16} stroke={1.5} />
                          <span>{item.label}</span>
                        </Link>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  )
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>

      <SidebarFooter className="border-sidebar-border/70 gap-3 border-t p-3">
        <SidebarMenu>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <SidebarMenuButton size="lg" className="h-auto rounded-lg px-2 py-2">
                  <GeneratedAvatar
                    kind="user"
                    name={session.user.name || session.user.email}
                    className="size-8 rounded-lg"
                    size={32}
                    square
                  />
                  <div className="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
                    <span className="truncate font-medium">{session.user.name}</span>
                    <span className="text-sidebar-foreground/70 truncate text-xs">
                      {session.user.email}
                    </span>
                  </div>
                  <AppIcon
                    icon={ArrowRight01Icon}
                    size={16}
                    stroke={1.5}
                    className="ml-auto rotate-[-90deg] group-data-[collapsible=icon]:hidden"
                  />
                </SidebarMenuButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent side="top" align="start" className="w-64">
                <DropdownMenuLabel className="grid gap-1">
                  <span className="truncate text-sm font-medium">{session.user.name}</span>
                  <span className="text-muted-foreground truncate text-xs font-normal">
                    {session.user.email}
                  </span>
                  <span className="text-muted-foreground text-xs font-normal">
                    {formatRole(session.user.global_role)}
                  </span>
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem asChild>
                  <Link to="/change-password">Change password</Link>
                </DropdownMenuItem>
                <DropdownMenuItem
                  variant="destructive"
                  disabled={signOutPending}
                  onSelect={(event) => {
                    event.preventDefault()
                    onSignOut()
                  }}
                >
                  {signOutPending ? 'Signing out...' : 'Sign out'}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>

      <SidebarRail />
    </Sidebar>
  )
}

function formatRole(role: string) {
  return role
    .split('_')
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(' ')
}
