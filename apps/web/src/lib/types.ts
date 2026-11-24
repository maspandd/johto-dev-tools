// Shapes returned by the management API (`/api/v1`). They mirror the Rust
// domain types in `crates/workbench-domain` as serialized by serde.

export type Id = string
export type Timestamp = string

export type Method = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE'
export const METHODS: Method[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE']

export interface Page<T> {
  data: T[]
  meta: { page: number; limit: number; total: number; total_pages: number }
}

export interface Workspace {
  id: Id
  name: string
  archived_at: Timestamp | null
  created_at: Timestamp
  updated_at: Timestamp
}

export interface Project {
  id: Id
  workspace_id: Id
  name: string
  description: string
  base_path: string | null
  default_headers: Record<string, string>
  format_version: number
  archived_at: Timestamp | null
  created_at: Timestamp
  updated_at: Timestamp
}

export interface BindAddress {
  host: string
  port: number
  allow_network_exposure: boolean
}

export interface Environment {
  id: Id
  project_id: Id
  name: string
  base_url: string | null
  default_headers: Record<string, string>
  variables: Record<string, string>
  bind: BindAddress
  created_at: Timestamp
  updated_at: Timestamp
}

export interface SecretMeta {
  name: string
  created_at: Timestamp
  updated_at: Timestamp
}

export interface ParamSpec {
  name: string
  required: boolean
  description: string
}

export interface RequestSpec {
  headers: ParamSpec[]
  query: ParamSpec[]
  body_schema?: unknown
  body_example?: unknown
}

export type ResponseBody =
  | { type: 'empty' }
  | { type: 'json'; content: unknown }
  | { type: 'text'; content: string }

export interface MockResponse {
  status: number
  headers: Record<string, string>
  body: ResponseBody
  delay_ms: number
}

export type ValueSource =
  | { from: 'header'; name: string }
  | { from: 'query'; name: string }
  | { from: 'path'; name: string }
  | { from: 'body'; pointer: string }

export type Operator = 'equals' | 'not_equals' | 'contains' | 'exists' | 'absent'
export const OPERATORS: Operator[] = ['equals', 'not_equals', 'contains', 'exists', 'absent']

export interface Condition {
  source: ValueSource
  operator: Operator
  value?: string
}

export interface ResponseRule {
  name: string
  when: Condition[]
  response: MockResponse
}

export type AuthSimulation = { type: 'none' } | { type: 'bearer' } | { type: 'api_key'; header: string }

/** An endpoint definition (what the editor changes). */
export interface EndpointDef {
  id: Id
  name: string
  description: string
  method: Method
  path: string
  enabled: boolean
  request: RequestSpec
  response: MockResponse
  rules: ResponseRule[]
  auth: AuthSimulation
}

/** An endpoint as listed by the API, with ordering metadata. */
export interface Endpoint extends EndpointDef {
  project_id: Id
  position: number
  created_at: Timestamp
  updated_at: Timestamp
}

export type Locale = 'en' | 'id'

export interface FieldSpec {
  name: string
  type: string
  null_rate?: number
  unique?: boolean
  [option: string]: unknown
}

export interface DataSchema {
  fields: FieldSpec[]
}

export interface Resource {
  id: Id
  project_id: Id
  name: string
  path: string
  enabled: boolean
  primary_key: string
  key_strategy: 'uuid' | 'increment'
  schema: DataSchema
  seed?: { count: number; seed: number; locale: Locale }
  body_schema?: unknown
  pagination: { default_limit: number; max_limit: number }
  searchable: string[]
  delete: 'hard' | 'soft'
  data_initialized: boolean
  created_at: Timestamp
  updated_at: Timestamp
}

export interface Dataset {
  id: Id
  project_id: Id
  name: string
  description: string
  schema: DataSchema
  count: number
  seed: number
  locale: Locale
  created_at: Timestamp
  updated_at: Timestamp
}

export interface CorsConfig {
  enabled: boolean
  allowed_origins: string[]
}

export interface MockServer {
  id: Id
  project_id: Id
  environment_id: Id | null
  status: string
  url: string
  host: string
  port: number
  base_path: string | null
  endpoints: number
  resources: number
  started_at: Timestamp
  cors: CorsConfig
  last_reload_error: string | null
}

export interface CapturedBody {
  content: string
  size: number
  truncated: boolean
}

export interface RequestLog {
  id: Id
  server_id: Id
  timestamp: Timestamp
  method: string
  path: string
  query: [string, string][]
  request_headers: [string, string][]
  request_body: CapturedBody | null
  matched_endpoint: Id | null
  matched_rule: string | null
  matched_resource: Id | null
  status: number
  duration_ms: number
  response_headers: [string, string][]
  response_body: CapturedBody | null
}

export interface LogFilter {
  method?: string
  status?: number
  endpoint_id?: Id
  resource_id?: Id
  server_id?: Id
  limit?: number
}

// --- Import / export -------------------------------------------------------

export interface Issue {
  severity: 'error' | 'warning' | 'info'
  pointer: string
  message: string
}

export interface OpenApiOperation {
  key: string
  method: string
  path: string
  operation_id?: string
  name: string
  endpoint: EndpointDef | null
  issues: Issue[]
  importable: boolean
  conflicts_with_project: boolean
}

export interface OpenApiPreview {
  openapi: string
  title: string
  description: string
  version: string
  base_path: string | null
  operations: OpenApiOperation[]
  issues: Issue[]
}

export interface OpenApiImportResult {
  imported: Endpoint[]
  disabled_due_to_conflicts: string[]
  base_path_set: string | null
  issues: Issue[]
}

export interface NameValue {
  name: string
  value: string
}

export interface CurlPreview {
  request: {
    method: Method
    url: string
    scheme: string
    host: string
    port?: number
    path: string
    query: NameValue[]
    headers: NameValue[]
    body?: { type: 'json'; content: unknown } | { type: 'text'; content: string }
    redacted: string[]
    warnings: string[]
  }
  endpoint: EndpointDef | null
  endpoint_error?: string
}

export interface ArchiveManifest {
  format: string
  format_version: number
  generator: string
  exported_at: Timestamp
  project_name: string
  contents: { environments: number; endpoints: number; resources: number; datasets: number; records: number }
  secrets_included: boolean
}

export interface ArchivePreview {
  manifest: ArchiveManifest
  project: { name: string; description: string; base_path: string | null }
  environments: string[]
  endpoints: { name: string; method: Method; path: string; enabled: boolean }[]
  resources: { name: string; path: string; records: number | null }[]
  datasets: string[]
  secrets_to_set: { environment: string; names: string[] }[]
  warnings: string[]
}

export interface ArchiveImportResult {
  project: Project
  secrets_to_set: { environment: string; names: string[] }[]
  warnings: string[]
}
