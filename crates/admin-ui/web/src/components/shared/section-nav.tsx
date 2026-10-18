import type { IconSvgElement } from '@hugeicons/react'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'

/** One section of a detail dialog. */
export interface DetailSection<Id extends string> {
  id: Id
  label: string
  icon: IconSvgElement
}

interface SectionNavProps<Id extends string> {
  sections: ReadonlyArray<DetailSection<Id>>
  activeSection: Id
  onSectionChange: (section: Id) => void
}

/**
 * Section navigation for a detail dialog: a fixed sidebar from `md` up. Render it inside a
 * `SidebarProvider`, and pair it with `SectionNavTabs` in the dialog header for small screens.
 */
export function SectionNavSidebar<Id extends string>({
  sections,
  activeSection,
  onSectionChange,
}: SectionNavProps<Id>) {
  return (
    <Sidebar collapsible="none" className="border-border hidden border-r md:flex">
      <SidebarContent className="p-3">
        <SidebarGroup className="px-0 py-0">
          <SidebarGroupContent>
            <SidebarMenu className="gap-1">
              {sections.map((section) => (
                <SidebarMenuItem key={section.id}>
                  <SidebarMenuButton
                    type="button"
                    className="h-10 px-3 py-2"
                    isActive={activeSection === section.id}
                    onClick={() => onSectionChange(section.id)}
                  >
                    <AppIcon icon={section.icon} stroke={1.5} aria-hidden />
                    <span>{section.label}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  )
}

/** The same sections as a row of buttons, shown below `md` where the sidebar is hidden. */
export function SectionNavTabs<Id extends string>({
  sections,
  activeSection,
  onSectionChange,
}: SectionNavProps<Id>) {
  return (
    <div className="flex gap-2 overflow-x-auto md:hidden">
      {sections.map((section) => (
        <Button
          key={section.id}
          type="button"
          size="sm"
          variant={activeSection === section.id ? 'secondary' : 'ghost'}
          onClick={() => onSectionChange(section.id)}
        >
          <AppIcon icon={section.icon} stroke={1.5} aria-hidden data-icon="inline-start" />
          {section.label}
        </Button>
      ))}
    </div>
  )
}
