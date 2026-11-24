/** Records as a table; nested values are shown as compact JSON. */
export function RecordsTable({ rows }: { rows: Record<string, unknown>[] }) {
  const columns = [...new Set(rows.flatMap((r) => Object.keys(r)))]
  const cell = (v: unknown) =>
    v === null || v === undefined ? (
      <span className="text-muted-foreground">null</span>
    ) : typeof v === 'object' ? (
      JSON.stringify(v)
    ) : (
      String(v)
    )
  return (
    <div className="overflow-auto rounded-md border">
      <table className="w-full text-xs">
        <thead className="bg-muted/50 text-left">
          <tr>
            {columns.map((c) => (
              <th key={c} scope="col" className="px-2 py-1.5 font-mono font-medium whitespace-nowrap">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} className="border-t">
              {columns.map((c) => (
                <td key={c} className="max-w-64 truncate px-2 py-1 font-mono" title={JSON.stringify(r[c])}>
                  {cell(r[c])}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
