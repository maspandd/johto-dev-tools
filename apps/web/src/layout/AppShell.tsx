import { useQuery } from '@tanstack/react-query'
import {
  BookOpen,
  Boxes,
  ChevronRight,
  Code2,
  Database,
  FlaskConical,
  FolderOpen,
  LayoutDashboard,
  Menu,
  Moon,
  Search,
  Settings,
  Sun,
  TestTube2,
  Wrench,
  X,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, NavLink, Outlet, useParams } from 'react-router'

import { Button } from '@/components/ui/button'
import { Kbd } from '@/components/ui/misc'
import { Tooltip } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { useProject, useWorkspaces } from '@/lib/queries'
import { useTheme } from '@/lib/theme'
import { cn, isMod, modKey } from '@/lib/utils'

import { CommandPalette } from './CommandPalette'
import { ProjectSwitcher } from './ProjectSwitcher'
import { ServerControl } from './ServerControl'

const PLANNED = [
  { name: 'SQL Lab', icon: Database },
  { name: 'Data Lab', icon: FlaskConical },
  { name: 'Code Generator', icon: Code2 },
  { name: 'Testing Lab', icon: TestTube2 },
  { name: 'Developer Utilities', icon: Wrench },
]

const PROJECT_SECTIONS = [
  { to: 'endpoints', label: 'Endpoints' },
  { to: 'resources', label: 'CRUD resources' },
  { to: 'datasets', label: 'Datasets' },
  { to: 'environments', label: 'Environments' },
  { to: 'servers', label: 'Servers & history' },
  { to: 'import-export', label: 'Import / export' },
  { to: 'settings', label: 'Project settings' },
]

function navClass({ isActive }: { isActive: boolean }) {
  return cn(
    'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm transition-colors',
    isActive ? 'bg-accent font-medium text-accent-foreground' : 'text-muted-foreground hover:bg-accent/60 hover:text-foreground',
  )
}

