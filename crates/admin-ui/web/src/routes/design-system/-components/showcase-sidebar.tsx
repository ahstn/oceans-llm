import { HugeiconsIcon } from '@hugeicons/react'
import {
  HomeIcon,
  Key01Icon,
  McpServerIcon,
  SaveMoneyDollarIcon,
  UserCircleIcon,
  UserGroupIcon,
} from '@hugeicons/core-free-icons'

import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'

const groups = [
  {
    label: 'Personal',
    items: [
      { label: 'Profile', icon: UserCircleIcon, active: true },
      { label: 'API Keys', icon: Key01Icon, badge: '3' },
    ],
  },
  {
    label: 'Platform',
    items: [
      { label: 'Overview', icon: HomeIcon },
      { label: 'Teams', icon: UserGroupIcon },
      { label: 'MCP servers', icon: McpServerIcon },
      { label: 'Spend controls', icon: SaveMoneyDollarIcon },
    ],
  },
] as const

/**
 * A static, non-collapsible copy of the app sidebar. The page already sits inside the app's
 * SidebarProvider, so this reuses that context instead of nesting a second provider.
 */
export function ShowcaseSidebar() {
  return (
    <div className="bg-sidebar overflow-hidden rounded-lg border">
      <Sidebar collapsible="none" className="w-full">
        <SidebarContent>
          {groups.map((group) => (
            <SidebarGroup key={group.label}>
              <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {group.items.map((item) => (
                    <SidebarMenuItem key={item.label}>
                      <SidebarMenuButton isActive={'active' in item && item.active}>
                        <HugeiconsIcon icon={item.icon} />
                        <span>{item.label}</span>
                      </SidebarMenuButton>
                      {'badge' in item ? <SidebarMenuBadge>{item.badge}</SidebarMenuBadge> : null}
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
        </SidebarContent>
      </Sidebar>
    </div>
  )
}
