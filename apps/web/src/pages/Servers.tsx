import { AlertTriangle, Copy, Play, RefreshCw, Server, Square } from 'lucide-react'
import { useState } from 'react'
import { useParams } from 'react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Field, Input, Select, Switch } from '@/components/ui/form'
import { Badge, EmptyState, ErrorNotice, Spinner } from '@/components/ui/misc'
import { api } from '@/lib/api'
import { keys, useAction, useEnvironments, useSelectedEnvironment, useServers } from '@/lib/queries'
import type { MockServer } from '@/lib/types'
import { relativeTime } from '@/lib/utils'

function ServerCard({ server, projectId }: { server: MockServer; projectId: string }) {
  const environments = useEnvironments(projectId)
  const env = environments.data?.find((e) => e.id === server.environment_id)
  const stop = useAction(() => api.stopServer(server.id), { invalidate: [keys.servers(projectId)], success: 'Server stopped' })
  const reload = useAction(() => api.reloadServer(server.id), {
    invalidate: [keys.servers(projectId)],
    success: 'Reloaded from the saved configuration',
  })
  return (
    <div className="flex flex-col gap-3 rounded-lg border bg-card p-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="size-2 rounded-full bg-success" aria-hidden />
        <a href={server.url} target="_blank" rel="noreferrer" className="font-mono text-sm font-medium hover:underline">
          {server.url}
        </a>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Copy URL"
          onClick={() => void navigator.clipboard.writeText(server.url).then(() => toast.success('URL copied'))}
        >
          <Copy />
        </Button>
        <div className="ml-auto flex gap-2">
          <Button variant="outline" size="sm" disabled={reload.isPending} onClick={() => reload.mutate(undefined)}>
            <RefreshCw /> Reload
          </Button>
          <Button variant="outline" size="sm" disabled={stop.isPending} onClick={() => stop.mutate(undefined)}>
            <Square className="fill-current" /> Stop
          </Button>
        </div>
      </div>
      <dl className="grid grid-cols-2 gap-x-6 gap-y-1 text-xs sm:grid-cols-4">
        <div>
          <dt className="text-muted-foreground">Endpoints</dt>
          <dd>{server.endpoints}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">CRUD resources</dt>
          <dd>{server.resources}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Environment</dt>
          <dd>{env?.name ?? 'none'}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Started</dt>
          <dd>{relativeTime(server.started_at)}</dd>
        </div>
        <div className="col-span-2 sm:col-span-4">
          <dt className="text-muted-foreground">Browser access (CORS)</dt>
          <dd>
            {server.cors.enabled
              ? `This machine${server.cors.allowed_origins.length ? ` + ${server.cors.allowed_origins.join(', ')}` : ''}`
              : 'Off'}
          </dd>
        </div>
      </dl>
      {server.last_reload_error && (
        <div role="alert" className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 p-2 text-xs">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-500" />
          <span>
            <strong>Latest changes are not live.</strong> The server keeps its previous routes because:{' '}
            {server.last_reload_error}
          </span>
        </div>
      )}
    </div>
  )
}

export function ServersPage() {
  const { projectId } = useParams()
  const servers = useServers(projectId)
  const environments = useEnvironments(projectId)
  const [envId, setEnvId] = useSelectedEnvironment(projectId!)
  const env = environments.data?.find((e) => e.id === envId)
  const [port, setPort] = useState('')
  const [cors, setCors] = useState(true)
  const [origins, setOrigins] = useState('')
  const start = useAction(
    () =>
      api.startServer(projectId!, {
        environment_id: envId || null,
        port: port.trim() ? Number.parseInt(port, 10) : undefined,
        cors: {
          enabled: cors,
          allowed_origins: origins
            .split(/[\s,]+/)
            .map((o) => o.trim())
            .filter(Boolean),
        },
      }),
    { invalidate: [keys.servers(projectId!)], success: (s) => `Running at ${s.url}`, silentError: true },
  )

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-4xl flex-col gap-6 p-4">
        <div>
          <h1 className="text-lg font-semibold">Mock servers</h1>
          <p className="text-sm text-muted-foreground">
            Real HTTP servers on this machine. Endpoint and resource changes apply to running servers immediately; the
            request history below records every call.
          </p>
        </div>

        <section aria-labelledby="start-title" className="flex flex-col gap-3 rounded-lg border p-4">
          <h2 id="start-title" className="text-sm font-semibold">
            Start a server
          </h2>
          <div className="flex flex-wrap items-end gap-3">
            <Field label="Environment" className="w-48">
              {(id) => (
                <Select id={id} value={envId} onChange={(e) => setEnvId(e.target.value)}>
                  <option value="">None</option>
                  {(environments.data ?? []).map((e) => (
                    <option key={e.id} value={e.id}>
                      {e.name}
                    </option>
                  ))}
                </Select>
              )}
            </Field>
            <Field label="Port" hint={`Default ${env?.bind.port ?? 4010}; 0 picks a free one`} className="w-40">
              {(id) => (
                <Input
                  id={id}
                  type="number"
                  min={0}
                  max={65535}
                  placeholder={String(env?.bind.port ?? 4010)}
                  value={port}
                  onChange={(e) => setPort(e.target.value)}
                />
              )}
            </Field>
            <label className="flex items-center gap-2 pb-6 text-sm">
              <Switch checked={cors} onCheckedChange={setCors} /> Browser access
            </label>
            <Field label="Extra allowed origins" hint="Pages on localhost are always allowed" className="min-w-56 flex-1">
              {(id) => (
                <Input
                  id={id}
                  disabled={!cors}
                  placeholder="https://app.example.test"
                  value={origins}
                  onChange={(e) => setOrigins(e.target.value)}
                />
              )}
            </Field>
          </div>
          <div>
            <Button disabled={start.isPending} onClick={() => start.mutate(undefined)}>
              {start.isPending ? <Spinner className="text-current" /> : <Play className="fill-current" />} Start
            </Button>
          </div>
          <ErrorNotice error={start.error} />
        </section>

        <section aria-labelledby="running-title" className="flex flex-col gap-3">
          <h2 id="running-title" className="flex items-center gap-2 text-sm font-semibold">
            Running <Badge>{servers.data?.length ?? 0}</Badge>
          </h2>
          {servers.error && <ErrorNotice error={servers.error} />}
          {(servers.data ?? []).length === 0 ? (
            <EmptyState icon={<Server />} title="No servers running" className="rounded-lg border border-dashed" />
          ) : (
            servers.data!.map((s) => <ServerCard key={s.id} server={s} projectId={projectId!} />)
          )}
        </section>
      </div>
    </div>
  )
}
