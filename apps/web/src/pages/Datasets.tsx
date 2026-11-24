import { useQueryClient } from '@tanstack/react-query'
import { Download, Eye, Table2, Trash2 } from 'lucide-react'
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
import { Field, Input, Label, Select, Textarea } from '@/components/ui/form'
import { EmptyState, ErrorNotice, Spinner } from '@/components/ui/misc'
import { api, describeError, saveFile } from '@/lib/api'
import { FIELDS_JSON_SCHEMA } from '@/lib/fieldSchema'
import { keys, useAction, useDatasets } from '@/lib/queries'
import type { Dataset, Locale } from '@/lib/types'

type DatasetDef = Pick<Dataset, 'name' | 'description' | 'schema' | 'count' | 'seed' | 'locale'>

const toDef = (d: Dataset): DatasetDef => ({
  name: d.name,
  description: d.description,
  schema: d.schema,
  count: d.count,
  seed: d.seed,
  locale: d.locale,
})

function DatasetEditor({ projectId, dataset }: { projectId: string; dataset: Dataset }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [invalid, report] = useInvalidFields()
  const [confirm, setConfirm] = useState(false)
  const d = useDraft<DatasetDef>(toDef(dataset), async (def) => {
    await api.replaceDataset(dataset.id, def as unknown as Record<string, unknown>)
    await qc.invalidateQueries({ queryKey: keys.datasets(projectId) })
    toast.success('Dataset saved')
  })
  const preview = useAction(
    (def: DatasetDef) =>
      api.previewSchema(projectId, { schema: def.schema, count: Math.min(def.count, 20), seed: def.seed, locale: def.locale }),
    { silentError: true },
  )
  const exportAs = async (format: 'json' | 'csv') => {
    try {
      saveFile(await api.exportDataset(dataset.id, format))
    } catch (e) {
      toast.error(describeError(e).title)
    }
  }
  const v = d.draft

  return (
    <InvalidFields.Provider value={report}>
      <div className="flex flex-col gap-4 p-4" onKeyDown={d.onKeyDown}>
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-base font-semibold">{dataset.name}</h2>
          <div className="ml-auto">
            <SaveBar dirty={d.dirty} saving={d.saving} blocked={invalid.size > 0} onSave={() => void d.commit()} onReset={d.reset} />
          </div>
        </div>
        <ErrorNotice error={d.error} />
        <div className="grid max-w-3xl gap-4 sm:grid-cols-4">
          <Field label="Name" className="sm:col-span-2">
            {(id) => <Input id={id} value={v.name} onChange={(e) => d.set({ name: e.target.value })} />}
          </Field>
          <Field label="Records">
            {(id) => (
              <Input
                id={id}
                type="number"
                min={1}
                max={10000}
                value={v.count}
                onChange={(e) => d.set({ count: Number.parseInt(e.target.value, 10) || 1 })}
              />
            )}
          </Field>
          <Field label="Random seed" hint="Same seed, same data">
            {(id) => (
              <Input
                id={id}
                type="number"
                min={0}
                value={v.seed}
                onChange={(e) => d.set({ seed: Number.parseInt(e.target.value, 10) || 0 })}
              />
            )}
          </Field>
          <Field label="Locale">
            {(id) => (
              <Select id={id} value={v.locale} onChange={(e) => d.set({ locale: e.target.value as Locale })}>
                <option value="en">English</option>
                <option value="id">Indonesian</option>
              </Select>
            )}
          </Field>
          <Field label="Description" className="sm:col-span-3">
            {(id) => (
              <Textarea
                id={id}
                className="min-h-9 font-sans text-sm"
                value={v.description}
                onChange={(e) => d.set({ description: e.target.value })}
              />
            )}
          </Field>
        </div>
        <div className="flex flex-col gap-1.5">
          <Label>Fields</Label>
          <JsonValue
            label="Fields"
            height={280}
            schema={FIELDS_JSON_SCHEMA}
            value={v.schema.fields}
            onSave={() => void d.commit()}
            onChange={(fields) => d.set({ schema: { fields: (fields as DatasetDef['schema']['fields']) ?? [] } })}
          />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" disabled={invalid.size > 0 || preview.isPending} onClick={() => preview.mutate(v)}>
            {preview.isPending ? <Spinner /> : <Eye />} Preview{d.dirty ? ' unsaved changes' : ''}
          </Button>
          <Button variant="outline" disabled={d.dirty} onClick={() => void exportAs('json')}>
            <Download /> Export JSON
          </Button>
          <Button variant="outline" disabled={d.dirty} onClick={() => void exportAs('csv')}>
            <Download /> Export CSV
          </Button>
          <Button variant="ghost" className="ml-auto text-destructive" onClick={() => setConfirm(true)}>
            <Trash2 /> Delete
          </Button>
        </div>
        {d.dirty && <p className="text-xs text-muted-foreground">Save to export with the current settings.</p>}
        <ErrorNotice error={preview.error} />
        {preview.data && (
          <div className="flex flex-col gap-1">
            <p className="text-xs text-muted-foreground">
              First {preview.data.length} of {v.count} records
            </p>
            <RecordsTable rows={preview.data} />
          </div>
        )}
        <ConfirmDialog
          open={confirm}
          onOpenChange={setConfirm}
          title={`Delete “${dataset.name}”?`}
          description="Only the dataset definition is stored, so nothing else is affected. This cannot be undone."
          onConfirm={async () => {
            await api.deleteDataset(dataset.id)
            await qc.invalidateQueries({ queryKey: keys.datasets(projectId) })
            navigate(`/p/${projectId}/datasets`)
          }}
        />
      </div>
    </InvalidFields.Provider>
  )
}

export function DatasetsPage() {
  const { projectId, datasetId } = useParams()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const datasets = useDatasets(projectId)
  const [creating, setCreating] = useState(false)
  const list = datasets.data ?? []
  const active = list.find((d) => d.id === datasetId) ?? (datasetId ? undefined : list[0])

  return (
    <>
      <MasterDetail
        title="Datasets"
        newLabel="New dataset"
        onNew={() => setCreating(true)}
        basePath={`/p/${projectId}/datasets`}
        activeId={active?.id}
        items={list.map((d) => ({ id: d.id, title: d.name, subtitle: `${d.count} records · ${d.locale}` }))}
      >
        {datasets.isLoading ? (
          <Spinner className="m-4" />
        ) : datasets.error ? (
          <ErrorNotice error={datasets.error} className="m-4" />
        ) : active ? (
          <DatasetEditor key={active.id} projectId={projectId!} dataset={active} />
        ) : (
          <EmptyState
            icon={<Table2 />}
            title={list.length ? 'Dataset not found' : 'No datasets yet'}
            className="h-full"
            action={<Button onClick={() => setCreating(true)}>New dataset</Button>}
          >
            Generate realistic, synthetic records (names, emails, addresses, dates, …) and export them as JSON or CSV.
          </EmptyState>
        )}
      </MasterDetail>
      <NameDialog
        open={creating}
        onOpenChange={setCreating}
        title="New dataset"
        onSubmit={async (name) => {
          const created = await api.createDataset(projectId!, {
            name,
            count: 50,
            seed: 1,
            schema: {
              fields: [
                { name: 'id', type: 'uuid' },
                { name: 'name', type: 'full_name' },
                { name: 'email', type: 'email', unique: true },
                { name: 'phone', type: 'phone' },
                { name: 'city', type: 'city' },
              ],
            },
          })
          await qc.invalidateQueries({ queryKey: keys.datasets(projectId!) })
          navigate(`/p/${projectId}/datasets/${created.id}`)
        }}
      />
    </>
  )
}
