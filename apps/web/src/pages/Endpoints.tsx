import { useQueryClient } from '@tanstack/react-query'
import { FileInput, GripVertical, Plus, Route, Search, X } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router'
import { useDefaultLayout } from 'react-resizable-panels'
import { toast } from 'sonner'

import { EndpointEditor } from '@/components/endpoint/EndpointEditor'
import { RequestClient } from '@/components/endpoint/RequestClient'
import { Button } from '@/components/ui/button'
import { Field, Input, Select } from '@/components/ui/form'
import { EmptyState, ErrorNotice, MethodBadge, Spinner } from '@/components/ui/misc'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/overlay'
import { ResizableGroup, ResizableHandle, ResizablePanel } from '@/components/ui/resizable'
import { api, describeError } from '@/lib/api'
import { keys, useEndpoints, useStoredString } from '@/lib/queries'
import { type Endpoint, METHODS, type Method } from '@/lib/types'
import { useMediaQuery } from '@/lib/useMediaQuery'
import { cn } from '@/lib/utils'

function NewEndpointDialog({
  projectId,
  open,
  onOpenChange,
}: {
  projectId: string
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [method, setMethod] = useState<Method>('GET')
  const [path, setPath] = useState('/')
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (o) {
          setMethod('GET')
          setPath('/')
          setName('')
        }
        setError(null)
        onOpenChange(o)
      }}
    >
      <DialogContent title="New endpoint" description="Use :name or {name} for path parameters, e.g. /users/:id.">
        <form
          className="flex flex-col gap-4"
          onSubmit={async (e) => {
            e.preventDefault()
            setBusy(true)
            setError(null)
            try {
              const created = await api.createEndpoint(projectId, {
                method,
                path: path.trim(),
                name: name.trim() || `${method} ${path.trim()}`,
                response: { status: method === 'POST' ? 201 : 200, headers: {}, delay_ms: 0, body: { type: 'json', content: { ok: true } } },
              })
              await qc.invalidateQueries({ queryKey: keys.endpoints(projectId) })
              onOpenChange(false)
              navigate(`/p/${projectId}/endpoints/${created.id}`)
            } catch (err) {
              setError(err)
            } finally {
              setBusy(false)
            }
          }}
        >
          <div className="flex gap-2">
            <Field label="Method" className="w-28">
              {(id) => (
                <Select id={id} value={method} onChange={(e) => setMethod(e.target.value as Method)}>
                  {METHODS.map((m) => (
                    <option key={m}>{m}</option>
                  ))}
                </Select>
              )}
            </Field>
            <Field label="Path" className="flex-1">
              {(id) => (
                <Input id={id} autoFocus className="font-mono" value={path} onChange={(e) => setPath(e.target.value)} />
              )}
            </Field>
          </div>
          <Field label="Name (optional)">
            {(id) => <Input id={id} placeholder="Get user" value={name} onChange={(e) => setName(e.target.value)} />}
          </Field>
          <ErrorNotice error={error} />
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !path.trim()}>
              {busy && <Spinner className="text-current" />} Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

