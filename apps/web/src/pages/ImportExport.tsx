import { useQueryClient } from '@tanstack/react-query'
import { Download, Eye, FileCode2, FileUp, Terminal, Upload } from 'lucide-react'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'

import { CodeEditor } from '@/components/editor'
import { Button } from '@/components/ui/button'
import { Checkbox, Textarea } from '@/components/ui/form'
import { Badge, ErrorNotice, IssueList, MethodBadge, Spinner } from '@/components/ui/misc'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/overlay'
import { api, describeError, saveFile } from '@/lib/api'
import { keys, useProject } from '@/lib/queries'
import type { CurlPreview, OpenApiImportResult, OpenApiPreview } from '@/lib/types'

function Card({ title, icon, children }: { title: string; icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <section aria-label={title} className="flex flex-col gap-3 rounded-lg border bg-card p-4">
      <h2 className="flex items-center gap-2 text-sm font-semibold [&_svg]:size-4">
        {icon} {title}
      </h2>
      {children}
    </section>
  )
}

function OpenApiImport({ projectId }: { projectId: string }) {
  const qc = useQueryClient()
  const project = useProject(projectId)
  const navigate = useNavigate()
  const [text, setText] = useState('')
  const [preview, setPreview] = useState<OpenApiPreview | null>(null)
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [setBase, setSetBase] = useState(true)
  const [result, setResult] = useState<OpenApiImportResult | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  const run = async (fn: () => Promise<void>) => {
    setBusy(true)
    setError(null)
    try {
      await fn()
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  const doPreview = () =>
    run(async () => {
      setResult(null)
      const p = await api.previewOpenApi(projectId, text)
      setPreview(p)
      setChosen(new Set(p.operations.filter((o) => o.importable).map((o) => o.key)))
    })

  const doImport = () =>
    run(async () => {
      const r = await api.importOpenApi(projectId, text, [...chosen], setBase)
      setResult(r)
      setPreview(null)
      await qc.invalidateQueries({ queryKey: keys.endpoints(projectId) })
      await qc.invalidateQueries({ queryKey: keys.project(projectId) })
      toast.success(`Imported ${r.imported.length} endpoints`)
    })

  const importable = preview?.operations.filter((o) => o.importable) ?? []
  const allChosen = importable.length > 0 && importable.every((o) => chosen.has(o.key))

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        OpenAPI 3.0, 3.1 or 3.2 as YAML or JSON. Preview first, then pick the operations to import. Nothing is dropped
        silently: anything skipped or approximated is listed.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <label className="inline-flex">
          <span className="sr-only">Load an OpenAPI file</span>
          <input
            type="file"
            accept=".yaml,.yml,.json,application/json,application/yaml"
            className="text-xs file:mr-2 file:rounded-md file:border file:bg-secondary file:px-2 file:py-1 file:text-xs"
            onChange={async (e) => {
              const f = e.target.files?.[0]
              if (f) {
                setText(await f.text())
                setPreview(null)
                setResult(null)
              }
            }}
          />
        </label>
      </div>
      <CodeEditor
        language="plaintext"
        height={240}
        ariaLabel="OpenAPI document"
        value={text}
        onChange={(t) => {
          setText(t)
          setPreview(null)
        }}
      />
      <div className="flex gap-2">
        <Button variant="secondary" disabled={!text.trim() || busy} onClick={doPreview}>
          {busy && !preview ? <Spinner /> : <Eye />} Preview
        </Button>
      </div>
      <ErrorNotice error={error} />

      {preview && (
        <div className="flex flex-col gap-3">
          <div className="text-sm">
            <span className="font-medium">{preview.title}</span>{' '}
            <span className="text-muted-foreground">
              {preview.version} · OpenAPI {preview.openapi}
            </span>
          </div>
          <IssueList issues={preview.issues} />
          <div className="overflow-auto rounded-md border">
            <table className="w-full text-xs">
              <thead className="bg-muted/50 text-left">
                <tr>
                  <th className="w-8 px-2 py-1.5">
                    <Checkbox
                      aria-label="Select all importable operations"
                      checked={allChosen}
                      onChange={(e) => setChosen(e.target.checked ? new Set(importable.map((o) => o.key)) : new Set())}
                    />
                  </th>
                  <th className="px-2 py-1.5 font-medium">Operation</th>
                  <th className="px-2 py-1.5 font-medium">Notes</th>
                </tr>
              </thead>
              <tbody>
                {preview.operations.map((o) => (
                  <tr key={o.key} className="border-t align-top">
                    <td className="px-2 py-1.5">
                      <Checkbox
                        aria-label={`Import ${o.key}`}
                        disabled={!o.importable}
                        checked={chosen.has(o.key)}
                        onChange={(e) => {
                          const next = new Set(chosen)
                          if (e.target.checked) next.add(o.key)
                          else next.delete(o.key)
                          setChosen(next)
                        }}
                      />
                    </td>
                    <td className="px-2 py-1.5">
                      <div className="flex items-center gap-1">
                        <MethodBadge method={o.method} />
                        <code className="font-mono">{o.path}</code>
                      </div>
                      <div className="text-muted-foreground">{o.name}</div>
                    </td>
                    <td className="px-2 py-1.5">
                      {!o.importable && <Badge tone="destructive">not importable</Badge>}
                      {o.conflicts_with_project && (
                        <Badge tone="warning">route exists · imported disabled</Badge>
                      )}
                      <IssueList issues={o.issues} className="mt-1" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {preview.base_path && (
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                aria-label={`Use ${preview.base_path} as the project base path`}
                checked={setBase}
                disabled={!!project.data?.base_path}
                onChange={(e) => setSetBase(e.target.checked)}
              />
              Use <code className="font-mono">{preview.base_path}</code> as the project base path
              {project.data?.base_path && (
                <span className="text-xs text-muted-foreground">(the project already has {project.data.base_path})</span>
              )}
            </label>
          )}
          <div>
            <Button disabled={!chosen.size || busy} onClick={doImport}>
              {busy ? <Spinner className="text-current" /> : <Upload />} Import {chosen.size} {chosen.size === 1 ? 'operation' : 'operations'}
            </Button>
          </div>
        </div>
      )}

      {result && (
        <div className="flex flex-col gap-2 rounded-md border p-3 text-sm">
          <p>
            Imported <strong>{result.imported.length}</strong> {result.imported.length === 1 ? 'endpoint' : 'endpoints'}
            {result.base_path_set && (
              <>
                {' '}
                and set the base path to <code className="font-mono">{result.base_path_set}</code>
              </>
            )}
            .
          </p>
          {result.disabled_due_to_conflicts.length > 0 && (
            <p className="text-xs text-amber-700 dark:text-amber-300">
              Imported disabled because the route already exists: {result.disabled_due_to_conflicts.join(', ')}
            </p>
          )}
          <IssueList issues={result.issues} />
          <div>
            <Button variant="outline" size="sm" onClick={() => navigate(`/p/${projectId}/endpoints/${result.imported[0]?.id ?? ''}`)}>
              Open endpoints
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

function CurlImport({ projectId }: { projectId: string }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [command, setCommand] = useState('')
  const [keep, setKeep] = useState(false)
  const [preview, setPreview] = useState<CurlPreview | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  const parse = async () => {
    setBusy(true)
    setError(null)
    try {
      setPreview(await api.parseCurl(command, projectId, keep))
    } catch (e) {
      setPreview(null)
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  const create = async () => {
    if (!preview?.endpoint) return
    setBusy(true)
    try {
      const { id: _id, ...def } = preview.endpoint
      const created = await api.createEndpoint(projectId, def)
      await qc.invalidateQueries({ queryKey: keys.endpoints(projectId) })
      toast.success('Endpoint created; edit its response next.')
      navigate(`/p/${projectId}/endpoints/${created.id}`)
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  const r = preview?.request
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-muted-foreground">
        Paste a cURL command (bash, cmd or PowerShell). It is parsed, never run, and files it mentions are not read.
      </p>
      <Textarea
        aria-label="cURL command"
        className="min-h-28"
        placeholder={`curl -X POST https://api.example.test/users \\\n  -H 'Content-Type: application/json' \\\n  -d '{"name":"Sari"}'`}
        value={command}
        onChange={(e) => setCommand(e.target.value)}
      />
      <label className="flex items-center gap-2 text-sm">
        <Checkbox checked={keep} onChange={(e) => setKeep(e.target.checked)} />
        Keep credentials (tokens, passwords, cookies) in the preview
      </label>
      <div>
        <Button variant="secondary" disabled={!command.trim() || busy} onClick={() => void parse()}>
          {busy && !preview ? <Spinner /> : <Eye />} Parse
        </Button>
      </div>
      <ErrorNotice error={error} />
      {r && preview && (
        <div className="flex flex-col gap-3 rounded-md border p-3 text-xs">
          <div className="flex items-center gap-2">
            <MethodBadge method={r.method} className="w-auto" />
            <code className="font-mono break-all">{r.url}</code>
          </div>
          {r.headers.length > 0 && (
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 font-mono">
              {r.headers.map((h, i) => (
                <div key={i} className="contents">
                  <dt className="text-muted-foreground">{h.name}</dt>
                  <dd className="break-all">{h.value}</dd>
                </div>
              ))}
            </dl>
          )}
          {r.body && (
            <pre className="max-h-40 overflow-auto rounded bg-muted/50 p-2 font-mono whitespace-pre-wrap">
              {r.body.type === 'json' ? JSON.stringify(r.body.content, null, 2) : r.body.content}
            </pre>
          )}
          {r.redacted.length > 0 && <p className="text-muted-foreground">Redacted: {r.redacted.join(', ')}</p>}
          {r.warnings.length > 0 && (
            <ul className="list-disc pl-5 text-amber-700 dark:text-amber-300">
              {r.warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          )}
          {preview.endpoint ? (
            <div className="flex flex-wrap items-center gap-2 border-t pt-3">
              <span>Endpoint draft:</span>
              <MethodBadge method={preview.endpoint.method} className="w-auto" />
              <code className="font-mono">{preview.endpoint.path}</code>
              {preview.endpoint.auth.type !== 'none' && <Badge>{preview.endpoint.auth.type} auth</Badge>}
              <Button size="sm" className="ml-auto" disabled={busy} onClick={() => void create()}>
                Create endpoint
              </Button>
            </div>
          ) : (
            <p className="text-destructive">{preview.endpoint_error}</p>
          )}
        </div>
      )}
    </div>
  )
}

function Exports({ projectId }: { projectId: string }) {
  const [records, setRecords] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const get = async (what: string, fn: () => Promise<Parameters<typeof saveFile>[0]>) => {
    setBusy(what)
    try {
      saveFile(await fn())
    } catch (e) {
      toast.error(describeError(e).title)
    } finally {
      setBusy(null)
    }
  }
  return (
    <div className="grid gap-4 md:grid-cols-2">
      <Card title="OpenAPI document" icon={<FileCode2 />}>
        <p className="text-sm text-muted-foreground">
          OpenAPI 3.1 with every endpoint and CRUD resource. Rules, delays and disabled endpoints are kept in an{' '}
          <code className="font-mono">x-workbench</code> extension so they survive a round trip.
        </p>
        <div className="flex gap-2">
          <Button variant="outline" disabled={!!busy} onClick={() => void get('yaml', () => api.exportOpenApi(projectId, 'yaml'))}>
            {busy === 'yaml' ? <Spinner /> : <Download />} YAML
          </Button>
          <Button variant="outline" disabled={!!busy} onClick={() => void get('json', () => api.exportOpenApi(projectId, 'json'))}>
            {busy === 'json' ? <Spinner /> : <Download />} JSON
          </Button>
        </div>
      </Card>
      <Card title="Project archive" icon={<FileUp />}>
        <p className="text-sm text-muted-foreground">
          A portable .zip of the whole project for another machine or deployment. Secret values are never included.
        </p>
        <label className="flex items-center gap-2 text-sm">
          <Checkbox checked={records} onChange={(e) => setRecords(e.target.checked)} /> Include CRUD resource records
        </label>
        <div>
          <Button
            variant="outline"
            disabled={!!busy}
            onClick={() => void get('zip', () => api.exportArchive(projectId, records))}
          >
            {busy === 'zip' ? <Spinner /> : <Download />} Download .zip
          </Button>
        </div>
      </Card>
    </div>
  )
}

export function ImportExportPage() {
  const { projectId } = useParams()
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-5xl flex-col gap-6 p-4">
        <div>
          <h1 className="text-lg font-semibold">Import / export</h1>
          <p className="text-sm text-muted-foreground">
            To import a whole project archive, use “Import archive” on the Dashboard; it creates a new project.
          </p>
        </div>
        <Tabs defaultValue="openapi">
          <TabsList className="px-0">
            <TabsTrigger value="openapi">
              <FileCode2 className="size-4" /> OpenAPI
            </TabsTrigger>
            <TabsTrigger value="curl">
              <Terminal className="size-4" /> cURL
            </TabsTrigger>
            <TabsTrigger value="export">
              <Download className="size-4" /> Export
            </TabsTrigger>
          </TabsList>
          <TabsContent value="openapi" className="pt-4">
            <OpenApiImport projectId={projectId!} />
          </TabsContent>
          <TabsContent value="curl" className="pt-4">
            <CurlImport projectId={projectId!} />
          </TabsContent>
          <TabsContent value="export" className="pt-4">
            <Exports projectId={projectId!} />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  )
}
