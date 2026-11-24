import { HelpCircle } from 'lucide-react'

import { CodeEditor } from '@/components/editor'
import { JsonValue, KeyValueEditor } from '@/components/fields'
import { Field, Input, Label } from '@/components/ui/form'
import { Tooltip } from '@/components/ui/overlay'
import type { MockResponse, ResponseBody } from '@/lib/types'
import { cn } from '@/lib/utils'

const COMMON_STATUSES = [200, 201, 202, 204, 301, 304, 400, 401, 403, 404, 409, 422, 429, 500, 502, 503]

const TEMPLATE_HELP = [
  ['{{ path.id }}', 'path parameter'],
  ['{{ query.page }}', 'query parameter'],
  ['{{ header.x-tenant }}', 'request header'],
  ['{{ body.user.name }}', 'request body field'],
  ['{{ env.NAME }}', 'environment variable'],
  ['{{ uuid }}', 'random UUID'],
  ['{{ now }}', 'current time (RFC 3339)'],
  ['{{ timestamp }}', 'Unix seconds'],
  ['{{ request.id }}', 'request ID'],
]

export function TemplateHelp() {
  return (
    <Tooltip
      content={
        <div className="flex flex-col gap-0.5 py-0.5">
          <span className="font-semibold">Placeholders in bodies and headers</span>
          {TEMPLATE_HELP.map(([t, d]) => (
            <span key={t}>
              <code className="font-mono">{t}</code> — {d}
            </span>
          ))}
        </div>
      }
    >
      <button type="button" className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
        <HelpCircle className="size-3.5" /> Placeholders
      </button>
    </Tooltip>
  )
}

const BODY_TYPES: { type: ResponseBody['type']; label: string }[] = [
  { type: 'json', label: 'JSON' },
  { type: 'text', label: 'Text' },
  { type: 'empty', label: 'Empty' },
]

export function ResponseEditor({
  value,
  onChange,
  label,
  bodyHeight = 260,
  onSave,
}: {
  value: MockResponse
  onChange: (v: MockResponse) => void
  label: string
  bodyHeight?: number
  onSave?: () => void
}) {
  const set = (patch: Partial<MockResponse>) => onChange({ ...value, ...patch })
  const setType = (type: ResponseBody['type']) => {
    if (type === value.body.type) return
    if (type === 'empty') set({ body: { type: 'empty' } })
    else if (type === 'json')
      set({
        body: {
          type: 'json',
          content: value.body.type === 'text' ? tryParse(value.body.content) : {},
        },
      })
    else
      set({
        body: {
          type: 'text',
          content: value.body.type === 'json' ? JSON.stringify(value.body.content, null, 2) : '',
        },
      })
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Status" className="w-28">
          {(id) => (
            <>
              <Input
                id={id}
                type="number"
                min={100}
                max={599}
                list={`${id}-statuses`}
                className="font-mono"
                value={value.status}
                onChange={(e) => set({ status: Number.parseInt(e.target.value, 10) || 0 })}
                aria-invalid={value.status < 100 || value.status > 599}
              />
              <datalist id={`${id}-statuses`}>
                {COMMON_STATUSES.map((s) => (
                  <option key={s} value={s} />
                ))}
              </datalist>
            </>
          )}
        </Field>
        <Field label="Delay (ms)" className="w-28">
          {(id) => (
            <Input
              id={id}
              type="number"
              min={0}
              max={30000}
              step={100}
              value={value.delay_ms}
              onChange={(e) => set({ delay_ms: Math.max(0, Number.parseInt(e.target.value, 10) || 0) })}
            />
          )}
        </Field>
        <div className="flex flex-col gap-1">
          <Label>Body</Label>
          <div role="radiogroup" aria-label={`${label} body type`} className="flex rounded-md border p-0.5">
            {BODY_TYPES.map((b) => (
              <button
                key={b.type}
                type="button"
                role="radio"
                aria-checked={value.body.type === b.type}
                onClick={() => setType(b.type)}
                className={cn(
                  'rounded px-2.5 py-1 text-xs',
                  value.body.type === b.type ? 'bg-accent font-medium text-accent-foreground' : 'text-muted-foreground hover:text-foreground',
                )}
              >
                {b.label}
              </button>
            ))}
          </div>
        </div>
        <div className="ml-auto pb-1.5">
          <TemplateHelp />
        </div>
      </div>

      {value.body.type === 'json' && (
        <JsonValue
          label={`${label} body`}
          value={value.body.content}
          height={bodyHeight}
          onSave={onSave}
          onChange={(content) => set({ body: { type: 'json', content } })}
        />
      )}
      {value.body.type === 'text' && (
        <CodeEditor
          language="plaintext"
          height={bodyHeight}
          ariaLabel={`${label} text body`}
          value={value.body.content}
          onSave={onSave}
          onChange={(content) => set({ body: { type: 'text', content } })}
        />
      )}

      <div className="flex flex-col gap-1.5">
        <Label>Headers</Label>
        <KeyValueEditor
          value={value.headers}
          onChange={(headers) => set({ headers })}
          keyPlaceholder="Header"
          addLabel="Add header"
        />
      </div>
    </div>
  )
}

function tryParse(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return {}
  }
}
