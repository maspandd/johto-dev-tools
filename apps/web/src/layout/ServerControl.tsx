import { AlertTriangle, Copy, Play, Square } from 'lucide-react'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Select } from '@/components/ui/form'
import { Spinner } from '@/components/ui/misc'
import { Tooltip } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { keys, useAction, useEnvironments, useSelectedEnvironment, useServers } from '@/lib/queries'

/** Environment choice and start/stop for the project's mock server. */
export function ServerControl({ projectId }: { projectId: string }) {
  const servers = useServers(projectId)
  const environments = useEnvironments(projectId)
  const [envId, setEnvId] = useSelectedEnvironment(projectId)
  const running = servers.data ?? []
  const first = running[0]

  const start = useAction(
    () => api.startServer(projectId, { environment_id: envId || null }),
    {
      invalidate: [keys.servers(projectId)],
      success: (s) => `Mock server running at ${s.url}`,
    },
  )
  const stop = useAction(
    async () => {
      for (const s of running) await api.stopServer(s.id)
    },
    { invalidate: [keys.servers(projectId)], success: 'Mock server stopped' },
  )
  const busy = start.isPending || stop.isPending

  return (
    <div className="flex items-center gap-1.5">
      {(environments.data?.length ?? 0) > 0 && (
        <Select
          aria-label="Environment"
          className="hidden w-36 md:block"
          value={envId}
          onChange={(e) => setEnvId(e.target.value)}
        >
          <option value="">No environment</option>
          {environments.data!.map((env) => (
            <option key={env.id} value={env.id}>
              {env.name}
            </option>
          ))}
        </Select>
      )}
      {first ? (
        <div className="flex items-center gap-1 rounded-md border bg-background pr-1 pl-2 text-xs">
          <span className="relative flex size-2">
            <span className="absolute inline-flex size-full animate-ping rounded-full bg-success opacity-60" />
            <span className="relative inline-flex size-2 rounded-full bg-success" />
          </span>
          <span className="hidden font-mono sm:inline" title={first.url}>
            :{first.port}
            {running.length > 1 ? ` +${running.length - 1}` : ''}
          </span>
          {first.last_reload_error && (
            <Tooltip content={`Latest changes not applied: ${first.last_reload_error}`}>
              <AlertTriangle className="size-3.5 text-amber-500" aria-label="Reload failed" />
            </Tooltip>
          )}
          <Tooltip content="Copy server URL">
            <Button
              variant="ghost"
              size="icon-sm"
              className="size-6"
              aria-label="Copy server URL"
              onClick={() => {
                void navigator.clipboard.writeText(first.url).then(() => toast.success('URL copied'))
              }}
            >
              <Copy className="size-3.5!" />
            </Button>
          </Tooltip>
          <Tooltip content="Stop mock server">
            <Button
              variant="ghost"
              size="icon-sm"
              className="size-6"
              aria-label="Stop mock server"
              disabled={busy}
              onClick={() => stop.mutate(undefined)}
            >
              {stop.isPending ? <Spinner className="size-3.5" /> : <Square className="size-3.5! fill-current" />}
            </Button>
          </Tooltip>
        </div>
      ) : (
        <Button size="sm" disabled={busy || servers.isLoading} onClick={() => start.mutate(undefined)}>
          {start.isPending ? <Spinner className="text-primary-foreground" /> : <Play className="fill-current" />}
          <span className="hidden sm:inline">Start server</span>
        </Button>
      )}
    </div>
  )
}
