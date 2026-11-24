import { Play, Send } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

import { CodeEditor } from '@/components/editor'
import { PairsEditor } from '@/components/fields'
import { Button } from '@/components/ui/button'
import { Field, Input, Select } from '@/components/ui/form'
import { EmptyState, ErrorNotice, Kbd, Spinner, StatusBadge } from '@/components/ui/misc'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { keys, useAction, useSelectedEnvironment, useServers } from '@/lib/queries'
import { type EndpointDef, METHODS, type Method } from '@/lib/types'
import { formatBytes, isMod, modKey, pathParams } from '@/lib/utils'

interface Result {
  status: number
  statusText: string
  ms: number
  size: number
  headers: [string, string][]
  body: string
  json: boolean
}

function prettyBody(text: string): { body: string; json: boolean } {
  try {
    return { body: JSON.stringify(JSON.parse(text), null, 2), json: true }
  } catch {
    return { body: text, json: false }
  }
}

/**
 * Sends real requests to the project's running mock server, so they also
 * show up in request history. The mock server allows this page's origin
 * through CORS.
 */
export function RequestClient({ projectId, endpoint }: { projectId: string; endpoint: EndpointDef }) {
  const servers = useServers(projectId)
  const server = servers.data?.[0]
  const [envId] = useSelectedEnvironment(projectId)
  const start = useAction(() => api.startServer(projectId, { environment_id: envId || null }), {
    invalidate: [keys.servers(projectId)],
  })

  const [method, setMethod] = useState<Method>(endpoint.method)
  const [params, setParams] = useState<Record<string, string>>({})
  const [query, setQuery] = useState<[string, string][]>([])
  const [headers, setHeaders] = useState<[string, string][]>([])
  const [body, setBody] = useState('')
  const [result, setResult] = useState<Result | null>(null)
  const [error, setError] = useState<unknown>(null)
  const [sending, setSending] = useState(false)
  const controller = useRef<AbortController | null>(null)

  // Prefill from the endpoint definition when switching endpoints.
  useEffect(() => {
    setMethod(endpoint.method)
    setParams({})
    setQuery(endpoint.request.query.filter((q) => q.required).map((q) => [q.name, '']))
    const h: [string, string][] = endpoint.request.headers.filter((x) => x.required).map((x) => [x.name, ''])
    if (endpoint.auth.type === 'bearer') h.unshift(['Authorization', 'Bearer test-token'])
    if (endpoint.auth.type === 'api_key') h.unshift([endpoint.auth.header, 'test-key'])
    setHeaders(h)
    setBody(endpoint.request.body_example !== undefined ? JSON.stringify(endpoint.request.body_example, null, 2) : '')
    setResult(null)
    setError(null)
    // Only when the endpoint itself changes, not on every edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endpoint.id])

  const names = pathParams(endpoint.path)
  const path = endpoint.path
    .split('/')
    .map((seg) => {
      const name = seg.startsWith(':') ? seg.slice(1) : seg.startsWith('{') ? seg.slice(1, -1) : null
      if (!name) return seg
      return params[name] ? encodeURIComponent(params[name]) : `:${name}`
    })
    .join('/')
  const qs = new URLSearchParams(query.filter(([k]) => k.trim()))
  const url = server ? `${server.url}${path}${qs.size ? `?${qs}` : ''}` : ''
  const hasBody = method !== 'GET' && body.trim().length > 0

  const send = async () => {
    if (!server) return
    controller.current?.abort()
    const ac = new AbortController()
    controller.current = ac
    setSending(true)
    setError(null)
    const h = new Headers()
    for (const [k, v] of headers) if (k.trim()) h.append(k.trim(), v)
    if (hasBody && !h.has('content-type')) h.set('content-type', 'application/json')
    const startedAt = performance.now()
    try {
      const res = await fetch(url, { method, headers: h, body: hasBody ? body : undefined, signal: ac.signal })
      const text = await res.text()
      const ms = Math.round(performance.now() - startedAt)
      setResult({
        status: res.status,
        statusText: res.statusText,
        ms,
        size: new Blob([text]).size,
        headers: [...res.headers.entries()],
        ...prettyBody(text),
      })
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        setError(
          new Error(
            `The request failed before a response arrived (${(e as Error).message}). Is the mock server still running, and does it allow this page's origin (CORS)?`,
          ),
        )
      }
    } finally {
      setSending(false)
    }
  }

  if (!server) {
    return (
      <EmptyState
        title="Mock server is not running"
        action={
          <Button disabled={start.isPending} onClick={() => start.mutate(undefined)}>
            {start.isPending ? <Spinner className="text-current" /> : <Play className="fill-current" />} Start server
          </Button>
        }
      >
        Start it to send test requests. They appear in the request history below.
      </EmptyState>
    )
  }

  return (
    <div
      className="flex h-full flex-col"
      onKeyDown={(e) => {
        if (isMod(e) && e.key === 'Enter') {
          e.preventDefault()
          void send()
        }
      }}
    >
      <div className="flex flex-col gap-2 border-b p-3">
        <div className="flex items-center gap-1.5">
          <Select aria-label="Method" className="w-28" value={method} onChange={(e) => setMethod(e.target.value as Method)}>
            {METHODS.map((m) => (
              <option key={m}>{m}</option>
            ))}
          </Select>
          <Button className="ml-auto" disabled={sending} onClick={() => void send()}>
            {sending ? <Spinner className="text-current" /> : <Send />} Send <Kbd>{modKey} ↵</Kbd>
          </Button>
        </div>
        <code className="block truncate rounded bg-muted px-2 py-1 font-mono text-xs" title={url}>
          {url}
        </code>
        {names.length > 0 && (
          <div className="flex flex-wrap gap-2">
            {names.map((n) => (
              <Field key={n} label={`:${n}`} className="w-32">
                {(id) => (
                  <Input
                    id={id}
                    className="font-mono text-xs"
                    value={params[n] ?? ''}
                    onChange={(e) => setParams((p) => ({ ...p, [n]: e.target.value }))}
                  />
                )}
              </Field>
            ))}
          </div>
        )}
      </div>

      <Tabs defaultValue="query" className="flex min-h-0 flex-1 flex-col">
        <TabsList>
          <TabsTrigger value="query">Query</TabsTrigger>
          <TabsTrigger value="headers">Headers</TabsTrigger>
          <TabsTrigger value="body">Body</TabsTrigger>
        </TabsList>
        <TabsContent value="query" className="overflow-auto p-3">
          <PairsEditor value={query} onChange={setQuery} keyPlaceholder="Parameter" />
        </TabsContent>
        <TabsContent value="headers" className="overflow-auto p-3">
          <PairsEditor value={headers} onChange={setHeaders} keyPlaceholder="Header" />
        </TabsContent>
        <TabsContent value="body" className="flex flex-col gap-1 p-3">
          {method === 'GET' && <p className="text-xs text-muted-foreground">GET requests are sent without a body.</p>}
          <CodeEditor value={body} onChange={setBody} ariaLabel="Request body" height="100%" onSave={() => void send()} />
        </TabsContent>
      </Tabs>
      <section aria-label="Response" aria-live="polite" className="h-1/2 shrink-0 overflow-auto border-t p-3">
        <h3 className="mb-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">Response</h3>
        <ResponseView result={result} error={error} sending={sending} />
      </section>
    </div>
  )
}

function ResponseView({ result, error, sending }: { result: Result | null; error: unknown; sending: boolean }) {
  if (error) return <ErrorNotice error={error} />
  if (!result) return <p className="text-sm text-muted-foreground">{sending ? 'Sending…' : 'Send a request to see the response.'}</p>
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-3 text-xs">
        <StatusBadge status={result.status} />
        <span className="text-muted-foreground">{result.statusText}</span>
        <span className="text-muted-foreground">{result.ms} ms</span>
        <span className="text-muted-foreground">{formatBytes(result.size)}</span>
      </div>
      <details className="text-xs">
        <summary className="cursor-pointer text-muted-foreground">Headers ({result.headers.length})</summary>
        <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-3 font-mono">
          {result.headers.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted-foreground">{k}</dt>
              <dd className="break-all">{v}</dd>
            </div>
          ))}
        </dl>
      </details>
      {result.body ? (
        <pre className="overflow-auto rounded border bg-muted/40 p-2 font-mono text-xs whitespace-pre-wrap">{result.body}</pre>
      ) : (
        <p className="text-xs text-muted-foreground">Empty body</p>
      )}
    </div>
  )
}