function Sidebar({ projectId, onNavigate }: { projectId?: string; onNavigate?: () => void }) {
  return (
    <nav aria-label="Main" className="flex h-full flex-col gap-4 overflow-y-auto p-2" onClick={onNavigate}>
      <div className="flex flex-col gap-0.5">
        <NavLink to="/" end className={navClass}>
          <LayoutDashboard className="size-4" /> Dashboard
        </NavLink>
        <NavLink
          to={projectId ? `/p/${projectId}` : '/'}
          className={() => navClass({ isActive: !!projectId })}
          end={!projectId}
        >
          <Boxes className="size-4" /> API Studio
        </NavLink>
        {projectId && (
          <ul className="mt-0.5 ml-4 flex flex-col gap-0.5 border-l pl-2" aria-label="API Studio">
            {PROJECT_SECTIONS.map((s) => (
              <li key={s.to}>
                <NavLink to={`/p/${projectId}/${s.to}`} className={navClass}>
                  {s.label}
                </NavLink>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="flex flex-col gap-0.5">
        <p className="px-2 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">Coming later</p>
        {PLANNED.map(({ name, icon: Icon }) => (
          <span
            key={name}
            aria-disabled="true"
            className="flex cursor-not-allowed items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground/60"
          >
            <Icon className="size-4" /> {name}
          </span>
        ))}
      </div>

      <div className="mt-auto flex flex-col gap-0.5">
        <NavLink to="/settings" className={navClass}>
          <Settings className="size-4" /> Settings
        </NavLink>
        <a
          href="https://github.com/maspandd/johto-dev-tools#readme"
          target="_blank"
          rel="noreferrer"
          className={navClass({ isActive: false })}
        >
          <BookOpen className="size-4" /> Documentation
        </a>
      </div>
    </nav>
  )
}

function ApiStatus() {
  const health = useQuery({ queryKey: ['health'], queryFn: api.health, refetchInterval: 10_000 })
  if (health.data !== false) return null
  return (
    <div role="alert" className="border-b border-destructive/30 bg-destructive/10 px-4 py-2 text-sm">
      <strong className="text-destructive">The Workbench API is not reachable.</strong> Start it with{' '}
      <code className="rounded bg-muted px-1 font-mono text-xs">cargo run --bin workbench-server</code>, or check the
      API address in <Link to="/settings" className="underline">Settings</Link>.
    </div>
  )
}

export function AppShell() {
  const { projectId } = useParams()
  const project = useProject(projectId)
  const workspaces = useWorkspaces()
  const workspace = workspaces.data?.find((w) => w.id === project.data?.workspace_id)
  const { resolved, toggle } = useTheme()
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [drawerOpen, setDrawerOpen] = useState(false)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isMod(e) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setPaletteOpen((o) => !o)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  return (
    <div className="flex h-full flex-col">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:absolute focus:z-50 focus:m-2 focus:rounded focus:bg-primary focus:px-2 focus:py-1 focus:text-primary-foreground"
      >
        Skip to content
      </a>
      <header className="flex h-11 shrink-0 items-center gap-2 border-b bg-sidebar px-2">
        <Button
          variant="ghost"
          size="icon-sm"
          className="md:hidden"
          aria-label="Open navigation"
          onClick={() => setDrawerOpen(true)}
        >
          <Menu />
        </Button>
        <Link to="/" className="flex items-center gap-2 px-1 font-semibold">
          <span className="grid size-6 place-items-center rounded bg-primary text-[11px] font-bold text-primary-foreground">
            W
          </span>
          <span className="hidden sm:inline">Workbench</span>
        </Link>
        {project.data && (
          <div className="flex min-w-0 items-center gap-1 text-sm text-muted-foreground">
            <ChevronRight className="size-4 shrink-0" />
            <Link to="/" className="hidden max-w-40 truncate hover:text-foreground sm:inline">
              {workspace?.name ?? 'Workspace'}
            </Link>
            <ChevronRight className="hidden size-4 shrink-0 sm:inline" />
            <ProjectSwitcher project={project.data} />
          </div>
        )}
        <div className="ml-auto flex items-center gap-1.5">
          {projectId && <ServerControl projectId={projectId} />}
          <Button
            variant="outline"
            size="sm"
            className="hidden gap-2 text-muted-foreground lg:inline-flex"
            onClick={() => setPaletteOpen(true)}
          >
            <Search /> Search or run a command <Kbd>{modKey} K</Kbd>
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="lg:hidden"
            aria-label="Open command palette"
            onClick={() => setPaletteOpen(true)}
          >
            <Search />
          </Button>
          <Tooltip content={resolved === 'dark' ? 'Light theme' : 'Dark theme'}>
            <Button variant="ghost" size="icon-sm" aria-label="Toggle theme" onClick={toggle}>
              {resolved === 'dark' ? <Sun /> : <Moon />}
            </Button>
          </Tooltip>
        </div>
      </header>
      <ApiStatus />

      <div className="flex min-h-0 flex-1">
        <aside className="hidden w-56 shrink-0 border-r bg-sidebar md:block">
          <Sidebar projectId={projectId} />
        </aside>
        {drawerOpen && (
          <div className="fixed inset-0 z-40 md:hidden">
            <div className="absolute inset-0 bg-black/40" onClick={() => setDrawerOpen(false)} />
            <aside className="absolute inset-y-0 left-0 w-64 border-r bg-sidebar shadow-xl">
              <div className="flex h-11 items-center justify-between border-b px-3">
                <span className="flex items-center gap-2 font-semibold">
                  <FolderOpen className="size-4" /> Navigation
                </span>
                <Button variant="ghost" size="icon-sm" aria-label="Close navigation" onClick={() => setDrawerOpen(false)}>
                  <X />
                </Button>
              </div>
              <Sidebar projectId={projectId} onNavigate={() => setDrawerOpen(false)} />
            </aside>
          </div>
        )}
        <main id="main" className="min-w-0 flex-1 overflow-hidden">
          <Outlet />
        </main>
      </div>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} projectId={projectId} />
    </div>
  )
}
