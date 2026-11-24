import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Database, RotateCcw, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ConfirmDialog, NameDialog } from '@/components/dialogs'
import { JsonValue } from '@/components/fields'
import { InvalidFields, useInvalidFields } from '@/lib/invalidFields'
import { MasterDetail } from '@/components/MasterDetail'
import { useDraft } from '@/lib/useDraft'
import { RecordsTable } from '@/components/RecordsTable'
import { SaveBar } from '@/components/SaveBar'
import { Button } from '@/components/ui/button'
import { Checkbox, Field, Input, Label, Select, Switch } from '@/components/ui/form'
import { Badge, EmptyState, ErrorNotice, Spinner } from '@/components/ui/misc'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { FIELDS_JSON_SCHEMA } from '@/lib/fieldSchema'
import { keys, useResources, useServers } from '@/lib/queries'
import type { Locale, Resource } from '@/lib/types'

type ResourceDef = Omit<Resource, 'project_id' | 'data_initialized' | 'created_at' | 'updated_at'>

function toDef(r: Resource): ResourceDef {
  const { project_id: _p, data_initialized: _d, created_at: _c, updated_at: _u, ...def } = r
  return def
}

function Records({ resource }: { resource: Resource }) {
  const [page, setPage] = useState(1)
  const records = useQuery({
    queryKey: [...keys.records(resource.id), page],
    queryFn: () => api.resourceRecords(resource.id, page, 50),
    refetchInterval: 5_000,
  })
  if (records.error) return <ErrorNotice error={records.error} />
  if (!records.data) return <Spinner />
  const { data, meta } = records.data
  if (meta.total === 0) {
    return (
      <EmptyState title={resource.data_initialized ? 'No records' : 'Not seeded yet'}>
        {resource.data_initialized
          ? 'Create records through the mock server, or reset to generate seed data again.'
          : 'Seed records are generated the first time a mock server serves this resource.'}
      </EmptyState>
    )
  }
  return (
    <div className="flex flex-col gap-2">
      <RecordsTable rows={data} />
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <span>
          {meta.total} records · page {meta.page} of {meta.total_pages}
        </span>
        <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage((p) => p - 1)}>
          Previous
        </Button>
        <Button variant="outline" size="sm" disabled={page >= meta.total_pages} onClick={() => setPage((p) => p + 1)}>
          Next
        </Button>
      </div>
    </div>
  )
}

