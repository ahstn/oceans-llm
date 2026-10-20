import { useState, type CSSProperties } from 'react'
import { Configuration01Icon, ShieldKeyIcon, ViewIcon } from '@hugeicons/core-free-icons'

import { SectionNavSidebar, SectionNavTabs } from '@/components/shared/section-nav'
import { SidebarProvider } from '@/components/ui/sidebar'

const sections = [
  { id: 'overview', label: 'Overview', icon: ViewIcon },
  { id: 'configuration', label: 'Configuration', icon: Configuration01Icon },
  { id: 'credentials', label: 'Credentials', icon: ShieldKeyIcon },
] as const

type Section = (typeof sections)[number]['id']

const sectionCopy: Record<Section, string> = {
  overview: 'Status, ownership and recent activity.',
  configuration: 'The settings form, saved from the dialog footer.',
  credentials: 'Secrets bound to this server.',
}

/** The section navigation that detail dialogs use, laid out as it sits inside one. */
export function ShowcaseSectionNav() {
  const [section, setSection] = useState<Section>('overview')

  return (
    <div className="bg-popover overflow-hidden rounded-lg border">
      <SidebarProvider
        className="min-h-0 min-w-0 items-start"
        style={{ '--sidebar-width': '14rem' } as CSSProperties}
      >
        <SectionNavSidebar
          sections={sections}
          activeSection={section}
          onSectionChange={setSection}
        />
        <div className="flex min-h-48 min-w-0 flex-1 flex-col gap-4 p-4">
          <SectionNavTabs
            sections={sections}
            activeSection={section}
            onSectionChange={setSection}
          />
          <p className="text-muted-foreground text-sm">{sectionCopy[section]}</p>
        </div>
      </SidebarProvider>
    </div>
  )
}
