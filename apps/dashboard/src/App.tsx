import { useEffect, useState } from 'react'
import { LogOut, Moon, Sun } from 'lucide-react'
import { AppSidebar, type PageId } from '@/components/app-sidebar'
import { ConnectCard } from '@/components/connect-card'
import { StatusBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar'
import { TooltipProvider } from '@/components/ui/tooltip'
import { useAdminQuery } from '@/hooks/use-admin-query'
import { clearToken, getToken, setToken, type Health } from '@/lib/api'
import { ConfigurationPage } from '@/pages/configuration'
import { HealthPage } from '@/pages/health'
import { OverviewPage } from '@/pages/overview'
import { RequestsPage } from '@/pages/requests'

const PAGE_TITLES: Record<PageId, string> = {
  overview: 'Overview',
  health: 'Health',
  requests: 'Requests',
  configuration: 'Configuration',
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
  const [page, setPage] = useState<PageId>('overview')
  const { dark, toggle } = useTheme()
  const health = useAdminQuery<Health>('health', 15_000)

  return (
    <SidebarProvider>
      <AppSidebar page={page} onNavigate={setPage} />
      <SidebarInset>
        <header className="flex h-14 items-center gap-3 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="h-5" />
          <h1 className="text-sm font-semibold">{PAGE_TITLES[page]}</h1>
          <div className="ml-auto flex items-center gap-2">
            {health.data && <StatusBadge status={health.data.status} />}
            <Button variant="ghost" size="icon" onClick={toggle}>
              {dark ? <Sun data-slot="icon" /> : <Moon data-slot="icon" />}
            </Button>
            <Button variant="outline" size="sm" onClick={onDisconnect}>
              <LogOut data-slot="icon" /> Disconnect
            </Button>
          </div>
        </header>
        <main className="flex-1 p-4 md:p-6">
          {page === 'overview' && <OverviewPage />}
          {page === 'health' && <HealthPage />}
          {page === 'requests' && <RequestsPage />}
          {page === 'configuration' && <ConfigurationPage />}
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
