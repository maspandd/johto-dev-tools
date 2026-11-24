import { QueryClient, useMutation, useQuery, useQueryClient, type QueryKey } from '@tanstack/react-query'
import { useCallback, useSyncExternalStore } from 'react'
import { toast } from 'sonner'

import { api, ApiError, describeError } from './api'
import type { Id } from './types'

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: (count, error) => !(error instanceof ApiError && error.status > 0 && error.status < 500) && count < 2,
      refetchOnWindowFocus: true,
    },
  },
})

export const keys = {
  workspaces: ['workspaces'] as const,
  projects: (ws: Id) => ['projects', ws] as const,
  project: (id: Id) => ['project', id] as const,
  endpoints: (p: Id) => ['endpoints', p] as const,
  resources: (p: Id) => ['resources', p] as const,
  records: (r: Id) => ['records', r] as const,
  datasets: (p: Id) => ['datasets', p] as const,
  environments: (p: Id) => ['environments', p] as const,
  secrets: (env: Id) => ['secrets', env] as const,
  servers: (p: Id) => ['servers', p] as const,
  logs: (p: Id) => ['logs', p] as const,
}

export const useWorkspaces = () => useQuery({ queryKey: keys.workspaces, queryFn: () => api.workspaces() })
export const useProjects = (ws: Id | undefined) =>
  useQuery({ queryKey: keys.projects(ws ?? ''), queryFn: () => api.projects(ws!), enabled: !!ws })
export const useProject = (id: Id | undefined) =>
  useQuery({ queryKey: keys.project(id ?? ''), queryFn: () => api.project(id!), enabled: !!id })
export const useEndpoints = (p: Id | undefined) =>
  useQuery({ queryKey: keys.endpoints(p ?? ''), queryFn: () => api.endpoints(p!), enabled: !!p })
export const useResources = (p: Id | undefined) =>
  useQuery({ queryKey: keys.resources(p ?? ''), queryFn: () => api.resources(p!), enabled: !!p })
export const useDatasets = (p: Id | undefined) =>
  useQuery({ queryKey: keys.datasets(p ?? ''), queryFn: () => api.datasets(p!), enabled: !!p })
export const useEnvironments = (p: Id | undefined) =>
  useQuery({ queryKey: keys.environments(p ?? ''), queryFn: () => api.environments(p!), enabled: !!p })
export const useServers = (p: Id | undefined) =>
  useQuery({
    queryKey: keys.servers(p ?? ''),
    queryFn: () => api.servers(p!),
    enabled: !!p,
    refetchInterval: 5_000,
  })

/**
 * A mutation that shows its error as a toast and invalidates `invalidate`
 * on success.
 */
export function useAction<TArgs, TResult>(
  fn: (args: TArgs) => Promise<TResult>,
  opts: { invalidate?: QueryKey[]; success?: string | ((r: TResult) => string); silentError?: boolean } = {},
) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: fn,
    onSuccess: (result) => {
      for (const key of opts.invalidate ?? []) void qc.invalidateQueries({ queryKey: key })
      const msg = typeof opts.success === 'function' ? opts.success(result) : opts.success
      if (msg) toast.success(msg)
    },
    onError: (e) => {
      if (opts.silentError) return
      const { title, details } = describeError(e)
      toast.error(title, {
        description: details.length ? details.map((d) => `${d.location}: ${d.message}`).join('\n') : undefined,
      })
    },
  })
}

// --- Small persisted UI state ----------------------------------------------

const listeners = new Set<() => void>()

function read(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

/** A string kept in localStorage and shared by every component using it. */
export function useStoredString(key: string, fallback: string): [string, (v: string) => void] {
  const value = useSyncExternalStore(
    (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
    () => read(key) ?? fallback,
  )
  const set = useCallback(
    (v: string) => {
      try {
        localStorage.setItem(key, v)
      } catch {
        // Ignore: value lasts until reload.
      }
      listeners.forEach((l) => l())
    },
    [key],
  )
  return [value, set]
}

/** The environment chosen for a project's mock servers (`''` = none). */
export const useSelectedEnvironment = (projectId: Id) => useStoredString(`workbench.environment.${projectId}`, '')
