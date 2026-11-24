import { Plus, X } from 'lucide-react'
import { useContext, useEffect, useId, useRef, useState } from 'react'

import { JsonField } from '@/components/editor'
import { Button } from '@/components/ui/button'
import { Checkbox, Input } from '@/components/ui/form'
import { InvalidFields } from '@/lib/invalidFields'
import type { ParamSpec } from '@/lib/types'
import { formatJson } from '@/lib/utils'

/**
 * Edits a JSON value. Keeps the text while it does not parse and reports
 * the problem; `onChange` only ever receives valid values (`undefined` for
 * empty text when `allowEmpty`).
 */
export function JsonValue({
  value,
  onChange,
  allowEmpty = false,
  height = 180,
  label,
  onSave,
  schema,
}: {
  value: unknown
  onChange: (v: unknown) => void
  allowEmpty?: boolean
  height?: number | string
  label: string
  onSave?: () => void
  /** JSON Schema for completion and checks in the editor. */
  schema?: object
}) {
  const key = useId()
  const report = useContext(InvalidFields)
  const [text, setText] = useState(() => formatJson(value))
  const [error, setError] = useState<string | undefined>()
  const last = useRef(value)

  // Follow outside changes (e.g. a reload) without clobbering typing.
  useEffect(() => {
    if (JSON.stringify(value) !== JSON.stringify(last.current)) {
      last.current = value
      setText(formatJson(value))
      setError(undefined)
      report(key, null)
    }
  }, [value, key, report])

  useEffect(() => () => report(key, null), [key, report])

  return (
    <JsonField
      text={text}
      height={height}
      ariaLabel={label}
      error={error}
      onSave={onSave}
      schema={schema}
      onText={(t, parsed) => {
        setText(t)
        if (!parsed.ok) {
          setError(parsed.error)
          report(key, `${label}: ${parsed.error}`)
          return
        }
        if (parsed.value === undefined && !allowEmpty) {
          setError('Enter a JSON value')
          report(key, `${label}: empty`)
          return
        }
        setError(undefined)
        report(key, null)
        last.current = parsed.value
        onChange(parsed.value)
      }}
    />
  )
}


// --- Key/value maps ----------------------------------------------------------

type Row = { id: number; key: string; value: string }
let rowId = 0

/** Edits a string map such as response headers or environment variables. */
export function KeyValueEditor({
  value,
  onChange,
  keyPlaceholder = 'Name',
  valuePlaceholder = 'Value',
  addLabel = 'Add',
  mono = true,
}: {
  value: Record<string, string>
  onChange: (v: Record<string, string>) => void
  keyPlaceholder?: string
  valuePlaceholder?: string
  addLabel?: string
  mono?: boolean
}) {
  const toRows = (v: Record<string, string>): Row[] =>
    Object.entries(v).map(([key, val]) => ({ id: ++rowId, key, value: val }))
  const [rows, setRows] = useState<Row[]>(() => toRows(value))
  const emitted = useRef(value)

  useEffect(() => {
    if (JSON.stringify(value) !== JSON.stringify(emitted.current)) {
      emitted.current = value
      setRows(toRows(value))
    }
  }, [value])

  const emit = (next: Row[]) => {
    setRows(next)
    const map: Record<string, string> = {}
    for (const r of next) if (r.key.trim()) map[r.key.trim()] = r.value
    emitted.current = map
    onChange(map)
  }

  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((r, i) => (
        <div key={r.id} className="flex items-center gap-1.5">
          <Input
            aria-label={`${keyPlaceholder} ${i + 1}`}
            placeholder={keyPlaceholder}
            className={mono ? 'font-mono text-xs' : ''}
            value={r.key}
            onChange={(e) => emit(rows.map((x) => (x.id === r.id ? { ...x, key: e.target.value } : x)))}
          />
          <Input
            aria-label={`${valuePlaceholder} ${i + 1}`}
            placeholder={valuePlaceholder}
            className={mono ? 'font-mono text-xs' : ''}
            value={r.value}
            onChange={(e) => emit(rows.map((x) => (x.id === r.id ? { ...x, value: e.target.value } : x)))}
          />
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove ${r.key || 'row'}`}
            onClick={() => emit(rows.filter((x) => x.id !== r.id))}
          >
            <X />
          </Button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        className="self-start text-muted-foreground"
        onClick={() => setRows([...rows, { id: ++rowId, key: '', value: '' }])}
      >
        <Plus /> {addLabel}
      </Button>
    </div>
  )
}

/** Editable ordered pairs (duplicate names allowed), for the request client. */
export function PairsEditor({
  value,
  onChange,
  keyPlaceholder = 'Name',
}: {
  value: [string, string][]
  onChange: (v: [string, string][]) => void
  keyPlaceholder?: string
}) {
  return (
    <div className="flex flex-col gap-1.5">
      {value.map(([k, v], i) => (
        <div key={i} className="flex items-center gap-1.5">
          <Input
            aria-label={`${keyPlaceholder} ${i + 1}`}
            placeholder={keyPlaceholder}
            className="font-mono text-xs"
            value={k}
            onChange={(e) => onChange(value.map((p, j) => (j === i ? [e.target.value, p[1]] : p)))}
          />
          <Input
            aria-label={`Value ${i + 1}`}
            placeholder="Value"
            className="font-mono text-xs"
            value={v}
            onChange={(e) => onChange(value.map((p, j) => (j === i ? [p[0], e.target.value] : p)))}
          />
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove ${k || 'row'}`}
            onClick={() => onChange(value.filter((_, j) => j !== i))}
          >
            <X />
          </Button>
        </div>
      ))}
      <Button variant="ghost" size="sm" className="self-start text-muted-foreground" onClick={() => onChange([...value, ['', '']])}>
        <Plus /> Add
      </Button>
    </div>
  )
}

// --- Parameter lists ---------------------------------------------------------

export function ParamListEditor({
  value,
  onChange,
  kind,
}: {
  value: ParamSpec[]
  onChange: (v: ParamSpec[]) => void
  kind: 'query parameter' | 'header'
}) {
  const set = (i: number, patch: Partial<ParamSpec>) => onChange(value.map((p, j) => (j === i ? { ...p, ...patch } : p)))
  return (
    <div className="flex flex-col gap-1.5">
      {value.length > 0 && (
        <div className="grid grid-cols-[1fr_auto_1.5fr_auto] items-center gap-1.5 text-[11px] text-muted-foreground">
          <span>Name</span>
          <span>Required</span>
          <span>Description</span>
          <span />
        </div>
      )}
      {value.map((p, i) => (
        <div key={i} className="grid grid-cols-[1fr_auto_1.5fr_auto] items-center gap-1.5">
          <Input
            aria-label={`${kind} ${i + 1} name`}
            className="font-mono text-xs"
            value={p.name}
            onChange={(e) => set(i, { name: e.target.value })}
          />
          <div className="flex justify-center">
            <Checkbox
              aria-label={`${kind} ${p.name || i + 1} required`}
              checked={p.required}
              onChange={(e) => set(i, { required: e.target.checked })}
            />
          </div>
          <Input
            aria-label={`${kind} ${i + 1} description`}
            value={p.description}
            onChange={(e) => set(i, { description: e.target.value })}
          />
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={`Remove ${p.name || kind}`}
            onClick={() => onChange(value.filter((_, j) => j !== i))}
          >
            <X />
          </Button>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        className="self-start text-muted-foreground"
        onClick={() => onChange([...value, { name: '', required: false, description: '' }])}
      >
        <Plus /> Add {kind}
      </Button>
    </div>
  )
}
