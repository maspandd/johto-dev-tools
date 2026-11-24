import { AlertTriangle, Info, Loader2, XCircle } from 'lucide-react'
import type * as React from 'react'

import { describeError } from '@/lib/api'
import type { Issue, Method } from '@/lib/types'
import { cn, statusTone } from '@/lib/utils'

const TONES = {
  default: 'bg-secondary text-secondary-foreground',
  success: 'bg-success/15 text-success',
  warning: 'bg-warning/20 text-amber-700 dark:text-amber-300',
  destructive: 'bg-destructive/15 text-destructive',
  muted: 'bg-muted text-muted-foreground',
  primary: 'bg-primary/15 text-primary',
} as const

export function Badge({
  tone = 'default',
  className,
  ...props
}: React.ComponentProps<'span'> & { tone?: keyof typeof TONES }) {
  return (
    <span
      className={cn('inline-flex items-center rounded px-1.5 py-px text-[11px] font-medium', TONES[tone], className)}
      {...props}
    />
  )
}

const METHOD_COLORS: Record<string, string> = {
  GET: 'text-emerald-600 dark:text-emerald-400',
  POST: 'text-sky-600 dark:text-sky-400',
  PUT: 'text-amber-600 dark:text-amber-400',
  PATCH: 'text-violet-600 dark:text-violet-400',
  DELETE: 'text-rose-600 dark:text-rose-400',
}

export function MethodBadge({ method, className }: { method: Method | string; className?: string }) {
  return (
    <span className={cn('inline-block w-12 shrink-0 font-mono text-[11px] font-bold', METHOD_COLORS[method], className)}>
      {method === 'DELETE' ? 'DEL' : method}
    </span>
  )
}

export function StatusBadge({ status }: { status: number }) {
  return (
    <Badge tone={statusTone(status)} className="font-mono">
      {status}
    </Badge>
  )
}

export function Spinner({ className }: { className?: string }) {
  return <Loader2 className={cn('size-4 animate-spin text-muted-foreground', className)} aria-label="Loading" />
}

export function EmptyState({
  icon,
  title,
  children,
  action,
  className,
}: {
  icon?: React.ReactNode
  title: string
  children?: React.ReactNode
  action?: React.ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex flex-col items-center justify-center gap-2 p-8 text-center', className)}>
      {icon && <div className="mb-1 text-muted-foreground [&_svg]:size-8">{icon}</div>}
      <h3 className="text-sm font-semibold">{title}</h3>
      {children && <div className="max-w-sm text-sm text-muted-foreground">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  )
}

/** An API error with every field problem listed. */
export function ErrorNotice({ error, className }: { error: unknown; className?: string }) {
  if (!error) return null
  const { title, details } = describeError(error)
  return (
    <div role="alert" className={cn('rounded-md border border-destructive/40 bg-destructive/10 p-3 text-sm', className)}>
      <div className="flex items-start gap-2 font-medium text-destructive">
        <XCircle className="mt-0.5 size-4 shrink-0" />
        <span>{title}</span>
      </div>
      {details.length > 0 && (
        <ul className="mt-1.5 ml-6 list-disc space-y-0.5 text-xs">
          {details.map((d, i) => (
            <li key={i}>
              {d.location && <code className="mr-1 font-mono text-muted-foreground">{d.location}</code>}
              {d.message}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

export function IssueList({ issues, className }: { issues: Issue[]; className?: string }) {
  if (!issues.length) return null
  return (
    <ul className={cn('space-y-1 text-xs', className)}>
      {issues.map((i, n) => (
        <li key={n} className="flex items-start gap-1.5">
          {i.severity === 'error' ? (
            <XCircle className="mt-px size-3.5 shrink-0 text-destructive" />
          ) : i.severity === 'warning' ? (
            <AlertTriangle className="mt-px size-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          ) : (
            <Info className="mt-px size-3.5 shrink-0 text-muted-foreground" />
          )}
          <span>
            {i.message}
            {i.pointer && <code className="ml-1 font-mono text-[10px] text-muted-foreground">{i.pointer}</code>}
          </span>
        </li>
      ))}
    </ul>
  )
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="rounded border bg-muted px-1 font-mono text-[10px] text-muted-foreground">{children}</kbd>
  )
}

export function SectionTitle({ children, actions }: { children: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2">
      <h3 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{children}</h3>
      {actions}
    </div>
  )
}
