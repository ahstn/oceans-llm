import type { ReactNode } from 'react'
import { ComputerIcon, Layers01Icon, McpServerIcon } from '@hugeicons/core-free-icons'
import { AppIcon } from '@/components/icons/app-icon'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar'

export function PreviewShell({ children }: { children: ReactNode }) {
  return (
    <SidebarProvider>
      <Sidebar collapsible="icon" variant="inset">
        <SidebarHeader className="p-4">
          <div className="flex items-center gap-3">
            <span className="bg-primary text-primary-foreground flex size-8 shrink-0 items-center justify-center rounded-lg text-xs font-semibold">
              OC
            </span>
            <div className="group-data-[collapsible=icon]:hidden">
              <p className="text-sm font-medium">Oceans Gateway</p>
              <p className="text-muted-foreground text-xs">Operations console</p>
            </div>
          </div>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupLabel>Workspace</SidebarGroupLabel>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton asChild>
                  <div>
                    <AppIcon icon={ComputerIcon} aria-hidden />
                    <span>Overview</span>
                  </div>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton asChild>
                  <div>
                    <AppIcon icon={Layers01Icon} aria-hidden />
                    <span>Models</span>
                  </div>
                </SidebarMenuButton>
              </SidebarMenuItem>
              <SidebarMenuItem>
                <SidebarMenuButton asChild isActive>
                  <div aria-current="page">
                    <AppIcon icon={McpServerIcon} aria-hidden />
                    <span>MCP workspace</span>
                  </div>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroup>
        </SidebarContent>
        <SidebarFooter className="border-t p-4">
          <p className="text-muted-foreground text-xs group-data-[collapsible=icon]:hidden">
            Design preview
            <br />
            Local sample workspace
          </p>
        </SidebarFooter>
      </Sidebar>
      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-3 border-b px-4 sm:px-7">
          <SidebarTrigger />
          <Separator orientation="vertical" className="h-4" />
          <span className="text-muted-foreground text-sm">Workspace</span>
          <span className="text-muted-foreground">/</span>
          <span className="text-sm">MCP</span>
          <Badge variant="secondary" className="ml-auto">
            Preview
          </Badge>
        </header>
        <main className="mx-auto flex w-full max-w-[1600px] min-w-0 flex-1 flex-col gap-6 p-4 sm:p-7">
          {children}
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
