import { lazy, Suspense } from 'react'

import { Spinner } from '@/components/ui/misc'
import { parseJson } from '@/lib/utils'

import type { CodeEditorProps } from './CodeEditor'

const LazyEditor = lazy(() => import('./CodeEditor'))

/** Monaco, loaded on first use. */
export function CodeEditor(props: CodeEditorProps) {
  return (
    <Suspense
      fallback={
        <div
          className="flex items-center justify-center rounded-md border bg-muted/40"
          style={{ height: props.height ?? '100%' }}
        >
          <Spinner />
        </div>
      }
    >
      <LazyEditor {...props} />
    </Suspense>
  )
}

/**
 * A JSON editor bound to a value. Keeps the text while it is invalid and
 * reports the parse error; calls `onValue` only with valid JSON
 * (`undefined` for empty text when `allowEmpty`).
 */
export function JsonField({
  text,
  onText,
  error,
  height = 200,
  schema,
  ariaLabel,
  onSave,
  readOnly,
}: {
  text: string
  onText: (text: string, parsed: ReturnType<typeof parseJson>) => void
  error?: string
  height?: number | string
  schema?: object
  ariaLabel?: string
  onSave?: () => void
  readOnly?: boolean
}) {
  return (
    <div className="flex flex-col gap-1">
      <CodeEditor
        value={text}
        onChange={(t) => onText(t, parseJson(t))}
        height={height}
        schema={schema}
        ariaLabel={ariaLabel}
        onSave={onSave}
        readOnly={readOnly}
      />
      {error && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </div>
  )
}

