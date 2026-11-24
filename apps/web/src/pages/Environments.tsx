import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Globe, KeyRound, Lock, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { ConfirmDialog, NameDialog } from '@/components/dialogs'
import { KeyValueEditor } from '@/components/fields'
import { MasterDetail } from '@/components/MasterDetail'
import { useDraft } from '@/lib/useDraft'
import { SaveBar } from '@/components/SaveBar'
import { Button } from '@/components/ui/button'
import { Field, Input, Label } from '@/components/ui/form'
import { EmptyState, ErrorNotice, Spinner } from '@/components/ui/misc'
import { api } from '@/lib/api'
import { keys, useAction, useEnvironments, useSelectedEnvironment } from '@/lib/queries'
import type { Environment } from '@/lib/types'
import { relativeTime } from '@/lib/utils'

type EnvDef = Pick<Environment, 'name' | 'base_url' | 'default_headers' | 'variables' | 'bind'>

const toDef = (e: Environment): EnvDef => ({
  name: e.name,
  base_url: e.base_url,
  default_headers: e.default_headers,
  variables: e.variables,
  bind: e.bind,
})

/** Merge-patch for a string map: removed keys become null. */
function mapPatch(before: Record<string, string>, after: Record<string, string>) {
  const patch: Record<string, string | null> = { ...after }
  for (const k of Object.keys(before)) if (!(k in after)) patch[k] = null
  return patch
}

