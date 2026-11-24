import { type ClassValue, clsx } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

export function formatJson(value: unknown): string {
  return value === undefined ? '' : JSON.stringify(value, null, 2)
}

/** Parses JSON text; empty text is `undefined`. */
export function parseJson(text: string): { ok: true; value: unknown } | { ok: false; error: string } {
  if (!text.trim()) return { ok: true, value: undefined }
  try {
    return { ok: true, value: JSON.parse(text) }
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : 'Invalid JSON' }
  }
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

export function formatTime(iso: string): string {
  const d = new Date(iso)
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

export function relativeTime(iso: string): string {
  const seconds = Math.round((Date.now() - new Date(iso).getTime()) / 1000)
  if (seconds < 60) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours} h ago`
  return new Date(iso).toLocaleDateString()
}

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
export const modKey = isMac ? '⌘' : 'Ctrl'

/** Ctrl on Windows/Linux, Cmd on macOS. */
export function isMod(e: KeyboardEvent | React.KeyboardEvent): boolean {
  return isMac ? e.metaKey : e.ctrlKey
}

export function statusTone(status: number): 'success' | 'warning' | 'destructive' | 'muted' {
  if (status >= 500) return 'destructive'
  if (status >= 400) return 'warning'
  if (status >= 200 && status < 400) return 'success'
  return 'muted'
}

/** Path parameter names in `/users/:id` or `/users/{id}`. */
export function pathParams(path: string): string[] {
  return path
    .split('/')
    .map((s) => (s.startsWith(':') ? s.slice(1) : s.startsWith('{') && s.endsWith('}') ? s.slice(1, -1) : null))
    .filter((s): s is string => !!s)
}
