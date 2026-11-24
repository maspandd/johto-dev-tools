// Client for the management API. Every error comes back as the shared
// envelope `{ error: { code, message, details, request_id } }`.

import type {
  ArchiveImportResult,
  ArchivePreview,
  CorsConfig,
  CurlPreview,
  Dataset,
  DataSchema,
  Endpoint,
  EndpointDef,
  Environment,
  Id,
  Issue,
  LogFilter,
  MockServer,
  OpenApiImportResult,
  OpenApiPreview,
  Page,
  Project,
  RequestLog,
  Resource,
  SecretMeta,
  Workspace,
} from './types'

export interface ErrorDetail {
  location: string
  message: string
}

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly details: ErrorDetail[]
  readonly requestId?: string

  constructor(status: number, code: string, message: string, details: ErrorDetail[] = [], requestId?: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.details = details
    this.requestId = requestId
  }
}

// --- Connection settings (kept in this browser only) -----------------------

const SETTINGS_KEY = 'workbench.connection'

export interface Connection {
  /** Empty: same origin (the dev server proxies `/api`). */
  baseUrl: string
  token: string
}

/** Set by the desktop app, which runs the API in-process. */
interface DesktopConnection {
  baseUrl: string
  token: string
  desktop: true
}

declare global {
  interface Window {
    __WORKBENCH__?: DesktopConnection
  }
}

/** The desktop app's connection; settings cannot change it. */
export const desktopConnection: DesktopConnection | undefined =
  typeof window === 'undefined' ? undefined : window.__WORKBENCH__

export function loadConnection(): Connection {
  if (desktopConnection) return { baseUrl: desktopConnection.baseUrl, token: desktopConnection.token }
  try {
    const raw = localStorage.getItem(SETTINGS_KEY)
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<Connection>
      return { baseUrl: parsed.baseUrl ?? '', token: parsed.token ?? '' }
    }
  } catch {
    // Storage unavailable or corrupt: fall back to defaults.
  }
  return { baseUrl: '', token: '' }
}

export function saveConnection(c: Connection) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(c))
  } catch {
    // Ignore: settings then last for this page only.
  }
  connection = c
}

let connection = loadConnection()

function url(path: string): string {
  return `${connection.baseUrl.replace(/\/$/, '')}/api/v1${path}`
}

function query(params: Record<string, string | number | boolean | undefined | null>): string {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
  }
  const s = q.toString()
  return s ? `?${s}` : ''
}

interface RequestOptions {
  method?: string
  json?: unknown
  body?: BodyInit
  contentType?: string
}

async function send(path: string, opts: RequestOptions = {}): Promise<Response> {
  const headers: Record<string, string> = {}
  if (connection.token) headers.authorization = `Bearer ${connection.token}`
  let body = opts.body
  if (opts.json !== undefined) {
    headers['content-type'] = 'application/json'
    body = JSON.stringify(opts.json)
  } else if (opts.contentType) {
    headers['content-type'] = opts.contentType
  }
  let res: Response
  try {
    res = await fetch(url(path), { method: opts.method ?? 'GET', headers, body })
  } catch {
    throw new ApiError(
      0,
      'NETWORK_ERROR',
      'Cannot reach the Workbench API. Is `workbench-server` running?',
    )
  }
  if (!res.ok) {
    let envelope: { error?: { code?: string; message?: string; details?: ErrorDetail[]; request_id?: string } } = {}
    try {
      envelope = await res.json()
    } catch {
      // Not JSON (e.g. a proxy error page).
    }
    const e = envelope.error
    throw new ApiError(
      res.status,
      e?.code ?? 'HTTP_ERROR',
      e?.message ?? `The API answered ${res.status} ${res.statusText}.`,
      e?.details ?? [],
      e?.request_id ?? res.headers.get('x-request-id') ?? undefined,
    )
  }
  return res
}

async function json<T>(path: string, opts?: RequestOptions): Promise<T> {
  const res = await send(path, opts)
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

async function data<T>(path: string, opts?: RequestOptions): Promise<T> {
  return (await json<{ data: T }>(path, opts)).data
}

/** Fetches every page of a collection. */
async function all<T>(path: string): Promise<T[]> {
  const sep = path.includes('?') ? '&' : '?'
  const out: T[] = []
  for (let page = 1; ; page++) {
    const p = await json<Page<T>>(`${path}${sep}page=${page}&limit=200`)
    out.push(...p.data)
    if (page >= p.meta.total_pages) return out
  }
}

export interface FileDownload {
  blob: Blob
  filename: string
}

async function download(path: string): Promise<FileDownload> {
  const res = await send(path)
  const disposition = res.headers.get('content-disposition') ?? ''
  const filename = /filename="([^"]+)"/.exec(disposition)?.[1] ?? 'download'
  return { blob: await res.blob(), filename }
}

