import { useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Check, CircleDot, Copy, Loader2, MoreHorizontal, Trash2 } from 'lucide-react'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { ConfirmDialog } from '@/components/dialogs'
import { JsonValue, ParamListEditor } from '@/components/fields'
import { InvalidFields, useInvalidFields } from '@/lib/invalidFields'
import { Button } from '@/components/ui/button'
import { Field, Input, Label, Select, Switch, Textarea } from '@/components/ui/form'
import { Badge, ErrorNotice, Kbd } from '@/components/ui/misc'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@/components/ui/overlay'
import { api, describeError } from '@/lib/api'
import { keys } from '@/lib/queries'
import { type AuthSimulation, type Endpoint, type EndpointDef, METHODS, type Method } from '@/lib/types'
import { modKey, pathParams } from '@/lib/utils'

import { ResponseEditor } from './ResponseEditor'
import { RulesEditor } from './RulesEditor'

function toDef(e: Endpoint | EndpointDef): EndpointDef {
  return {
    id: e.id,
    name: e.name,
    description: e.description,
    method: e.method,
    path: e.path,
    enabled: e.enabled,
    request: {
      headers: e.request.headers ?? [],
      query: e.request.query ?? [],
      ...(e.request.body_schema !== undefined ? { body_schema: e.request.body_schema } : {}),
      ...(e.request.body_example !== undefined ? { body_example: e.request.body_example } : {}),
    },
    response: e.response,
    rules: e.rules,
    auth: e.auth,
  }
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

type SaveState = 'saved' | 'dirty' | 'saving' | 'error' | 'invalid'

function SaveIndicator({ state }: { state: SaveState }) {
  const map = {
    saved: { icon: <Check className="size-3.5" />, text: 'Saved', cls: 'text-muted-foreground' },
    dirty: { icon: <CircleDot className="size-3.5" />, text: 'Unsaved changes', cls: 'text-muted-foreground' },
    saving: { icon: <Loader2 className="size-3.5 animate-spin" />, text: 'Saving…', cls: 'text-muted-foreground' },
    error: { icon: <AlertCircle className="size-3.5" />, text: 'Not saved', cls: 'text-destructive' },
    invalid: { icon: <AlertCircle className="size-3.5" />, text: 'Fix errors to save', cls: 'text-destructive' },
  }[state]
  return (
    <span role="status" className={`flex items-center gap-1 text-xs ${map.cls}`}>
      {map.icon} {map.text}
    </span>
  )
}

/**
 * Edits one endpoint. Changes save automatically shortly after typing
 * stops (or at once with Ctrl/⌘+S) and apply to running mock servers.
 */
export function EndpointEditor({ projectId, endpoint }: { projectId: string; endpoint: Endpoint }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [draft, setDraft] = useState<EndpointDef>(() => toDef(endpoint))
  const [saved, setSaved] = useState<EndpointDef>(() => toDef(endpoint))
  const [invalid, report] = useInvalidFields()
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)

  const dirty = !same(draft, saved)
  const state: SaveState = invalid.size ? 'invalid' : saving ? 'saving' : error ? 'error' : dirty ? 'dirty' : 'saved'

  // Refs so the unmount flush and the debounced save see current values.
  const latest = useRef({ draft, saved, invalid: invalid.size, saving })
  useLayoutEffect(() => {
    latest.current = { draft, saved, invalid: invalid.size, saving }
  })

  const applySaved = useCallback(
    (result: Endpoint, sent: EndpointDef) => {
      const def = toDef(result)
      setSaved(def)
      // The server normalizes (e.g. `{id}` → `:id`); adopt that unless the
      // user typed more while saving.
      setDraft((d) => (same(d, sent) ? def : d))
      qc.setQueryData<Endpoint[]>(keys.endpoints(projectId), (list) =>
        list?.map((e) => (e.id === result.id ? result : e)),
      )
      void qc.invalidateQueries({ queryKey: keys.servers(projectId) })
    },
    [qc, projectId],
  )

  const save = useCallback(async () => {
    const { draft: sent, saved: base, invalid: bad, saving: busy } = latest.current
    if (busy || bad || same(sent, base)) return
    setSaving(true)
    try {
      const result = await api.replaceEndpoint(sent.id, sent)
      applySaved(result, sent)
      setError(null)
    } catch (e) {
      setError(e)
    } finally {
      setSaving(false)
    }
  }, [applySaved])

  // Autosave after a pause in typing.
  useEffect(() => {
    if (!dirty || invalid.size) return
    const t = setTimeout(() => void save(), 800)
    return () => clearTimeout(t)
  }, [draft, dirty, invalid.size, save])

  // Save pending changes when leaving the editor.
  useEffect(
    () => () => {
      const { draft: d, saved: s, invalid: bad } = latest.current
      if (!bad && !same(d, s)) {
        api
          .replaceEndpoint(d.id, d)
          .then(() => qc.invalidateQueries({ queryKey: keys.endpoints(projectId) }))
          .catch((e) => toast.error(`“${d.name}” was not saved: ${describeError(e).title}`))
      }
    },
    [qc, projectId],
  )

  const set = (patch: Partial<EndpointDef>) => setDraft((d) => ({ ...d, ...patch }))
  const params = pathParams(draft.path)
  const pathError = draft.path.startsWith('/') ? undefined : 'The path must start with /'

  return (
    <InvalidFields.Provider value={report}>
      <div
        className="flex h-full min-h-0 flex-col"
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
            e.preventDefault()
            void save()
          }
        }}
      >
        <div className="flex flex-col gap-2 border-b p-3">
          <div className="flex flex-wrap items-center gap-2">
            <Input
              aria-label="Endpoint name"
              className="h-8 min-w-40 max-w-md flex-1 border-transparent bg-transparent px-1 text-base font-semibold shadow-none hover:border-input focus:border-input"
              value={draft.name}
              onChange={(e) => set({ name: e.target.value })}
            />
            {!draft.enabled && <Badge tone="muted">Disabled</Badge>}
            <div className="ml-auto flex shrink-0 items-center gap-3">
              <SaveIndicator state={state} />
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                <Switch checked={draft.enabled} onCheckedChange={(enabled) => set({ enabled })} aria-label="Enabled" />
                Enabled
              </label>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon-sm" aria-label="Endpoint actions">
                    <MoreHorizontal />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem
                    onSelect={async () => {
                      try {
                        await save()
                        const copy = await api.duplicateEndpoint(endpoint.id)
                        await qc.invalidateQueries({ queryKey: keys.endpoints(projectId) })
                        toast.success('Duplicated; the copy starts disabled.')
                        navigate(`/p/${projectId}/endpoints/${copy.id}`)
                      } catch (e) {
                        toast.error(describeError(e).title)
                      }
                    }}
                  >
                    <Copy /> Duplicate
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => {
                      void navigator.clipboard.writeText(endpoint.id)
                      toast.success('Endpoint ID copied')
                    }}
                  >
                    <Copy /> Copy ID
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem destructive onSelect={() => setConfirmDelete(true)}>
                    <Trash2 /> Delete
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          <div className="flex items-start gap-1.5">
            <Select
              aria-label="Method"
              className="w-28 font-mono font-semibold"
              value={draft.method}
              onChange={(e) => set({ method: e.target.value as Method })}
            >
              {METHODS.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </Select>
            <div className="flex flex-1 flex-col gap-1">
              <Input
                aria-label="Path"
                className="font-mono"
                placeholder="/users/:id"
                value={draft.path}
                aria-invalid={!!pathError}
                onChange={(e) => set({ path: e.target.value })}
              />
              {pathError && <p className="text-xs text-destructive">{pathError}</p>}
            </div>
          </div>
          {invalid.size > 0 && (
            <ul role="alert" className="list-disc pl-5 text-xs text-destructive">
              {[...invalid.values()].map((m) => (
                <li key={m}>{m}</li>
              ))}
            </ul>
          )}
          <ErrorNotice error={error} />
        </div>

        <Tabs defaultValue="response" className="flex min-h-0 flex-1 flex-col">
          <TabsList>
            <TabsTrigger value="response">Response</TabsTrigger>
            <TabsTrigger value="rules">
              Rules {draft.rules.length > 0 && <Badge>{draft.rules.length}</Badge>}
            </TabsTrigger>
            <TabsTrigger value="request">Request</TabsTrigger>
            <TabsTrigger value="details">Auth & details</TabsTrigger>
            <span className="ml-auto hidden text-[11px] text-muted-foreground lg:inline">
              Autosaves · <Kbd>{modKey} S</Kbd> saves now
            </span>
          </TabsList>
          <TabsContent value="response" className="overflow-y-auto p-3">
            <ResponseEditor label="Default response" value={draft.response} onChange={(response) => set({ response })} onSave={() => void save()} />
          </TabsContent>
          <TabsContent value="rules" className="overflow-y-auto p-3">
            <RulesEditor value={draft.rules} pathParams={params} onChange={(rules) => set({ rules })} onSave={() => void save()} />
          </TabsContent>
          <TabsContent value="request" className="overflow-y-auto p-3">
            <div className="flex flex-col gap-5">
              <div className="flex flex-col gap-1.5">
                <Label>Query parameters</Label>
                <ParamListEditor
                  kind="query parameter"
                  value={draft.request.query}
                  onChange={(query) => set({ request: { ...draft.request, query } })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>Headers</Label>
                <ParamListEditor
                  kind="header"
                  value={draft.request.headers}
                  onChange={(headers) => set({ request: { ...draft.request, headers } })}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>Body schema (JSON Schema; leave empty to accept any body)</Label>
                <JsonValue
                  label="Body schema"
                  allowEmpty
                  height={220}
                  value={draft.request.body_schema}
                  onSave={() => void save()}
                  onChange={(body_schema) => {
                    const { body_schema: _, ...rest } = draft.request
                    set({ request: body_schema === undefined ? rest : { ...rest, body_schema } })
                  }}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label>Example body (shown in “Try it” and exported to OpenAPI)</Label>
                <JsonValue
                  label="Example body"
                  allowEmpty
                  height={160}
                  value={draft.request.body_example}
                  onSave={() => void save()}
                  onChange={(body_example) => {
                    const { body_example: _, ...rest } = draft.request
                    set({ request: body_example === undefined ? rest : { ...rest, body_example } })
                  }}
                />
              </div>
            </div>
          </TabsContent>
          <TabsContent value="details" className="overflow-y-auto p-3">
            <div className="flex max-w-xl flex-col gap-4">
              <AuthEditor value={draft.auth} onChange={(auth) => set({ auth })} />
              <Field label="Description">
                {(id) => (
                  <Textarea
                    id={id}
                    className="font-sans text-sm"
                    value={draft.description}
                    onChange={(e) => set({ description: e.target.value })}
                  />
                )}
              </Field>
            </div>
          </TabsContent>
        </Tabs>

        <ConfirmDialog
          open={confirmDelete}
          onOpenChange={setConfirmDelete}
          title={`Delete “${endpoint.name}”?`}
          description="Running mock servers stop answering this route. This cannot be undone."
          onConfirm={async () => {
            await api.deleteEndpoint(endpoint.id)
            latest.current.saved = latest.current.draft
            qc.setQueryData<Endpoint[]>(keys.endpoints(projectId), (list) => list?.filter((e) => e.id !== endpoint.id))
            navigate(`/p/${projectId}/endpoints`)
          }}
        />
      </div>
    </InvalidFields.Provider>
  )
}

function AuthEditor({ value, onChange }: { value: AuthSimulation; onChange: (a: AuthSimulation) => void }) {
  return (
    <div className="flex flex-col gap-2">
      <Field
        label="Authentication"
        hint="Simulated: any credential is accepted, only its presence is checked. Missing credentials get 401."
      >
        {(id) => (
          <Select
            id={id}
            value={value.type}
            onChange={(e) => {
              const type = e.target.value as AuthSimulation['type']
              onChange(type === 'api_key' ? { type, header: value.type === 'api_key' ? value.header : 'X-API-Key' } : { type })
            }}
          >
            <option value="none">None</option>
            <option value="bearer">Bearer token (Authorization header)</option>
            <option value="api_key">API key header</option>
          </Select>
        )}
      </Field>
      {value.type === 'api_key' && (
        <Field label="Header name">
          {(id) => (
            <Input
              id={id}
              className="font-mono"
              value={value.header}
              onChange={(e) => onChange({ type: 'api_key', header: e.target.value })}
            />
          )}
        </Field>
      )}
    </div>
  )
}
