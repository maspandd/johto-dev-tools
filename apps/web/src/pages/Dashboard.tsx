import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Archive,
  ArchiveRestore,
  Copy,
  FileArchive,
  FolderPlus,
  MoreHorizontal,
  Pencil,
  Plus,
  Sparkles,
  Trash2,
} from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'

import { ArchiveImportDialog } from '@/components/ArchiveImport'
import { ConfirmDialog, NameDialog } from '@/components/dialogs'
import { Button } from '@/components/ui/button'
import { Checkbox, Field, Input } from '@/components/ui/form'
import { Badge, EmptyState, ErrorNotice, Spinner } from '@/components/ui/misc'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/overlay'
import { api, describeError } from '@/lib/api'
import { createDemoProject } from '@/lib/demo'
import { keys, useStoredString } from '@/lib/queries'
import type { Project, Workspace } from '@/lib/types'
import { cn, relativeTime } from '@/lib/utils'

function ProjectCard({ project, onChanged }: { project: Project; onChanged: () => void }) {
  const navigate = useNavigate()
  const [confirm, setConfirm] = useState(false)
  const archived = !!project.archived_at
  return (
    <div
      className={cn(
        'group relative flex flex-col gap-2 rounded-lg border bg-card p-4 transition-colors hover:border-primary/50',
        archived && 'opacity-70',
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <Link to={`/p/${project.id}`} className="font-medium after:absolute after:inset-0 hover:underline">
          {project.name}
        </Link>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" className="relative z-10" aria-label={`Actions for ${project.name}`}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem
              onSelect={async () => {
                try {
                  const copy = await api.duplicateProject(project.id)
                  toast.success(`Created “${copy.name}”`)
                  onChanged()
                } catch (e) {
                  toast.error(describeError(e).title)
                }
              }}
            >
              <Copy /> Duplicate
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={async () => {
                try {
                  await api.updateProject(project.id, { archived: !archived })
                  onChanged()
                } catch (e) {
                  toast.error(describeError(e).title)
                }
              }}
            >
              {archived ? <ArchiveRestore /> : <Archive />} {archived ? 'Restore' : 'Archive'}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => navigate(`/p/${project.id}/settings`)}>
              <Pencil /> Settings
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem destructive onSelect={() => setConfirm(true)}>
              <Trash2 /> Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {project.description && <p className="line-clamp-2 text-sm text-muted-foreground">{project.description}</p>}
      <div className="mt-auto flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {project.base_path && <code className="rounded bg-muted px-1 font-mono">{project.base_path}</code>}
        {archived && <Badge tone="muted">Archived</Badge>}
        <span>Updated {relativeTime(project.updated_at)}</span>
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Delete “${project.name}”?`}
        description="Endpoints, resources and their data, datasets, environments, secrets and request history are deleted. This cannot be undone."
        onConfirm={async () => {
          await api.deleteProject(project.id)
          onChanged()
        }}
      />
    </div>
  )
}

function NewProjectDialog({
  workspace,
  open,
  onOpenChange,
}: {
  workspace: Workspace
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [basePath, setBasePath] = useState('')
  return (
    <NameDialog
      open={open}
      onOpenChange={(o) => {
        if (o) setBasePath('')
        onOpenChange(o)
      }}
      title="New API project"
      label="Project name"
      onSubmit={async (name) => {
        const p = await api.createProject({ workspace_id: workspace.id, name, base_path: basePath.trim() || null })
        await qc.invalidateQueries({ queryKey: keys.projects(workspace.id) })
        navigate(`/p/${p.id}`)
      }}
    >
      <Field label="Base path (optional)" hint="A prefix for every route, e.g. /api or /v1.">
        {(id) => (
          <Input
            id={id}
            placeholder="/api"
            className="font-mono"
            value={basePath}
            onChange={(e) => setBasePath(e.target.value)}
          />
        )}
      </Field>
    </NameDialog>
  )
}

export function Dashboard() {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [showArchived, setShowArchived] = useState(false)
  const workspaces = useQuery({
    queryKey: [...keys.workspaces, showArchived],
    queryFn: () => api.workspaces(showArchived),
  })
  const [selectedId, setSelectedId] = useStoredString('workbench.workspace', '')
  const list = workspaces.data ?? []
  const selected = list.find((w) => w.id === selectedId) ?? list.find((w) => !w.archived_at) ?? list[0]
  const projectList = useQuery({
    queryKey: [...keys.projects(selected?.id ?? ''), 'all', showArchived],
    queryFn: () => api.projects(selected!.id, showArchived),
    enabled: !!selected,
  })
  const [dialog, setDialog] = useState<null | 'workspace' | 'rename' | 'project' | 'delete-ws' | 'import'>(null)
  const [demoBusy, setDemoBusy] = useState(false)

  useEffect(() => {
    if (selected && selected.id !== selectedId) setSelectedId(selected.id)
  }, [selected, selectedId, setSelectedId])

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: keys.workspaces })
    if (selected) void qc.invalidateQueries({ queryKey: keys.projects(selected.id) })
  }

  const createDemo = async () => {
    if (!selected) return
    setDemoBusy(true)
    try {
      const p = await createDemoProject(selected.id)
      refresh()
      toast.success('Demo project created')
      navigate(`/p/${p.id}`)
    } catch (e) {
      toast.error(describeError(e).title)
    } finally {
      setDemoBusy(false)
    }
  }

  if (workspaces.isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    )
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-6xl flex-col gap-6 p-4 sm:p-6">
        <div>
          <h1 className="text-xl font-semibold">Dashboard</h1>
          <p className="text-sm text-muted-foreground">Design REST APIs and serve them from a real local mock server.</p>
        </div>
        <ErrorNotice error={workspaces.error} />

        {list.length === 0 && !workspaces.error ? (
          <div className="rounded-lg border bg-card">
            <EmptyState
              icon={<FolderPlus />}
              title="Create your first workspace"
              action={
                <Button onClick={() => setDialog('workspace')}>
                  <Plus /> New workspace
                </Button>
              }
            >
              Workspaces group related API projects, for example one per team or product.
            </EmptyState>
          </div>
        ) : (
          <div className="grid gap-6 md:grid-cols-[14rem_1fr]">
            <section aria-labelledby="ws-title" className="flex flex-col gap-2">
              <div className="flex items-center justify-between">
                <h2 id="ws-title" className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                  Workspaces
                </h2>
                <Button variant="ghost" size="icon-sm" aria-label="New workspace" onClick={() => setDialog('workspace')}>
                  <Plus />
                </Button>
              </div>
              <ul className="flex flex-col gap-0.5">
                {list.map((w) => (
                  <li key={w.id}>
                    <button
                      type="button"
                      onClick={() => setSelectedId(w.id)}
                      aria-current={w.id === selected?.id}
                      className={cn(
                        'flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm',
                        w.id === selected?.id ? 'bg-accent font-medium' : 'hover:bg-accent/60',
                      )}
                    >
                      <span className="truncate">{w.name}</span>
                      {w.archived_at && <Badge tone="muted">archived</Badge>}
                    </button>
                  </li>
                ))}
              </ul>
              <label className="mt-1 flex items-center gap-2 px-2 text-xs text-muted-foreground">
                <Checkbox checked={showArchived} onChange={(e) => setShowArchived(e.target.checked)} /> Show archived
              </label>
            </section>

            {selected && (
              <section aria-labelledby="projects-title" className="flex min-w-0 flex-col gap-3">
                <div className="flex flex-wrap items-center gap-2">
                  <h2 id="projects-title" className="mr-auto text-base font-semibold">
                    {selected.name}
                  </h2>
                  <Button variant="outline" size="sm" onClick={() => setDialog('import')}>
                    <FileArchive /> Import archive
                  </Button>
                  <Button size="sm" onClick={() => setDialog('project')}>
                    <Plus /> New project
                  </Button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon-sm" aria-label="Workspace actions">
                        <MoreHorizontal />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={() => setDialog('rename')}>
                        <Pencil /> Rename
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={async () => {
                          await api.updateWorkspace(selected.id, { archived: !selected.archived_at })
                          refresh()
                        }}
                      >
                        {selected.archived_at ? <ArchiveRestore /> : <Archive />}
                        {selected.archived_at ? 'Restore' : 'Archive'}
                      </DropdownMenuItem>
                      <DropdownMenuSeparator />
                      <DropdownMenuItem destructive onSelect={() => setDialog('delete-ws')}>
                        <Trash2 /> Delete workspace
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                <ErrorNotice error={projectList.error} />
                {projectList.isLoading ? (
                  <Spinner />
                ) : (projectList.data ?? []).length === 0 ? (
                  <div className="rounded-lg border border-dashed">
                    <EmptyState
                      icon={<Sparkles />}
                      title="No projects yet"
                      action={
                        <div className="flex flex-wrap justify-center gap-2">
                          <Button onClick={() => setDialog('project')}>
                            <Plus /> New project
                          </Button>
                          <Button variant="outline" disabled={demoBusy} onClick={createDemo}>
                            {demoBusy ? <Spinner /> : <Sparkles />} Create demo project
                          </Button>
                        </div>
                      }
                    >
                      Start empty, import an OpenAPI document later, or explore a ready-made demo with rules, auth,
                      delays and CRUD resources.
                    </EmptyState>
                  </div>
                ) : (
                  <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
                    {projectList.data!.map((p) => (
                      <ProjectCard
                        key={p.id}
                        project={p}
                        onChanged={refresh}
                      />
                    ))}
                    <button
                      type="button"
                      onClick={createDemo}
                      disabled={demoBusy}
                      className="flex min-h-28 items-center justify-center gap-2 rounded-lg border border-dashed text-sm text-muted-foreground hover:border-primary/50 hover:text-foreground"
                    >
                      {demoBusy ? <Spinner /> : <Sparkles className="size-4" />} Add demo project
                    </button>
                  </div>
                )}
              </section>
            )}
          </div>
        )}
      </div>

      <NameDialog
        open={dialog === 'workspace'}
        onOpenChange={(o) => setDialog(o ? 'workspace' : null)}
        title="New workspace"
        onSubmit={async (name) => {
          const w = await api.createWorkspace(name)
          setSelectedId(w.id)
          refresh()
        }}
      />
      {selected && (
        <>
          <NameDialog
            open={dialog === 'rename'}
            onOpenChange={(o) => setDialog(o ? 'rename' : null)}
            title="Rename workspace"
            initial={selected.name}
            submitLabel="Rename"
            onSubmit={async (name) => {
              await api.updateWorkspace(selected.id, { name })
              refresh()
            }}
          />
          <NewProjectDialog
            workspace={selected}
            open={dialog === 'project'}
            onOpenChange={(o) => setDialog(o ? 'project' : null)}
          />
          <ConfirmDialog
            open={dialog === 'delete-ws'}
            onOpenChange={(o) => setDialog(o ? 'delete-ws' : null)}
            title={`Delete workspace “${selected.name}”?`}
            description="Every project in it is deleted with all its data. This cannot be undone."
            onConfirm={async () => {
              await api.deleteWorkspace(selected.id)
              setSelectedId('')
              refresh()
            }}
          />
          <ArchiveImportDialog
            workspaceId={selected.id}
            open={dialog === 'import'}
            onOpenChange={(o) => setDialog(o ? 'import' : null)}
          />
        </>
      )}
    </div>
  )
}