function Secrets({ environment }: { environment: Environment }) {
  const secrets = useQuery({ queryKey: keys.secrets(environment.id), queryFn: () => api.secrets(environment.id) })
  const [name, setName] = useState('')
  const [value, setValue] = useState('')
  const put = useAction(() => api.putSecret(environment.id, name.trim(), value), {
    invalidate: [keys.secrets(environment.id)],
    success: 'Secret saved',
  })
  const remove = useAction((n: string) => api.deleteSecret(environment.id, n), {
    invalidate: [keys.secrets(environment.id)],
    success: 'Secret deleted',
  })
  return (
    <section aria-labelledby="secrets-title" className="flex flex-col gap-3 rounded-lg border p-4">
      <h3 id="secrets-title" className="flex items-center gap-2 text-sm font-semibold">
        <Lock className="size-4" /> Secrets
      </h3>
      <p className="text-xs text-muted-foreground">
        Encrypted at rest and write-only: values are never shown again and never exported. Not available as response
        placeholders.
      </p>
      {secrets.error && <ErrorNotice error={secrets.error} />}
      {(secrets.data ?? []).length > 0 && (
        <ul className="divide-y rounded-md border">
          {secrets.data!.map((s) => (
            <li key={s.name} className="flex items-center gap-2 px-3 py-1.5 text-sm">
              <KeyRound className="size-3.5 text-muted-foreground" />
              <code className="font-mono">{s.name}</code>
              <span className="text-xs text-muted-foreground">set {relativeTime(s.updated_at)}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                className="ml-auto"
                aria-label={`Delete secret ${s.name}`}
                onClick={() => remove.mutate(s.name)}
              >
                <Trash2 />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          put.mutate(undefined, {
            onSuccess: () => {
              setName('')
              setValue('')
            },
          })
        }}
      >
        <Field label="Name" className="w-48">
          {(id) => (
            <Input id={id} className="font-mono" placeholder="API_TOKEN" value={name} onChange={(e) => setName(e.target.value)} />
          )}
        </Field>
        <Field label="Value" className="min-w-48 flex-1">
          {(id) => (
            <Input
              id={id}
              type="password"
              autoComplete="off"
              value={value}
              onChange={(e) => setValue(e.target.value)}
            />
          )}
        </Field>
        <Button type="submit" disabled={!name.trim() || !value || put.isPending}>
          Set secret
        </Button>
      </form>
    </section>
  )
}

function EnvironmentEditor({ projectId, environment }: { projectId: string; environment: Environment }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [selected, setSelected] = useSelectedEnvironment(projectId)
  const [confirm, setConfirm] = useState(false)
  const d = useDraft<EnvDef>(toDef(environment), async (def) => {
    await api.updateEnvironment(environment.id, {
      name: def.name,
      base_url: def.base_url || null,
      default_headers: mapPatch(environment.default_headers, def.default_headers),
      variables: mapPatch(environment.variables, def.variables),
      bind: def.bind,
    })
    await qc.invalidateQueries({ queryKey: keys.environments(projectId) })
    toast.success('Environment saved; restart running servers to use it.')
  })
  const v = d.draft

  return (
    <div className="flex max-w-3xl flex-col gap-5 p-4" onKeyDown={d.onKeyDown}>
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-base font-semibold">{environment.name}</h2>
        {selected === environment.id ? (
          <span className="text-xs text-success">Used when starting servers</span>
        ) : (
          <Button variant="link" size="sm" onClick={() => setSelected(environment.id)}>
            Use for servers
          </Button>
        )}
        <div className="ml-auto">
          <SaveBar dirty={d.dirty} saving={d.saving} onSave={() => void d.commit()} onReset={d.reset} />
        </div>
      </div>
      <ErrorNotice error={d.error} />
      <div className="grid gap-4 sm:grid-cols-3">
        <Field label="Name">{(id) => <Input id={id} value={v.name} onChange={(e) => d.set({ name: e.target.value })} />}</Field>
        <Field label="Base URL" hint="For external clients" className="sm:col-span-2">
          {(id) => (
            <Input
              id={id}
              placeholder="http://localhost:4010"
              value={v.base_url ?? ''}
              onChange={(e) => d.set({ base_url: e.target.value || null })}
            />
          )}
        </Field>
        <Field label="Mock server port" hint="0 picks a free port">
          {(id) => (
            <Input
              id={id}
              type="number"
              min={0}
              max={65535}
              value={v.bind.port}
              onChange={(e) => d.set({ bind: { ...v.bind, port: Number.parseInt(e.target.value, 10) || 0 } })}
            />
          )}
        </Field>
        <Field label="Listen on" hint="Loopback only; network exposure is an explicit opt-in.">
          {(id) => <Input id={id} disabled value={v.bind.host} />}
        </Field>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label>Variables</Label>
        <p className="text-xs text-muted-foreground">
          Available in responses as <code className="font-mono">{'{{ env.NAME }}'}</code>.
        </p>
        <KeyValueEditor value={v.variables} onChange={(variables) => d.set({ variables })} keyPlaceholder="NAME" addLabel="Add variable" />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label>Default response headers</Label>
        <KeyValueEditor
          value={v.default_headers}
          onChange={(default_headers) => d.set({ default_headers })}
          keyPlaceholder="Header"
          addLabel="Add header"
        />
      </div>
      <Secrets environment={environment} />
      <div>
        <Button variant="ghost" className="text-destructive" onClick={() => setConfirm(true)}>
          <Trash2 /> Delete environment
        </Button>
      </div>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title={`Delete “${environment.name}”?`}
        description="Its variables and secrets are deleted. This cannot be undone."
        onConfirm={async () => {
          await api.deleteEnvironment(environment.id)
          if (selected === environment.id) setSelected('')
          await qc.invalidateQueries({ queryKey: keys.environments(projectId) })
          navigate(`/p/${projectId}/environments`)
        }}
      />
    </div>
  )
}

export function EnvironmentsPage() {
  const { projectId, environmentId } = useParams()
  const qc = useQueryClient()
  const navigate = useNavigate()
  const environments = useEnvironments(projectId)
  const [creating, setCreating] = useState(false)
  const list = environments.data ?? []
  const active = list.find((e) => e.id === environmentId) ?? (environmentId ? undefined : list[0])

  return (
    <>
      <MasterDetail
        title="Environments"
        newLabel="New environment"
        onNew={() => setCreating(true)}
        basePath={`/p/${projectId}/environments`}
        activeId={active?.id}
        items={list.map((e) => ({ id: e.id, title: e.name, subtitle: `port ${e.bind.port}` }))}
      >
        {environments.isLoading ? (
          <Spinner className="m-4" />
        ) : environments.error ? (
          <ErrorNotice error={environments.error} className="m-4" />
        ) : active ? (
          <EnvironmentEditor key={active.id} projectId={projectId!} environment={active} />
        ) : (
          <EmptyState
            icon={<Globe />}
            title={list.length ? 'Environment not found' : 'No environments yet'}
            className="h-full"
            action={<Button onClick={() => setCreating(true)}>New environment</Button>}
          >
            Environments such as Development or Testing hold variables, default headers, secrets and the mock server
            port.
          </EmptyState>
        )}
      </MasterDetail>
      <NameDialog
        open={creating}
        onOpenChange={setCreating}
        title="New environment"
        initial="Development"
        onSubmit={async (name) => {
          const created = await api.createEnvironment(projectId!, { name })
          await qc.invalidateQueries({ queryKey: keys.environments(projectId!) })
          navigate(`/p/${projectId}/environments/${created.id}`)
        }}
      />
    </>
  )
}
