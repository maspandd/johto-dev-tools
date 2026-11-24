import { useQueryClient } from '@tanstack/react-query'
import { Archive, ArchiveRestore, Copy, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ConfirmDialog, NameDialog } from '@/components/dialogs'
import { KeyValueEditor } from '@/components/fields'
import { useDraft } from '@/lib/useDraft'
import { SaveBar } from '@/components/SaveBar'
import { Button } from '@/components/ui/button'
import { Field, Input, Label, Textarea } from '@/components/ui/form'
import { Badge, ErrorNotice, Spinner } from '@/components/ui/misc'
import { api, describeError } from '@/lib/api'
import { keys, useProject } from '@/lib/queries'
import type { Project } from '@/lib/types'

type ProjectDef = Pick<Project, 'name' | 'description' | 'base_path' | 'default_headers'>

function Editor({ project }: { project: Project }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [dialog, setDialog] = useState<null | 'duplicate' | 'delete'>(null)
  const d = useDraft<ProjectDef>(
    { name: project.name, description: project.description, base_path: project.base_path, default_headers: project.default_headers },
    async (def) => {
      const headers: Record<string, string | null> = { ...def.default_headers }
      for (const k of Object.keys(project.default_headers)) if (!(k in def.default_headers)) headers[k] = null
      await api.updateProject(project.id, {
        name: def.name,
        description: def.description,
        base_path: def.base_path?.trim() || null,
        default_headers: headers,
      })
      await qc.invalidateQueries({ queryKey: keys.project(project.id) })
      await qc.invalidateQueries({ queryKey: keys.projects(project.workspace_id) })
      toast.success('Project saved; restart running servers to apply the base path and headers.')
    },
  )
  const archived = !!project.archived_at
  const v = d.draft

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-5 p-4" onKeyDown={d.onKeyDown}>
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="text-lg font-semibold">Project settings</h1>
        {archived && <Badge tone="muted">Archived</Badge>}
        <div className="ml-auto">
          <SaveBar dirty={d.dirty} saving={d.saving} onSave={() => void d.commit()} onReset={d.reset} />
        </div>
      </div>
      <ErrorNotice error={d.error} />
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Name">{(id) => <Input id={id} value={v.name} onChange={(e) => d.set({ name: e.target.value })} />}</Field>
        <Field label="Base path" hint="Prefix for every route, e.g. /api">
          {(id) => (
            <Input
              id={id}
              className="font-mono"
              placeholder="/api"
              value={v.base_path ?? ''}
              onChange={(e) => d.set({ base_path: e.target.value || null })}
            />
          )}
        </Field>
        <Field label="Description" className="sm:col-span-2">
          {(id) => (
            <Textarea
              id={id}
              className="font-sans text-sm"
              value={v.description}
              onChange={(e) => d.set({ description: e.target.value })}
            />
          )}
        </Field>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label>Headers on every mock response</Label>
        <KeyValueEditor
          value={v.default_headers}
          onChange={(default_headers) => d.set({ default_headers })}
          keyPlaceholder="Header"
          addLabel="Add header"
        />
      </div>
      <div className="flex flex-wrap gap-2 border-t pt-4">
        <Button variant="outline" onClick={() => setDialog('duplicate')}>
          <Copy /> Duplicate
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            try {
              await api.updateProject(project.id, { archived: !archived })
              await qc.invalidateQueries({ queryKey: keys.project(project.id) })
              toast.success(archived ? 'Project restored' : 'Project archived; its servers were stopped.')
            } catch (e) {
              toast.error(describeError(e).title)
            }
          }}
        >
          {archived ? <ArchiveRestore /> : <Archive />} {archived ? 'Restore' : 'Archive'}
        </Button>
        <Button variant="outline" className="text-destructive" onClick={() => setDialog('delete')}>
          <Trash2 /> Delete project
        </Button>
      </div>
      <NameDialog
        open={dialog === 'duplicate'}
        onOpenChange={(o) => setDialog(o ? 'duplicate' : null)}
        title="Duplicate project"
        initial={`${project.name} (copy)`}
        submitLabel="Duplicate"
        onSubmit={async (name) => {
          const copy = await api.duplicateProject(project.id, name)
          await qc.invalidateQueries({ queryKey: keys.projects(project.workspace_id) })
          navigate(`/p/${copy.id}`)
        }}
      >
        <p className="text-xs text-muted-foreground">
          Endpoints, resources with their data, datasets and environments are copied. Secrets and request history are
          not.
        </p>
      </NameDialog>
      <ConfirmDialog
        open={dialog === 'delete'}
        onOpenChange={(o) => setDialog(o ? 'delete' : null)}
        title={`Delete “${project.name}”?`}
        description="Everything in the project is deleted, including data, secrets and request history. This cannot be undone."
        onConfirm={async () => {
          await api.deleteProject(project.id)
          await qc.invalidateQueries({ queryKey: keys.projects(project.workspace_id) })
          navigate('/')
        }}
      />
    </div>
  )
}

export function ProjectSettingsPage() {
  const { projectId } = useParams()
  const project = useProject(projectId)
  if (!project.data) return <Spinner className="m-4" />
  return (
    <div className="h-full overflow-y-auto">
      <Editor key={project.data.id} project={project.data} />
    </div>
  )
}