function EndpointList({
  projectId,
  endpoints,
  activeId,
  onNew,
}: {
  projectId: string
  endpoints: Endpoint[]
  activeId?: string
  onNew: () => void
}) {
  const qc = useQueryClient()
  const [search, setSearch] = useState('')
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)
  const q = search.trim().toLowerCase()
  const shown = q
    ? endpoints.filter((e) => `${e.method} ${e.path} ${e.name}`.toLowerCase().includes(q))
    : endpoints

  const reorder = async (from: string, to: string) => {
    if (from === to) return
    const ids = endpoints.map((e) => e.id)
    const next = ids.filter((id) => id !== from)
    next.splice(next.indexOf(to) + (ids.indexOf(from) < ids.indexOf(to) ? 1 : 0), 0, from)
    const byId = new Map(endpoints.map((e) => [e.id, e]))
    qc.setQueryData<Endpoint[]>(keys.endpoints(projectId), next.map((id) => byId.get(id)!))
    try {
      await api.reorderEndpoints(projectId, next)
    } catch (e) {
      toast.error(describeError(e).title)
    }
    void qc.invalidateQueries({ queryKey: keys.endpoints(projectId) })
  }

  return (
    <div className="flex h-full flex-col bg-sidebar">
      <div className="flex items-center gap-1.5 border-b p-2">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute top-2 left-2 size-4 text-muted-foreground" />
          <Input
            aria-label="Search endpoints"
            placeholder="Search endpoints"
            className="pl-7"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <Button size="icon-sm" aria-label="New endpoint" onClick={onNew}>
          <Plus />
        </Button>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto p-1" aria-label="Endpoints">
        {shown.map((e) => (
          <li
            key={e.id}
            draggable={!q}
            onDragStart={(ev) => {
              setDragId(e.id)
              ev.dataTransfer.effectAllowed = 'move'
            }}
            onDragOver={(ev) => {
              if (!dragId) return
              ev.preventDefault()
              setOverId(e.id)
            }}
            onDragLeave={() => setOverId((o) => (o === e.id ? null : o))}
            onDrop={(ev) => {
              ev.preventDefault()
              if (dragId) void reorder(dragId, e.id)
              setDragId(null)
              setOverId(null)
            }}
            onDragEnd={() => {
              setDragId(null)
              setOverId(null)
            }}
            className={cn(
              'group rounded-md',
              overId === e.id && dragId !== e.id && 'ring-2 ring-primary/60',
              dragId === e.id && 'opacity-50',
            )}
          >
            <Link
              to={`/p/${projectId}/endpoints/${e.id}`}
              aria-current={e.id === activeId ? 'page' : undefined}
              className={cn(
                'flex items-center gap-1 rounded-md px-1.5 py-1.5 text-sm',
                e.id === activeId ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60',
                !e.enabled && 'opacity-55',
              )}
            >
              {!q && (
                <GripVertical
                  aria-hidden
                  className="size-3.5 shrink-0 cursor-grab text-muted-foreground opacity-0 group-hover:opacity-100"
                />
              )}
              <MethodBadge method={e.method} />
              <span className="flex min-w-0 flex-col">
                <span className="truncate font-mono text-xs">{e.path}</span>
                <span className="truncate text-[11px] text-muted-foreground">{e.name}</span>
              </span>
            </Link>
          </li>
        ))}
        {shown.length === 0 && (
          <li className="p-4 text-center text-xs text-muted-foreground">
            {q ? 'No endpoints match.' : 'No endpoints yet.'}
          </li>
        )}
      </ul>
      <p className="border-t px-2 py-1.5 text-[11px] text-muted-foreground">
        {endpoints.length} endpoints{!q && endpoints.length > 1 ? ' · drag to reorder' : ''}
      </p>
    </div>
  )
}

/** Endpoint IDs open as tabs in this project. */
function useOpenTabs(projectId: string): [string[], (ids: string[]) => void] {
  const [raw, setRaw] = useStoredString(`workbench.tabs.${projectId}`, '[]')
  const ids = useMemo(() => {
    try {
      const v = JSON.parse(raw)
      return Array.isArray(v) ? (v.filter((x) => typeof x === 'string') as string[]) : []
    } catch {
      return []
    }
  }, [raw])
  return [ids, (next) => setRaw(JSON.stringify(next))]
}

