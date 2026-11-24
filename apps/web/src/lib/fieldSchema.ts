// JSON Schema for data generator field lists (`DataSchema.fields`), so the
// editor can complete and check them. Mirrors `FieldKind` in
// `crates/workbench-domain/src/dataset.rs`.

const kind = (type: string, description: string, properties: Record<string, object> = {}, required: string[] = []) => ({
  if: { properties: { type: { const: type } } },
  then: { properties, required },
  description,
})

const KINDS = [
  kind('uuid', 'Random UUID'),
  kind('sequence', 'start, start + step, … by record', { start: { type: 'integer' }, step: { type: 'integer' } }),
  kind('integer', 'Whole number in a range', { min: { type: 'integer' }, max: { type: 'integer' } }, ['min', 'max']),
  kind(
    'decimal',
    'Number with fixed decimals',
    { min: { type: 'number' }, max: { type: 'number' }, scale: { type: 'integer', minimum: 0, maximum: 10 } },
    ['min', 'max'],
  ),
  kind('boolean', 'true or false'),
  kind('first_name', 'First name'),
  kind('last_name', 'Last name'),
  kind('full_name', 'Full name'),
  kind('email', 'Email address (example.test domain)'),
  kind('phone', 'Phone number'),
  kind('address', 'Street address'),
  kind('city', 'City'),
  kind('company', 'Company name'),
  kind('words', 'Random words', { min: { type: 'integer', minimum: 1 }, max: { type: 'integer' } }),
  kind('date', 'YYYY-MM-DD in a range', { from: { type: 'string', format: 'date' }, to: { type: 'string', format: 'date' } }, [
    'from',
    'to',
  ]),
  kind(
    'timestamp',
    'RFC 3339 time in a range',
    { from: { type: 'string', format: 'date-time' }, to: { type: 'string', format: 'date-time' } },
    ['from', 'to'],
  ),
  kind('enum', 'One of the given values', { values: { type: 'array', minItems: 1 } }, ['values']),
  kind('constant', 'Always the same value', { value: {} }, ['value']),
  kind('template', '# → digit, ? → letter, {{field}} → earlier field', { template: { type: 'string', minLength: 1 } }, [
    'template',
  ]),
  kind('object', 'Nested object', { fields: { $ref: '#/$defs/fields' } }, ['fields']),
  kind(
    'array',
    'List of generated values',
    { items: { $ref: '#/$defs/kind' }, min_items: { type: 'integer', minimum: 0 }, max_items: { type: 'integer' } },
    ['items'],
  ),
  kind(
    'reference',
    'A value from a record of another resource',
    { resource: { type: 'string' }, field: { type: 'string' } },
    ['resource'],
  ),
]

const TYPES = KINDS.map((k) => (k.if.properties.type as { const: string }).const)

export const FIELDS_JSON_SCHEMA = {
  $defs: {
    kind: {
      type: 'object',
      required: ['type'],
      properties: {
        type: { enum: TYPES, markdownEnumDescriptions: KINDS.map((k) => k.description) },
      },
      allOf: KINDS.map(({ if: i, then }) => ({ if: i, then })),
    },
    field: {
      allOf: [
        { $ref: '#/$defs/kind' },
        {
          type: 'object',
          required: ['name', 'type'],
          properties: {
            name: { type: 'string', minLength: 1 },
            null_rate: { type: 'number', minimum: 0, maximum: 1, description: 'Probability of null' },
            unique: { type: 'boolean', description: 'No two records share a value' },
          },
        },
      ],
    },
    fields: { type: 'array', items: { $ref: '#/$defs/field' } },
  },
  $ref: '#/$defs/fields',
}

export const FIELD_TYPES = TYPES