function ResourceEditor({ projectId, resource }: { projectId: string; resource: Resource }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const servers = useServers(projectId)
  const [invalid, report] = useInvalidFields()
  const [confirm, setConfirm] = useState<null | 'delete' | 'reset'>(null)
  const d = useDraft<ResourceDef>(toDef(resource), async (def) => {
    await api.replaceResource(resource.id, def as unknown as Record<string, unknown>)
    await qc.invalidateQueries({ queryKey: keys.resources(projectId) })
    toast.success('Resource saved')
  })
  const r = d.draft
  const item = `${r.path}/:${r.primary_key || 'id'}`

  return (
    <InvalidFields.Provider value={report}>
      <div className="flex flex-col gap-4 p-4" onKeyDown={d.onKeyDown}>
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-base font-semibold">{resource.name}</h2>
          {!resource.data_initialized && <Badge tone="muted">not seeded yet</Badge>}
          <div className="ml-auto">
            <SaveBar dirty={d.dirty} saving={d.saving} blocked={invalid.size > 0} onSave={() => void d.commit()} onReset={d.reset} />
          </div>
        </div>
        <p className="text-sm text-muted-foreground">
          Answers <code className="font-mono">GET POST {r.path}</code> and{' '}
          <code className="font-mono">GET PUT PATCH DELETE {item}</code> with stateful data. Lists support{' '}
          <code className="font-mono">?field=value</code>, <code className="font-mono">_q</code>,{' '}
          <code className="font-mono">_sort</code>, <code className="font-mono">_page</code> and{' '}
          <code className="font-mono">_limit</code>.
        </p>
        <ErrorNotice error={d.error} />

        <Tabs defaultValue="settings" className="flex flex-col">
          <TabsList className="px-0">
            <TabsTrigger value="settings">Settings</TabsTrigger>
            <TabsTrigger value="schema">Fields & seed</TabsTrigger>
            <TabsTrigger value="validation">Validation</TabsTrigger>
            <TabsTrigger value="records">Records</TabsTrigger>
          </TabsList>

          <TabsContent value="settings" className="pt-4">
            <div className="grid max-w-3xl gap-4 sm:grid-cols-2">
              <Field label="Name" hint="Identifier used by references, e.g. users">
                {(id) => <Input id={id} className="font-mono" value={r.name} onChange={(e) => d.set({ name: e.target.value })} />}
              </Field>
              <Field label="Collection path">
                {(id) => <Input id={id} className="font-mono" value={r.path} onChange={(e) => d.set({ path: e.target.value })} />}
              </Field>
              <Field label="Primary key field">
                {(id) => (
                  <Input id={id} className="font-mono" value={r.primary_key} onChange={(e) => d.set({ primary_key: e.target.value })} />
                )}
              </Field>
              <Field label="New keys">
                {(id) => (
                  <Select
                    id={id}
                    value={r.key_strategy}
                    onChange={(e) => d.set({ key_strategy: e.target.value as ResourceDef['key_strategy'] })}
                  >
                    <option value="uuid">Random UUID</option>
                    <option value="increment">1, 2, 3, …</option>
                  </Select>
                )}
              </Field>
              <Field label="Delete">
                {(id) => (
                  <Select id={id} value={r.delete} onChange={(e) => d.set({ delete: e.target.value as ResourceDef['delete'] })}>
                    <option value="hard">Remove the record</option>
                    <option value="soft">Soft delete (sets deleted_at)</option>
                  </Select>
                )}
              </Field>
              <Field label="Searchable fields" hint="Comma-separated, matched by _q">
                {(id) => (
                  <Input
                    id={id}
                    className="font-mono"
                    value={r.searchable.join(', ')}
                    onChange={(e) =>
                      d.set({
                        searchable: e.target.value
                          .split(',')
                          .map((s) => s.trim())
                          .filter(Boolean),
                      })
                    }
                  />
                )}
              </Field>
              <Field label="Default page size">
                {(id) => (
                  <Input
                    id={id}
                    type="number"
                    min={1}
                    value={r.pagination.default_limit}
                    onChange={(e) =>
                      d.set({ pagination: { ...r.pagination, default_limit: Number.parseInt(e.target.value, 10) || 1 } })
                    }
                  />
                )}
              </Field>
              <Field label="Largest page size">
                {(id) => (
                  <Input
                    id={id}
                    type="number"
                    min={1}
                    max={1000}
                    value={r.pagination.max_limit}
                    onChange={(e) =>
                      d.set({ pagination: { ...r.pagination, max_limit: Number.parseInt(e.target.value, 10) || 1 } })
                    }
                  />
                )}
              </Field>
              <label className="flex items-center gap-2 text-sm">
                <Switch checked={r.enabled} onCheckedChange={(enabled) => d.set({ enabled })} /> Enabled
              </label>
            </div>
            <div className="mt-6 flex flex-wrap gap-2 border-t pt-4">
              <Button variant="outline" onClick={() => setConfirm('reset')}>
                <RotateCcw /> Reset data
              </Button>
              <Button variant="outline" className="text-destructive" onClick={() => setConfirm('delete')}>
                <Trash2 /> Delete resource
              </Button>
            </div>
          </TabsContent>

          <TabsContent value="schema" className="flex flex-col gap-4 pt-4">
            <div className="flex flex-col gap-1.5">
              <Label>Fields (generated values for seed records)</Label>
              <JsonValue
                label="Fields"
                height={320}
                schema={FIELDS_JSON_SCHEMA}
                value={r.schema.fields}
                onSave={() => void d.commit()}
                onChange={(fields) => d.set({ schema: { fields: (fields as ResourceDef['schema']['fields']) ?? [] } })}
              />
            </div>
            <div className="flex flex-wrap items-end gap-3">
              <label className="flex items-center gap-2 pb-1.5 text-sm">
                <Checkbox
                  checked={!!r.seed}
                  onChange={(e) => d.set({ seed: e.target.checked ? { count: 20, seed: 1, locale: 'en' } : undefined })}
                />
                Seed records
              </label>
              {r.seed && (
                <>
                  <Field label="Count" className="w-24">
                    {(id) => (
                      <Input
                        id={id}
                        type="number"
                        min={0}
                        max={10000}
                        value={r.seed!.count}
                        onChange={(e) => d.set({ seed: { ...r.seed!, count: Number.parseInt(e.target.value, 10) || 0 } })}
                      />
                    )}
                  </Field>
                  <Field label="Random seed" className="w-28">
                    {(id) => (
                      <Input
                        id={id}
                        type="number"
                        min={0}
                        value={r.seed!.seed}
                        onChange={(e) => d.set({ seed: { ...r.seed!, seed: Number.parseInt(e.target.value, 10) || 0 } })}
                      />
                    )}
                  </Field>
                  <Field label="Locale" className="w-36">
                    {(id) => (
                      <Select
                        id={id}
                        value={r.seed!.locale}
                        onChange={(e) => d.set({ seed: { ...r.seed!, locale: e.target.value as Locale } })}
                      >
                        <option value="en">English</option>
                        <option value="id">Indonesian</option>
                      </Select>
                    )}
                  </Field>
                </>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              Seed data is generated once, the first time a server serves the resource. Use “Reset data” to generate it
              again after changing fields.
            </p>
          </TabsContent>

          <TabsContent value="validation" className="flex flex-col gap-1.5 pt-4">
            <Label>Body schema (JSON Schema checked on every create, replace and patch; empty accepts anything)</Label>
            <JsonValue
              label="Body schema"
              allowEmpty
              height={320}
              value={r.body_schema}
              onSave={() => void d.commit()}
              onChange={(body_schema) => d.set({ body_schema })}
            />
          </TabsContent>

          <TabsContent value="records" className="pt-4">
            {(servers.data?.length ?? 0) === 0 && (
              <p className="mb-2 text-xs text-muted-foreground">Showing stored records; start a server to see live data.</p>
            )}
            <Records resource={resource} />
          </TabsContent>
        </Tabs>

        <ConfirmDialog
          open={confirm === 'reset'}
          onOpenChange={(o) => setConfirm(o ? 'reset' : null)}
          title="Reset data?"
          description="Every record is replaced with fresh seed data (or removed if the resource has no seed). This cannot be undone."
          confirmLabel="Reset"
          onConfirm={async () => {
            await api.resetResource(resource.id)
            await qc.invalidateQueries({ queryKey: keys.records(resource.id) })
            await qc.invalidateQueries({ queryKey: keys.resources(projectId) })
            toast.success('Data reset')
          }}
        />
        <ConfirmDialog
          open={confirm === 'delete'}
          onOpenChange={(o) => setConfirm(o ? 'delete' : null)}
          title={`Delete “${resource.name}”?`}
          description="The resource and all its records are deleted. This cannot be undone."
          onConfirm={async () => {
            await api.deleteResource(resource.id)
            await qc.invalidateQueries({ queryKey: keys.resources(projectId) })
            navigate(`/p/${projectId}/resources`)
          }}
        />
      </div>
    </InvalidFields.Provider>
  )
}

export function ResourcesPage() {
  const { projectId, resourceId } = useParams()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const resources = useResources(projectId)
  const [creating, setCreating] = useState(false)
  const list = resources.data ?? []
  const active = list.find((r) => r.id === resourceId) ?? (resourceId ? undefined : list[0])

  return (
    <>
      <MasterDetail
        title="CRUD resources"
        newLabel="New resource"
        onNew={() => setCreating(true)}
        basePath={`/p/${projectId}/resources`}
        activeId={active?.id}
        items={list.map((r) => ({
          id: r.id,
          title: r.name,
          subtitle: <span className="font-mono">{r.path}</span>,
          dim: !r.enabled,
        }))}
      >
        {resources.isLoading ? (
          <Spinner className="m-4" />
        ) : resources.error ? (
          <ErrorNotice error={resources.error} className="m-4" />
        ) : active ? (
          <ResourceEditor key={active.id} projectId={projectId!} resource={active} />
        ) : (
          <EmptyState
            icon={<Database />}
            title={list.length ? 'Resource not found' : 'No CRUD resources yet'}
            className="h-full"
            action={<Button onClick={() => setCreating(true)}>New resource</Button>}
          >
            A resource such as <code>users</code> at <code>/users</code> gives you list, get, create, replace, patch and
            delete routes backed by stored, generated data.
          </EmptyState>
        )}
      </MasterDetail>
      <NameDialog
        open={creating}
        onOpenChange={setCreating}
        title="New CRUD resource"
        label="Name (letters, digits and _)"
        onSubmit={async (name) => {
          const created = await api.createResource(projectId!, {
            name,
            path: `/${name}`,
            schema: {
              fields: [
                { name: 'name', type: 'full_name' },
                { name: 'email', type: 'email', unique: true },
                { name: 'created_at', type: 'timestamp', from: '2025-01-01T00:00:00Z', to: '2026-06-30T00:00:00Z' },
              ],
            },
            seed: { count: 20, seed: 1 },
          })
          await qc.invalidateQueries({ queryKey: keys.resources(projectId!) })
          navigate(`/p/${projectId}/resources/${created.id}`)
        }}
      />
    </>
  )
}
