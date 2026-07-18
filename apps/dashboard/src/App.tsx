import { useEffect, useState } from 'react'
import { Navigate, Route, Routes, useLocation } from 'react-router-dom'
import { CircleUserRound, LogOut, Moon, Sun } from 'lucide-react'
import { AppSidebar, type PageId } from '@/components/app-sidebar'
import { ConnectCard } from '@/components/connect-card'
import { StatusBadge } from '@/components/status-badge'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useAdminQuery } from '@/hooks/use-admin-query'
import {
  clearToken,
  getToken,
  setToken,
  type AdminPrincipal,
  type Health,
} from '@/lib/api'
import { AdministratorsPage } from '@/pages/administrators'
import { AuditPage } from '@/pages/audit'
import { ConfigurationPage } from '@/pages/configuration'
import { DatabasePage } from '@/pages/database'
import { HealthPage } from '@/pages/health'
import { EventsPage } from '@/pages/events'
import { JobsPage } from '@/pages/jobs'
import { NotificationsPage } from '@/pages/notifications'
import { OrganizationsPage } from '@/pages/organizations'
import { OverviewPage } from '@/pages/overview'
import { RequestsPage } from '@/pages/requests'
import { StoragePage } from '@/pages/storage'
import { UsersPage } from '@/pages/users'

const PAGE_TITLES: Record<PageId, string> = {
  overview: 'Overview',
  health: 'Health',
  requests: 'Requests',
  database: 'Database',
  administrators: 'Administrators',
  organizations: 'Organizations',
  users: 'Users',
  storage: 'Storage',
  configuration: 'Configuration',
  events: 'Events',
  jobs: 'Jobs',
  audit: 'Audit',
  notifications: 'Notifications',
}

function pageFromPath(pathname: string): PageId {
  const segment = pathname.split('/').filter(Boolean)[0] ?? ''
  return segment in PAGE_TITLES ? (segment as PageId) : 'overview'
}

function useTheme() {
  const [dark, setDark] = useState(
    () => localStorage.getItem('boson.theme') === 'dark',
  )
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
    localStorage.setItem('boson.theme', dark ? 'dark' : 'light')
  }, [dark])
  return { dark, toggle: () => setDark((value) => !value) }
}

function Shell({ onDisconnect }: { onDisconnect: () => void }) {
  const location = useLocation()
  const page = pageFromPath(location.pathname)
  const { dark, toggle } = useTheme()
  const health = useAdminQuery<Health>('health', 15_000)
  const session = useAdminQuery<AdminPrincipal>('admin-session', 60_000)

  useEffect(() => {
    if (session.unauthorized) {
      onDisconnect()
    }
  }, [session.unauthorized, onDisconnect])

  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <header className="flex h-14 items-center gap-3 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="h-5" />
          <h1 className="text-sm font-semibold">{PAGE_TITLES[page]}</h1>
          <div className="ml-auto flex items-center gap-2">
            {health.data && <StatusBadge status={health.data.status} />}
            {session.data && (
              <div className="hidden items-center gap-2 sm:flex">
                <CircleUserRound className="size-4 text-muted-foreground" />
                <span className="max-w-44 truncate text-xs font-medium">
                  {session.data.email ?? 'Bootstrap administrator'}
                </span>
                {session.data.bootstrap && (
                  <Badge variant="outline" className="text-[10px]">
                    bootstrap
                  </Badge>
                )}
              </div>
            )}
            <Button variant="ghost" size="icon" onClick={toggle}>
              {dark ? <Sun data-slot="icon" /> : <Moon data-slot="icon" />}
            </Button>
            <Button variant="outline" size="sm" onClick={onDisconnect}>
              <LogOut data-slot="icon" /> Disconnect
            </Button>
          </div>
        </header>
        <main className="flex-1 p-4 md:p-6">
          <Routes>
            <Route path="/" element={<OverviewPage />} />
            <Route path="/overview" element={<Navigate to="/" replace />} />
            <Route path="/health" element={<HealthPage />} />
            <Route path="/requests" element={<RequestsPage />} />
            <Route
              path="/database"
              element={<DatabasePage principal={session.data} />}
            />
            <Route
              path="/administrators"
              element={<AdministratorsPage principal={session.data} />}
            />
            <Route path="/organizations" element={<OrganizationsPage />} />
            <Route path="/users" element={<UsersPage />} />
            <Route path="/storage" element={<StoragePage />} />
            <Route path="/configuration" element={<ConfigurationPage />} />
            <Route path="/events" element={<EventsPage />} />
            <Route
              path="/jobs"
              element={<JobsPage principal={session.data} />}
            />
            <Route path="/notifications" element={<NotificationsPage />} />
            <Route path="/audit" element={<AuditPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}

function App() {
  const [connected, setConnected] = useState(() => Boolean(getToken()))
  const [connectError, setConnectError] = useState<string | undefined>()

  if (!connected) {
    return (
      <TooltipProvider>
        <ConnectCard
          error={connectError}
          onConnect={(token) => {
            setToken(token)
            setConnectError(undefined)
            setConnected(true)
          }}
        />
      </TooltipProvider>
    )
  }

  return (
    <TooltipProvider>
      <Shell
        onDisconnect={() => {
          clearToken()
          setConnectError(undefined)
          setConnected(false)
        }}
      />
    </TooltipProvider>
  )
}

export default App
