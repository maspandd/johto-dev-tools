import { useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronUp, History, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'

import { Button } from '@/components/ui/button'
import { Input, Select } from '@/components/ui/form'
import { Badge, EmptyState, ErrorNotice, MethodBadge, StatusBadge } from '@/components/ui/misc'
import { Tooltip } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { keys, useAction, useEndpoints, useResources, useServers } from '@/lib/queries'
import { type CapturedBody, type LogFilter, METHODS, type RequestLog } from '@/lib/types'
import { cn, formatBytes, formatTime } from '@/lib/utils'

function pretty(body: CapturedBody | null): string {
  if (!body) return ''
  try {
    return JSON.stringify(JSON.parse(body.content), null, 2)
  } catch {
    return body.content
  }
}

function Pairs({ title, pairs }: { title: string; pairs: [string, string][] }) {
  if (!pairs.length) return null
  return (
    <div>
      <h4 className="mb-1 text-[11px] font-semibold text-muted-foreground uppercase">{title}</h4>
      <dl className="grid grid-cols-[minmax(6rem,auto)_1fr] gap-x-3 gap-y-0.5 font-mono text-xs">
        {pairs.map(([k, v], i) => (
          <div key={i} className="contents">
            <dt className="text-muted-foreground">{k}</dt>
            <dd className="break-all">{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

function Body({ title, body }: { title: string; body: CapturedBody | null }) {
  if (!body || !body.size) return null
  return (
    <div>
      <h4 className="mb-1 flex items-center gap-2 text-[11px] font-semibold text-muted-foreground uppercase">
        {title} <span className="font-normal normal-case">{formatBytes(body.size)}</span>
        {body.truncated && <Badge tone="warning">truncated</Badge>}
      </h4>
      <pre className="max-h-64 overflow-auto rounded border bg-muted/40 p-2 font-mono text-xs whitespace-pre-wrap">
        {pretty(body)}
      </pre>
    </div>
  )
}

function Detail({ log, projectId }: { log: RequestLog; projectId: string }) {
  const endpoints = useEndpoints(projectId)
  const resources = useResources(projectId)
  const endpoint = endpoints.data?.find((e) => e.id === log.matched_endpoint)
  const resource = resources.data?.find((r) => r.id === log.matched_resource)
  const query = log.query.length ? `?${log.query.map(([k, v]) => `${k}=${v}`).join('&')}` : ''
  return (
    <div className="flex flex-col gap-3 p-3">
      <div className="flex flex-wrap items-center gap-2">
        <MethodBadge method={log.method} className="w-auto" />
        <code className="font-mono text-xs break-all">
          {log.path}
          {query}
        </code>
        <StatusBadge status={log.status} />
        <span className="text-xs text-muted-foreground">{log.duration_ms} ms</span>
      </div>
      <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
        {endpoint ? (
          <span>
            Endpoint{' '}
            <Link className="text-primary underline" to={`/p/${projectId}/endpoints/${endpoint.id}`}>
              {endpoint.name}
            </Link>
          </span>
        ) : resource ? (
          <span>
            Resource{' '}
            <Link className="text-primary underline" to={`/p/${projectId}/resources/${resource.id}`}>
              {resource.name}
            </Link>
          </span>
        ) : (
          <span>No endpoint matched</span>
        )}
        {log.matched_rule && <span>· rule “{log.matched_rule}”</span>}
        <span>· request {log.id}</span>
      </div>
      <Pairs title="Request headers" pairs={log.request_headers} />
      <Body title="Request body" body={log.request_body} />
      <Pairs title="Response headers" pairs={log.response_headers} />
      <Body title="Response body" body={log.response_body} />
    </div>
  )
}

/**
 * Live request history for a project. `compact` is the bottom panel;
 * otherwise it fills the page.
 */
export function RequestHistory({
  projectId,
  compact = false,
  collapsed = false,
  onToggle,
}: {
  projectId: string
  compact?: boolean
  collapsed?: boolean
  onToggle?: () => void
}) {
  const servers = useServers(projectId)
  const endpoints = useEndpoints(projectId)
  const live = (servers.data?.length ?? 0) > 0
  const [filter, setFilter] = useState<LogFilter>({ limit: 200 })
  const [selected, setSelected] = useState<string | null>(null)
  const logs = useQuery({
    queryKey: [...keys.logs(projectId), filter],
    queryFn: () => api.logs(projectId, filter),
    refetchInterval: live && !collapsed ? 2_000 : false,
  })
  const clear = useAction(() => api.clearLogs(projectId), {
    invalidate: [keys.logs(projectId)],
    success: (r) => `Cleared ${r.deleted} requests`,
  })
  const rows = logs.data ?? []
  const current = rows.find((r) => r.id === selected) ?? null

  return (
    <section aria-label="Request history" className="flex h-full flex-col bg-card">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-2">
        {onToggle && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="size-6"
            aria-label={collapsed ? 'Expand request history' : 'Collapse request history'}
            onClick={onToggle}
          >
            {collapsed ? <ChevronUp /> : <ChevronDown />}
          </Button>
        )}
        <h2 className="flex items-center gap-1.5 text-xs font-semibold">
          <History className="size-3.5" /> Request history
        </h2>
        <Badge tone={live ? 'success' : 'muted'}>{live ? 'live' : 'server stopped'}</Badge>
        <span className="text-xs text-muted-foreground">{rows.length} shown</span>
        {!collapsed && (
          <div className="ml-auto flex items-center gap-1.5">
            <Select
              aria-label="Filter by method"
              className="w-24"
              value={filter.method ?? ''}
              onChange={(e) => setFilter((f) => ({ ...f, method: e.target.value || undefined }))}
            >
              <option value="">Any method</option>
              {METHODS.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </Select>
            <Input
              aria-label="Filter by status"
              placeholder="Status"
              inputMode="numeric"
              className="h-8 w-20"
              value={filter.status ?? ''}
              onChange={(e) => {
                const n = Number.parseInt(e.target.value, 10)
                setFilter((f) => ({ ...f, status: Number.isFinite(n) ? n : undefined }))
              }}
            />
            {!compact && (
              <Select
                aria-label="Filter by endpoint"
                className="w-48"
                value={filter.endpoint_id ?? ''}
                onChange={(e) => setFilter((f) => ({ ...f, endpoint_id: e.target.value || undefined }))}
              >
                <option value="">Any endpoint</option>
                {(endpoints.data ?? []).map((e) => (
                  <option key={e.id} value={e.id}>
                    {e.method} {e.path}
                  </option>
                ))}
              </Select>
            )}
            <Tooltip content="Clear history">
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Clear history"
                disabled={!rows.length || clear.isPending}
                onClick={() => clear.mutate(undefined)}
              >
                <Trash2 />
              </Button>
            </Tooltip>
          </div>
        )}
      </div>

      {!collapsed && (
        <div className="flex min-h-0 flex-1">
          <div className={cn('min-h-0 overflow-auto', current ? 'w-1/2 border-r' : 'w-full')}>
            {logs.error ? (
              <ErrorNotice error={logs.error} className="m-2" />
            ) : rows.length === 0 ? (
              <EmptyState title="No requests yet" className="py-6">
                {live
                  ? 'Call the mock server from any client, or use “Try it” on an endpoint.'
                  : 'Start the mock server, then send requests to it.'}
              </EmptyState>
            ) : (
              <table className="w-full text-xs">
                <thead className="sticky top-0 bg-card text-left text-muted-foreground">
                  <tr className="border-b">
                    <th className="px-2 py-1 font-medium">Time</th>
                    <th className="px-2 py-1 font-medium">Method</th>
                    <th className="px-2 py-1 font-medium">Path</th>
                    <th className="px-2 py-1 font-medium">Status</th>
                    <th className="px-2 py-1 text-right font-medium">Time (ms)</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <tr
                      key={r.id}
                      tabIndex={0}
                      aria-selected={r.id === selected}
                      onClick={() => setSelected(r.id === selected ? null : r.id)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') {
                          e.preventDefault()
                          setSelected(r.id === selected ? null : r.id)
                        }
                      }}
                      className={cn(
                        'cursor-pointer border-b border-border/60 hover:bg-accent/50',
                        r.id === selected && 'bg-accent',
                      )}
                    >
                      <td className="px-2 py-1 font-mono whitespace-nowrap text-muted-foreground">
                        {formatTime(r.timestamp)}
                      </td>
                      <td className="px-2 py-1">
                        <MethodBadge method={r.method} />
                      </td>
                      <td className="max-w-0 truncate px-2 py-1 font-mono">{r.path}</td>
                      <td className="px-2 py-1">
                        <StatusBadge status={r.status} />
                      </td>
                      <td className="px-2 py-1 text-right font-mono">{r.duration_ms}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
          {current && (
            <div className="min-h-0 w-1/2 overflow-auto">
              <Detail log={current} projectId={projectId} />
            </div>
          )}
        </div>
      )}
    </section>
  )
}
