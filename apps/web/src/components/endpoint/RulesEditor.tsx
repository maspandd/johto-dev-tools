import { ArrowDown, ArrowUp, Plus, Trash2, X } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Input, Select } from '@/components/ui/form'
import { EmptyState } from '@/components/ui/misc'
import { type Condition, type MockResponse, OPERATORS, type Operator, type ResponseRule, type ValueSource } from '@/lib/types'

import { ResponseEditor } from './ResponseEditor'

const SOURCES: ValueSource['from'][] = ['header', 'query', 'path', 'body']
const OPERATOR_LABELS: Record<Operator, string> = {
  equals: 'equals',
  not_equals: 'does not equal',
  contains: 'contains',
  exists: 'exists',
  absent: 'is absent',
}
const needsValue = (op: Operator) => op === 'equals' || op === 'not_equals' || op === 'contains'

function sourceName(s: ValueSource): string {
  return s.from === 'body' ? s.pointer : s.name
}

function withName(from: ValueSource['from'], name: string): ValueSource {
  return from === 'body' ? { from, pointer: name } : { from, name }
}

function ConditionRow({
  value,
  onChange,
  onRemove,
  pathParams,
  index,
}: {
  value: Condition
  onChange: (c: Condition) => void
  onRemove: () => void
  pathParams: string[]
  index: number
}) {
  const listId = `path-params-${index}`
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <Select
        aria-label="Source"
        className="w-24"
        value={value.source.from}
        onChange={(e) => onChange({ ...value, source: withName(e.target.value as ValueSource['from'], sourceName(value.source)) })}
      >
        {SOURCES.map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </Select>
      <Input
        aria-label={value.source.from === 'body' ? 'JSON pointer' : 'Name'}
        placeholder={value.source.from === 'body' ? '/user/role' : value.source.from === 'path' ? 'id' : 'name'}
        className="w-36 font-mono text-xs"
        list={value.source.from === 'path' ? listId : undefined}
        value={sourceName(value.source)}
        onChange={(e) => onChange({ ...value, source: withName(value.source.from, e.target.value) })}
      />
      <datalist id={listId}>
        {pathParams.map((p) => (
          <option key={p} value={p} />
        ))}
      </datalist>
      <Select
        aria-label="Operator"
        className="w-36"
        value={value.operator}
        onChange={(e) => {
          const operator = e.target.value as Operator
          onChange(needsValue(operator) ? { ...value, operator, value: value.value ?? '' } : { source: value.source, operator })
        }}
      >
        {OPERATORS.map((o) => (
          <option key={o} value={o}>
            {OPERATOR_LABELS[o]}
          </option>
        ))}
      </Select>
      {needsValue(value.operator) && (
        <Input
          aria-label="Value"
          placeholder="value"
          className="w-36 font-mono text-xs"
          value={value.value ?? ''}
          onChange={(e) => onChange({ ...value, value: e.target.value })}
        />
      )}
      <Button variant="ghost" size="icon-sm" aria-label="Remove condition" onClick={onRemove}>
        <X />
      </Button>
    </div>
  )
}

const newRule = (n: number): ResponseRule => ({
  name: `Rule ${n}`,
  when: [{ source: { from: 'query', name: 'scenario' }, operator: 'equals', value: 'error' }],
  response: {
    status: 500,
    headers: {},
    delay_ms: 0,
    body: { type: 'json', content: { error: 'Simulated failure' } },
  },
})

export function RulesEditor({
  value,
  onChange,
  pathParams,
  onSave,
}: {
  value: ResponseRule[]
  onChange: (rules: ResponseRule[]) => void
  pathParams: string[]
  onSave?: () => void
}) {
  const set = (i: number, rule: ResponseRule) => onChange(value.map((r, j) => (j === i ? rule : r)))
  const move = (i: number, by: number) => {
    const next = [...value]
    const [r] = next.splice(i, 1)
    next.splice(i + by, 0, r)
    onChange(next)
  }

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted-foreground">
        Rules are checked in order; the first rule whose conditions all hold answers instead of the default response.
      </p>
      {value.length === 0 && (
        <EmptyState title="No rules" className="rounded-lg border border-dashed py-6">
          Return a different response for specific requests, e.g. a 404 when <code>path.id</code> equals{' '}
          <code>missing</code>, or a 500 when <code>?scenario=error</code>.
        </EmptyState>
      )}
      {value.map((rule, i) => (
        <section key={i} aria-label={`Rule ${rule.name}`} className="flex flex-col gap-3 rounded-lg border p-3">
          <div className="flex items-center gap-2">
            <span className="grid size-6 place-items-center rounded bg-muted text-xs font-semibold">{i + 1}</span>
            <Input
              aria-label="Rule name"
              className="max-w-xs font-medium"
              value={rule.name}
              onChange={(e) => set(i, { ...rule, name: e.target.value })}
            />
            <div className="ml-auto flex items-center">
              <Button variant="ghost" size="icon-sm" aria-label="Move rule up" disabled={i === 0} onClick={() => move(i, -1)}>
                <ArrowUp />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Move rule down"
                disabled={i === value.length - 1}
                onClick={() => move(i, 1)}
              >
                <ArrowDown />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Delete rule"
                onClick={() => onChange(value.filter((_, j) => j !== i))}
              >
                <Trash2 />
              </Button>
            </div>
          </div>
          <div className="flex flex-col gap-1.5">
            <span className="text-xs font-medium text-muted-foreground">When all of these hold</span>
            {rule.when.map((c, ci) => (
              <ConditionRow
                key={ci}
                index={i * 100 + ci}
                value={c}
                pathParams={pathParams}
                onChange={(nc) => set(i, { ...rule, when: rule.when.map((x, k) => (k === ci ? nc : x)) })}
                onRemove={() => set(i, { ...rule, when: rule.when.filter((_, k) => k !== ci) })}
              />
            ))}
            {rule.when.length === 0 && <p className="text-xs text-destructive">Add at least one condition.</p>}
            <Button
              variant="ghost"
              size="sm"
              className="self-start text-muted-foreground"
              onClick={() =>
                set(i, {
                  ...rule,
                  when: [...rule.when, { source: { from: 'header', name: '' }, operator: 'exists' }],
                })
              }
            >
              <Plus /> Add condition
            </Button>
          </div>
          <div className="flex flex-col gap-1.5 border-t pt-3">
            <span className="text-xs font-medium text-muted-foreground">Respond with</span>
            <ResponseEditor
              label={`Rule ${rule.name}`}
              value={rule.response}
              bodyHeight={160}
              onSave={onSave}
              onChange={(response: MockResponse) => set(i, { ...rule, response })}
            />
          </div>
        </section>
      ))}
      <Button variant="outline" className="self-start" onClick={() => onChange([...value, newRule(value.length + 1)])}>
        <Plus /> Add rule
      </Button>
    </div>
  )
}
