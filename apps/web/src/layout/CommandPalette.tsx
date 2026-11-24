import { Command } from 'cmdk'
import { Boxes, FileInput, Folder, LayoutDashboard, Moon, Play, Plus, Server, Settings, Square } from 'lucide-react'
import { Dialog as DialogPrimitive } from 'radix-ui'
import type * as React from 'react'
import { useNavigate } from 'react-router'

import { MethodBadge } from '@/components/ui/misc'
import { api } from '@/lib/api'
import {
  keys,
  useAction,
  useEndpoints,
  useProjects,
  useProject,
  useResources,
  useSelectedEnvironment,
  useServers,
} from '@/lib/queries'
import { useTheme } from '@/lib/theme'

function Item({
  onSelect,
  children,
  value,
  shortcut,
}: {
  onSelect: () => void
  children: React.ReactNode
  value: string
  shortcut?: string
}) {
  return (
    <Command.Item
      value={value}
      onSelect={onSelect}
      className="flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-sm data-[selected=true]:bg-accent [&_svg]:size-4 [&_svg]:text-muted-foreground"
    >
      {children}
      {shortcut && <span className="ml-auto text-xs text-muted-foreground">{shortcut}</span>}
    </Command.Item>
  )
}

const groupClass =
  '[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:text-muted-foreground'

export function CommandPalette({
  open,
  onOpenChange,
  projectId,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  projectId?: string
}) {
  const navigate = useNavigate()
  const { toggle } = useTheme()
  const project = useProject(projectId)
  const projects = useProjects(project.data?.workspace_id)
  const endpoints = useEndpoints(open ? projectId : undefined)
  const resources = useResources(open ? projectId : undefined)
  const servers = useServers(projectId)
  const [envId] = useSelectedEnvironment(projectId ?? '')
  const start = useAction(() => api.startServer(projectId!, { environment_id: envId || null }), {
    invalidate: [keys.servers(projectId ?? '')],
    success: (s) => `Mock server running at ${s.url}`,
  })
  const stop = useAction(
    async () => {
      for (const s of servers.data ?? []) await api.stopServer(s.id)
    },
    { invalidate: [keys.servers(projectId ?? '')], success: 'Mock server stopped' },
  )

  const run = (fn: () => void) => () => {
    onOpenChange(false)
    fn()
  }
  const go = (to: string) => run(() => navigate(to))
  const running = (servers.data?.length ?? 0) > 0

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-black/40" />
        <DialogPrimitive.Content
          className="fixed top-[15vh] left-1/2 z-50 w-[min(92vw,36rem)] -translate-x-1/2 overflow-hidden rounded-lg border bg-popover shadow-2xl"
          aria-describedby={undefined}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <Command loop label="Command palette">
            <Command.Input
              autoFocus
              placeholder="Search endpoints, pages and commands…"
              className="h-11 w-full border-b bg-transparent px-3 text-sm outline-none placeholder:text-muted-foreground"
            />
            <Command.List className="max-h-[50vh] overflow-y-auto p-1">
              <Command.Empty className="p-4 text-center text-sm text-muted-foreground">No matches.</Command.Empty>

              {projectId && (
                <Command.Group heading="Actions" className={groupClass}>
                  <Item value="new endpoint" onSelect={go(`/p/${projectId}/endpoints?new=1`)}>
                    <Plus /> New endpoint
                  </Item>
                  {running ? (
                    <Item value="stop mock server" onSelect={run(() => stop.mutate(undefined))}>
                      <Square /> Stop mock server
                    </Item>
                  ) : (
                    <Item value="start mock server" onSelect={run(() => start.mutate(undefined))}>
                      <Play /> Start mock server
                    </Item>
                  )}
                  <Item value="import openapi curl archive" onSelect={go(`/p/${projectId}/import-export`)}>
                    <FileInput /> Import OpenAPI, cURL or an archive
                  </Item>
                </Command.Group>
              )}

              {(endpoints.data?.length ?? 0) > 0 && (
                <Command.Group heading="Endpoints" className={groupClass}>
                  {endpoints.data!.map((e) => (
                    <Item
                      key={e.id}
                      value={`endpoint ${e.method} ${e.path} ${e.name} ${e.id}`}
                      onSelect={go(`/p/${projectId}/endpoints/${e.id}`)}
                    >
                      <MethodBadge method={e.method} />
                      <span className="font-mono text-xs">{e.path}</span>
                      <span className="truncate text-muted-foreground">{e.name}</span>
                    </Item>
                  ))}
                </Command.Group>
              )}

              {(resources.data?.length ?? 0) > 0 && (
                <Command.Group heading="CRUD resources" className={groupClass}>
                  {resources.data!.map((r) => (
                    <Item key={r.id} value={`resource ${r.name} ${r.path} ${r.id}`} onSelect={go(`/p/${projectId}/resources/${r.id}`)}>
                      <Server /> {r.name} <span className="font-mono text-xs text-muted-foreground">{r.path}</span>
                    </Item>
                  ))}
                </Command.Group>
              )}

              {projectId && (
                <Command.Group heading="Go to" className={groupClass}>
                  {[
                    ['endpoints', 'Endpoints'],
                    ['resources', 'CRUD resources'],
                    ['datasets', 'Datasets'],
                    ['environments', 'Environments'],
                    ['servers', 'Servers & history'],
                    ['import-export', 'Import / export'],
                    ['settings', 'Project settings'],
                  ].map(([to, label]) => (
                    <Item key={to} value={`go ${label}`} onSelect={go(`/p/${projectId}/${to}`)}>
                      <Boxes /> {label}
                    </Item>
                  ))}
                </Command.Group>
              )}

              {(projects.data?.length ?? 0) > 1 && (
                <Command.Group heading="Projects" className={groupClass}>
                  {projects.data!
                    .filter((p) => p.id !== projectId)
                    .map((p) => (
                      <Item key={p.id} value={`project ${p.name} ${p.id}`} onSelect={go(`/p/${p.id}`)}>
                        <Folder /> {p.name}
                      </Item>
                    ))}
                </Command.Group>
              )}

              <Command.Group heading="General" className={groupClass}>
                <Item value="dashboard workspaces projects" onSelect={go('/')}>
                  <LayoutDashboard /> Dashboard
                </Item>
                <Item value="settings connection token" onSelect={go('/settings')}>
                  <Settings /> Settings
                </Item>
                <Item value="toggle theme dark light" onSelect={run(toggle)}>
                  <Moon /> Toggle dark / light theme
                </Item>
              </Command.Group>
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}
