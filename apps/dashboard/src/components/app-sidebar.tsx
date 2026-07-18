import { NavLink, useLocation } from 'react-router-dom'
import {
  Activity,
  ArchiveX,
  Bell,
  Boxes,
  Database,
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
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from '@/components/ui/sidebar'

export type PageId =
  | 'overview'
  | 'health'
  | 'requests'
  | 'database'
  | 'administrators'
  | 'organizations'
  | 'users'
  | 'storage'
  | 'configuration'
  | 'events'
  | 'jobs'
  | 'audit'
  | 'notifications'

type NavItem = { path: string; title: string; icon: typeof Gauge }

const OPERATIONS: NavItem[] = [
  { path: '/', title: 'Overview', icon: Gauge },
  { path: '/health', title: 'Health', icon: HeartPulse },
  { path: '/requests', title: 'Requests', icon: Activity },
  { path: '/database', title: 'Database', icon: Database },
  { path: '/administrators', title: 'Administrators', icon: ShieldCheck },
  { path: '/configuration', title: 'Configuration', icon: Settings2 },
]

const PLATFORM: NavItem[] = [
  { path: '/users', title: 'Users', icon: Users },
  { path: '/organizations', title: 'Organizations', icon: Users },
  { path: '/storage', title: 'Storage', icon: ArchiveX },
  { path: '/events', title: 'Events', icon: Workflow },
  { path: '/jobs', title: 'Jobs', icon: ListTree },
  { path: '/notifications', title: 'Notifications', icon: Bell },
  { path: '/audit', title: 'Audit', icon: FileClock },
]

function NavMenu({ items }: { items: NavItem[] }) {
  const location = useLocation()
  const { isMobile, setOpenMobile } = useSidebar()

  return (
    <SidebarMenu>
      {items.map((item) => (
        <SidebarMenuItem key={item.path}>
          <SidebarMenuButton
            asChild
            isActive={location.pathname === item.path}
          >
            <NavLink
              to={item.path}
              onClick={() => {
                if (isMobile) setOpenMobile(false)
              }}
            >
              <item.icon />
              <span>{item.title}</span>
            </NavLink>
          </SidebarMenuButton>
        </SidebarMenuItem>
      ))}
    </SidebarMenu>
  )
}

export function AppSidebar() {
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
            <NavMenu items={OPERATIONS} />
          </SidebarGroupContent>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Platform</SidebarGroupLabel>
          <SidebarGroupContent>
            <NavMenu items={PLATFORM} />
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
