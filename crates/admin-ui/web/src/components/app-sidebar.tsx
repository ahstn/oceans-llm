import { ArrowRight01Icon } from '@hugeicons/core-free-icons'
import { Link } from '@tanstack/react-router'

import { AppIcon } from '@/components/icons/app-icon'
import {
  adminNavSections,
  getActiveNavSection,
  matchesAdminPath,
} from '@/components/layout/admin-nav'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarRail,
} from '@/components/ui/sidebar'
import type { AuthSessionView } from '@/types/api'

interface AppSidebarProps {
  currentPath: string
  session: AuthSessionView
}

export function AppSidebar({ currentPath, session }: AppSidebarProps) {
  const activeSection = getActiveNavSection(currentPath)

  return (
    <Sidebar collapsible="icon" variant="inset">
      <SidebarHeader className="border-sidebar-border/70 gap-3 border-b p-3">
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              size="lg"
              className="border-sidebar-border/70 bg-sidebar-accent/40 hover:bg-sidebar-accent/40 h-auto cursor-default rounded-xl border px-3 py-3 opacity-100"
            >
              <span className="bg-sidebar-primary text-sidebar-primary-foreground flex size-9 items-center justify-center rounded-lg">
                OC
              </span>
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="text-sidebar-foreground truncate font-medium">Oceans Gateway</span>
                <span className="text-sidebar-foreground/70 truncate text-xs">
                  Control plane · admin
                </span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent className="px-2 py-3">
        <SidebarMenu className="gap-2">
          {adminNavSections.map((section) => {
            const isSectionActive = section.items.some((item) =>
              matchesAdminPath(currentPath, item.to),
            )

            return (
              <Collapsible
                key={`${section.label}-${isSectionActive}`}
                asChild
                defaultOpen={activeSection?.label === section.label}
                className="group/collapsible"
              >
                <SidebarMenuItem>
                  <CollapsibleTrigger asChild>
                    <SidebarMenuButton
                      tooltip={section.label}
                      isActive={isSectionActive}
                      className="rounded-xl"
                    >
                      <AppIcon icon={section.icon} size={16} stroke={1.5} />
                      <span>{section.label}</span>
                      <AppIcon
                        icon={ArrowRight01Icon}
                        size={16}
                        stroke={1.5}
                        className="ml-auto transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90"
                      />
                    </SidebarMenuButton>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <SidebarMenuSub className="mx-0 mt-1 border-l-0 px-0 py-0 pl-2">
                      {section.items.map((item) => {
                        const active = matchesAdminPath(currentPath, item.to)

                        return (
                          <SidebarMenuSubItem key={item.to}>
                            <SidebarMenuSubButton asChild isActive={active}>
                              <Link to={item.to}>
                                <AppIcon icon={item.icon} size={15} stroke={1.5} />
                                <span>{item.label}</span>
                              </Link>
                            </SidebarMenuSubButton>
                          </SidebarMenuSubItem>
                        )
                      })}
                    </SidebarMenuSub>
                  </CollapsibleContent>
                </SidebarMenuItem>
              </Collapsible>
            )
          })}
        </SidebarMenu>
      </SidebarContent>

      <SidebarFooter className="border-sidebar-border/70 gap-3 border-t p-3">
        <div className="border-sidebar-border/70 bg-sidebar-accent/35 text-sidebar-foreground/75 rounded-xl border p-3 text-xs group-data-[collapsible=icon]:hidden">
          <p className="text-sidebar-foreground font-medium">Local preview mode</p>
          <p className="mt-1 leading-relaxed">
            Gateway-backed admin flows are live here. Model inventory remains preview-only.
          </p>
        </div>

        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild size="lg" className="h-auto rounded-xl px-3 py-3">
              <Link to="/change-password">
                <Avatar className="size-8 rounded-lg">
                  <AvatarFallback className="bg-sidebar-primary/15 text-sidebar-primary rounded-lg">
                    {getInitials(session.user.name)}
                  </AvatarFallback>
                </Avatar>
                <div className="grid flex-1 text-left text-sm leading-tight">
                  <span className="truncate font-medium">{session.user.name}</span>
                  <span className="text-sidebar-foreground/70 truncate text-xs">
                    {session.user.email}
                  </span>
                </div>
                <span className="border-sidebar-border/70 text-sidebar-foreground/70 hidden rounded-full border px-2 py-0.5 text-[10px] font-medium tracking-[0.14em] uppercase group-data-[collapsible=icon]:hidden xl:inline-flex">
                  {session.user.global_role}
                </span>
              </Link>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>

      <SidebarRail />
    </Sidebar>
  )
}

function getInitials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? '')
    .join('')
}