export const api = {
  health: async (): Promise<boolean> => {
    try {
      const res = await fetch(`${connection.baseUrl.replace(/\/$/, '')}/health`)
      return res.ok
    } catch {
      return false
    }
  },

  // Workspaces
  workspaces: (includeArchived = false) => all<Workspace>(`/workspaces${query({ include_archived: includeArchived })}`),
  workspace: (id: Id) => data<Workspace>(`/workspaces/${id}`),
  createWorkspace: (name: string) => data<Workspace>('/workspaces', { method: 'POST', json: { name } }),
  updateWorkspace: (id: Id, patch: { name?: string; archived?: boolean }) =>
    data<Workspace>(`/workspaces/${id}`, { method: 'PATCH', json: patch }),
  deleteWorkspace: (id: Id) => json<void>(`/workspaces/${id}`, { method: 'DELETE' }),

  // Projects
  projects: (workspaceId: Id, includeArchived = false) =>
    all<Project>(`/projects${query({ workspace_id: workspaceId, include_archived: includeArchived })}`),
  project: (id: Id) => data<Project>(`/projects/${id}`),
  createProject: (body: { workspace_id: Id; name: string; description?: string; base_path?: string | null }) =>
    data<Project>('/projects', { method: 'POST', json: body }),
  updateProject: (id: Id, patch: Record<string, unknown>) =>
    data<Project>(`/projects/${id}`, { method: 'PATCH', json: patch }),
  deleteProject: (id: Id) => json<void>(`/projects/${id}`, { method: 'DELETE' }),
  duplicateProject: (id: Id, name?: string) =>
    data<Project>(`/projects/${id}/duplicate`, { method: 'POST', json: name ? { name } : {} }),

  // Environments
  environments: (projectId: Id) => all<Environment>(`/projects/${projectId}/environments`),
  createEnvironment: (projectId: Id, body: Partial<Environment>) =>
    data<Environment>(`/projects/${projectId}/environments`, { method: 'POST', json: body }),
  updateEnvironment: (id: Id, patch: Record<string, unknown>) =>
    data<Environment>(`/environments/${id}`, { method: 'PATCH', json: patch }),
  deleteEnvironment: (id: Id) => json<void>(`/environments/${id}`, { method: 'DELETE' }),
  secrets: (environmentId: Id) => data<SecretMeta[]>(`/environments/${environmentId}/secrets`),
  putSecret: (environmentId: Id, name: string, value: string) =>
    json<void>(`/environments/${environmentId}/secrets/${encodeURIComponent(name)}`, {
      method: 'PUT',
      json: { value },
    }),
  deleteSecret: (environmentId: Id, name: string) =>
    json<void>(`/environments/${environmentId}/secrets/${encodeURIComponent(name)}`, { method: 'DELETE' }),

  // Endpoints
  endpoints: (projectId: Id) => all<Endpoint>(`/projects/${projectId}/endpoints`),
  endpoint: (id: Id) => data<Endpoint>(`/endpoints/${id}`),
  createEndpoint: (projectId: Id, body: Partial<EndpointDef>) =>
    data<Endpoint>(`/projects/${projectId}/endpoints`, { method: 'POST', json: body }),
  /** Saves the whole definition (keeps `null` values in bodies and schemas). */
  replaceEndpoint: (id: Id, def: EndpointDef) => data<Endpoint>(`/endpoints/${id}`, { method: 'PUT', json: def }),
  deleteEndpoint: (id: Id) => json<void>(`/endpoints/${id}`, { method: 'DELETE' }),
  duplicateEndpoint: (id: Id) => data<Endpoint>(`/endpoints/${id}/duplicate`, { method: 'POST' }),
  setEndpointEnabled: (id: Id, enabled: boolean) =>
    data<Endpoint>(`/endpoints/${id}/${enabled ? 'enable' : 'disable'}`, { method: 'POST' }),
  reorderEndpoints: (projectId: Id, order: Id[]) =>
    json<void>(`/projects/${projectId}/endpoints/reorder`, { method: 'POST', json: { order } }),

  // Resources
  resources: (projectId: Id) => all<Resource>(`/projects/${projectId}/resources`),
  createResource: (projectId: Id, body: Record<string, unknown>) =>
    data<Resource>(`/projects/${projectId}/resources`, { method: 'POST', json: body }),
  replaceResource: (id: Id, def: Record<string, unknown>) =>
    data<Resource>(`/resources/${id}`, { method: 'PUT', json: def }),
  deleteResource: (id: Id) => json<void>(`/resources/${id}`, { method: 'DELETE' }),
  resourceRecords: (id: Id, page = 1, limit = 50) =>
    json<Page<Record<string, unknown>>>(`/resources/${id}/records${query({ page, limit })}`),
  resetResource: (id: Id) => json<void>(`/resources/${id}/reset`, { method: 'POST' }),

  // Datasets
  datasets: (projectId: Id) => all<Dataset>(`/projects/${projectId}/datasets`),
  createDataset: (projectId: Id, body: Record<string, unknown>) =>
    data<Dataset>(`/projects/${projectId}/datasets`, { method: 'POST', json: body }),
  replaceDataset: (id: Id, def: Record<string, unknown>) =>
    data<Dataset>(`/datasets/${id}`, { method: 'PUT', json: def }),
  deleteDataset: (id: Id) => json<void>(`/datasets/${id}`, { method: 'DELETE' }),
  previewSchema: (projectId: Id, body: { schema: DataSchema; count?: number; seed?: number; locale?: string }) =>
    data<Record<string, unknown>[]>(`/projects/${projectId}/datasets/preview`, { method: 'POST', json: body }),
  exportDataset: (id: Id, format: 'json' | 'csv') => download(`/datasets/${id}/export${query({ format })}`),

  // Mock servers and logs
  servers: (projectId: Id) => data<MockServer[]>(`/projects/${projectId}/mock-servers`),
  startServer: (
    projectId: Id,
    body: { port?: number; environment_id?: Id | null; cors?: CorsConfig },
  ) => data<MockServer>(`/projects/${projectId}/mock-servers`, { method: 'POST', json: body }),
  stopServer: (id: Id) => json<void>(`/mock-servers/${id}`, { method: 'DELETE' }),
  reloadServer: (id: Id) => data<MockServer>(`/mock-servers/${id}`, { method: 'PATCH', json: {} }),
  logs: (projectId: Id, filter: LogFilter = {}) =>
    data<RequestLog[]>(`/projects/${projectId}/request-logs${query({ ...filter })}`),
  clearLogs: (projectId: Id) =>
    data<{ deleted: number }>(`/projects/${projectId}/request-logs`, { method: 'DELETE' }),

  // Import / export
  validateOpenApi: (text: string) =>
    data<{ valid: boolean; operations: number; importable: number; issues: Issue[] }>('/openapi/validate', {
      method: 'POST',
      body: text,
      contentType: 'text/plain',
    }),
  previewOpenApi: (projectId: Id, document: string) =>
    data<OpenApiPreview>(`/projects/${projectId}/openapi/import`, {
      method: 'POST',
      json: { document, dry_run: true },
    }),
  importOpenApi: (projectId: Id, document: string, operations: string[], setBasePath: boolean) =>
    data<OpenApiImportResult>(`/projects/${projectId}/openapi/import`, {
      method: 'POST',
      json: { document, operations, set_base_path: setBasePath },
    }),
  exportOpenApi: (projectId: Id, format: 'yaml' | 'json') =>
    download(`/projects/${projectId}/openapi/export${query({ format })}`),
  parseCurl: (command: string, projectId?: Id, keepCredentials = false) =>
    data<CurlPreview>('/curl/parse', {
      method: 'POST',
      json: { command, project_id: projectId, keep_credentials: keepCredentials },
    }),
  exportArchive: (projectId: Id, records: boolean) => download(`/projects/${projectId}/export${query({ records })}`),
  previewArchive: (workspaceId: Id, file: Blob) =>
    data<ArchivePreview>(`/projects/import${query({ workspace_id: workspaceId, preview: true })}`, {
      method: 'POST',
      body: file,
      contentType: 'application/zip',
    }),
  importArchive: (workspaceId: Id, file: Blob, name?: string) =>
    data<ArchiveImportResult>(`/projects/import${query({ workspace_id: workspaceId, name })}`, {
      method: 'POST',
      body: file,
      contentType: 'application/zip',
    }),
}

/** Saves a downloaded file through the browser. */
export function saveFile({ blob, filename }: FileDownload) {
  const href = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = href
  a.download = filename
  document.body.append(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(href), 1000)
}

/** Human-readable error text, including field details. */
export function describeError(e: unknown): { title: string; details: ErrorDetail[] } {
  if (e instanceof ApiError) return { title: e.message, details: e.details }
  if (e instanceof Error) return { title: e.message, details: [] }
  return { title: 'Something went wrong.', details: [] }
}
