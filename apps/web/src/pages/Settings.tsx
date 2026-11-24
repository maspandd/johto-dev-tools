import { useQueryClient } from '@tanstack/react-query'
import { CheckCircle2, XCircle } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, Input, Select } from '@/components/ui/form'
import { Kbd, Spinner } from '@/components/ui/misc'
import { api, loadConnection, saveConnection } from '@/lib/api'
import { type ThemeChoice, useTheme } from '@/lib/theme'
import { modKey } from '@/lib/utils'

const SHORTCUTS = [
  [`${modKey} K`, 'Command palette: search endpoints, pages and actions'],
  [`${modKey} S`, 'Save the current editor'],
  [`${modKey} Enter`, 'Send the request in “Try it”'],
  ['Middle-click a tab', 'Close an endpoint tab'],
]

export function SettingsPage() {
  const qc = useQueryClient()
  const { choice, setChoice } = useTheme()
  const [conn, setConn] = useState(loadConnection)
  const [check, setCheck] = useState<null | 'checking' | 'ok' | 'failed'>(null)

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-6 p-4">
        <h1 className="text-lg font-semibold">Settings</h1>

        <section aria-labelledby="conn-title" className="flex flex-col gap-3 rounded-lg border p-4">
          <h2 id="conn-title" className="text-sm font-semibold">
            Workbench API
          </h2>
          <form
            className="flex flex-col gap-3"
            onSubmit={async (e) => {
              e.preventDefault()
              saveConnection(conn)
              setCheck('checking')
              const ok = await api.health()
              setCheck(ok ? 'ok' : 'failed')
              await qc.invalidateQueries()
            }}
          >
            <Field label="API address" hint="Empty uses this page's server (the dev server forwards /api to 127.0.0.1:8080).">
              {(id) => (
                <Input
                  id={id}
                  placeholder="http://127.0.0.1:8080"
                  value={conn.baseUrl}
                  onChange={(e) => setConn({ ...conn, baseUrl: e.target.value })}
                />
              )}
            </Field>
            <Field
              label="Access token"
              hint="Only needed when the server runs with WORKBENCH_API_TOKEN. Stored in this browser."
            >
              {(id) => (
                <Input
                  id={id}
                  type="password"
                  autoComplete="off"
                  value={conn.token}
                  onChange={(e) => setConn({ ...conn, token: e.target.value })}
                />
              )}
            </Field>
            <div className="flex items-center gap-3">
              <Button type="submit">Save and test</Button>
              {check === 'checking' && <Spinner />}
              {check === 'ok' && (
                <span className="flex items-center gap-1 text-sm text-success">
                  <CheckCircle2 className="size-4" /> Connected
                </span>
              )}
              {check === 'failed' && (
                <span className="flex items-center gap-1 text-sm text-destructive">
                  <XCircle className="size-4" /> Not reachable
                </span>
              )}
            </div>
          </form>
        </section>

        <section aria-labelledby="look-title" className="flex flex-col gap-3 rounded-lg border p-4">
          <h2 id="look-title" className="text-sm font-semibold">
            Appearance
          </h2>
          <Field label="Theme" className="w-48">
            {(id) => (
              <Select id={id} value={choice} onChange={(e) => setChoice(e.target.value as ThemeChoice)}>
                <option value="system">Match the system</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </Select>
            )}
          </Field>
        </section>

        <section aria-labelledby="keys-title" className="flex flex-col gap-3 rounded-lg border p-4">
          <h2 id="keys-title" className="text-sm font-semibold">
            Keyboard shortcuts
          </h2>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            {SHORTCUTS.map(([k, d]) => (
              <div key={k} className="contents">
                <dt>
                  <Kbd>{k}</Kbd>
                </dt>
                <dd className="text-muted-foreground">{d}</dd>
              </div>
            ))}
          </dl>
        </section>
      </div>
    </div>
  )
}
