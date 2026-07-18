import {
  Activity,
  ArchiveX,
  Boxes,
  FileClock,
  Gauge,
  HeartPulse,
  ListTree,
  Settings2,
  ShieldCheck,
  Users,
  Workflow,
} from 'lucide-react'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/ui/sidebar'

export type PageId =
  | 'overview'
  | 'health'
  | 'requests'
  | 'administrators'
  | 'organizations'
  | 'configuration'
  | 'events'
  | 'jobs'

const OPERATIONS: Array<{ id: PageId; title: string; icon: typeof Gauge }> = [
  { id: 'overview', title: 'Overview', icon: Gauge },
  { id: 'health', title: 'Health', icon: HeartPulse },
  { id: 'requests', title: 'Requests', icon: Activity },
  { id: 'administrators', title: 'Administrators', icon: ShieldCheck },
  { id: 'configuration', title: 'Configuration', icon: Settings2 },
]

const PLATFORM: Array<{ id: PageId; title: string; icon: typeof Gauge }> = [
  { id: 'organizations', title: 'Organizations', icon: Users },
  { id: 'events', title: 'Events', icon: Workflow },
  { id: 'jobs', title: 'Jobs', icon: ListTree },
]

const COMING_SOON = [
  { title: 'Users', icon: Users },
  { title: 'Storage', icon: ArchiveX },
  { title: 'Audit', icon: FileClock },
]

export function AppSidebar({
  page,
  onNavigate,
}: {
  page: PageId
  onNavigate: (page: PageId) => void
}) {
  return (
    <Sidebar>
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1.5">
          <div className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Boxes className="size-4" />
          </div>
          <div className="grid leading-tight">
            <span className="text-sm font-semibold">Boson</span>
            <span className="text-xs text-muted-foreground">Backend Platform</span>
          </div>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Operations</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {OPERATIONS.map((item) => (
                <SidebarMenuItem key={item.id}>
                  <SidebarMenuButton
                    isActive={page === item.id}
                    onClick={() => onNavigate(item.id)}
                  >
                    <item.icon />
                    <span>{item.title}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Platform</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {PLATFORM.map((item) => (
                <SidebarMenuItem key={item.id}>
                  <SidebarMenuButton isActive={page === item.id} onClick={() => onNavigate(item.id)}>
                    <item.icon />
                    <span>{item.title}</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
              {COMING_SOON.map((item) => (
                <SidebarMenuItem key={item.title}>
                  <SidebarMenuButton disabled className="opacity-60">
                    <item.icon />
                    <span>{item.title}</span>
                  </SidebarMenuButton>
                  <SidebarMenuBadge className="text-muted-foreground">
                    soon
                  </SidebarMenuBadge>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <p className="px-2 pb-1 text-xs text-muted-foreground">
          Admin API · <code className="font-mono">/admin/v1</code>
        </p>
      </SidebarFooter>
    </Sidebar>
  )
}