export function EndpointsPage() {
  const { projectId, endpointId } = useParams()
  const [searchParams, setSearchParams] = useSearchParams()
  const navigate = useNavigate()
  const endpoints = useEndpoints(projectId)
  const [tabs, setTabs] = useOpenTabs(projectId!)
  const [newOpen, setNewOpen] = useState(false)
  const wide = useMediaQuery('(min-width: 1024px)')
  const layout = useDefaultLayout({
    id: wide ? 'workbench.endpoints-layout' : 'workbench.endpoints-layout-narrow',
    storage: localStorage,
  })
  const list = useMemo(() => endpoints.data ?? [], [endpoints.data])
  const active = list.find((e) => e.id === endpointId)
  const openTabs = useMemo(
    () => tabs.map((id) => list.find((e) => e.id === id)).filter((e): e is Endpoint => !!e),
    [tabs, list],
  )
  // `?new=1` (from the command palette) opens the new-endpoint dialog.
  const showNew = newOpen || searchParams.has('new')
  const setShowNew = (open: boolean) => {
    setNewOpen(open)
    if (!open && searchParams.has('new')) setSearchParams({}, { replace: true })
  }

  // Opening an endpoint adds a tab.
  useEffect(() => {
    if (active && !tabs.includes(active.id)) setTabs([...tabs, active.id].slice(-8))
  }, [active, tabs, setTabs])

  // Without a selection, reopen the last tab or the first endpoint.
  useEffect(() => {
    if (!endpointId && list.length) {
      const target = openTabs.at(-1) ?? list[0]
      navigate(`/p/${projectId}/endpoints/${target.id}`, { replace: true })
    }
  }, [endpointId, list, openTabs, navigate, projectId])

  const closeTab = (id: string) => {
    const remaining = tabs.filter((t) => t !== id)
    setTabs(remaining)
    if (id === endpointId) {
      const next = remaining.at(-1)
      navigate(next ? `/p/${projectId}/endpoints/${next}` : `/p/${projectId}/endpoints`)
    }
  }

  if (endpoints.isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    )
  }
  if (endpoints.error) return <ErrorNotice error={endpoints.error} className="m-4" />

  return (
    <>
      <ResizableGroup key={wide ? 'wide' : 'narrow'} orientation="horizontal" defaultLayout={layout.defaultLayout} onLayoutChanged={layout.onLayoutChanged}>
        <ResizablePanel id="list" defaultSize={wide ? '22%' : '38%'} minSize="140px" maxSize="50%">
          <EndpointList projectId={projectId!} endpoints={list} activeId={endpointId} onNew={() => setShowNew(true)} />
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="editor" minSize="30%">
          {list.length === 0 ? (
            <EmptyState
              icon={<Route />}
              title="No endpoints yet"
              className="h-full"
              action={
                <div className="flex flex-wrap justify-center gap-2">
                  <Button onClick={() => setShowNew(true)}>
                    <Plus /> New endpoint
                  </Button>
                  <Button variant="outline" asChild>
                    <Link to={`/p/${projectId}/import-export`}>
                      <FileInput /> Import OpenAPI or cURL
                    </Link>
                  </Button>
                </div>
              }
            >
              An endpoint is a method and path with the response the mock server returns.
            </EmptyState>
          ) : (
            <div className="flex h-full flex-col">
              <div role="tablist" aria-label="Open endpoints" className="flex shrink-0 overflow-x-auto border-b bg-sidebar">
                {openTabs.map((e) => (
                  <div
                    key={e.id}
                    role="tab"
                    aria-selected={e.id === endpointId}
                    className={cn(
                      'group flex max-w-56 shrink-0 items-center gap-1 border-r pr-1 pl-2 text-xs',
                      e.id === endpointId ? 'bg-background' : 'text-muted-foreground hover:bg-accent/50',
                    )}
                  >
                    <Link
                      to={`/p/${projectId}/endpoints/${e.id}`}
                      className="flex min-w-0 items-center gap-1 py-2"
                      onAuxClick={(ev) => {
                        if (ev.button === 1) closeTab(e.id)
                      }}
                    >
                      <MethodBadge method={e.method} className="w-auto" />
                      <span className="truncate">{e.name}</span>
                    </Link>
                    <button
                      type="button"
                      aria-label={`Close ${e.name}`}
                      className="rounded p-0.5 opacity-60 hover:bg-accent hover:opacity-100"
                      onClick={() => closeTab(e.id)}
                    >
                      <X className="size-3" />
                    </button>
                  </div>
                ))}
              </div>
              <div className="min-h-0 flex-1">
                {active ? (
                  <EndpointEditor key={active.id} projectId={projectId!} endpoint={active} />
                ) : (
                  <EmptyState title="Endpoint not found" className="h-full">
                    It may have been deleted. Pick another one from the list.
                  </EmptyState>
                )}
              </div>
            </div>
          )}
        </ResizablePanel>
        {active && wide && (
          <>
            <ResizableHandle />
            <ResizablePanel id="client" defaultSize="30%" minSize="260px" collapsible collapsedSize="0px">
              <div className="flex h-full flex-col bg-card">
                <h2 className="border-b px-3 py-2 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
                  Try it
                </h2>
                <div className="min-h-0 flex-1">
                  <RequestClient projectId={projectId!} endpoint={active} />
                </div>
              </div>
            </ResizablePanel>
          </>
        )}
      </ResizableGroup>
      <NewEndpointDialog projectId={projectId!} open={showNew} onOpenChange={setShowNew} />
    </>
  )
}
