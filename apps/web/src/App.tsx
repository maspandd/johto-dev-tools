// Phase 0 shell. The real API Studio UI (editors, inspector, history panel)
// lands in Phase 6 once the management API exists.

type Module = { name: string; planned?: boolean }

const MODULES: Module[] = [
  { name: 'Dashboard' },
  { name: 'API Studio' },
  { name: 'SQL Lab', planned: true },
  { name: 'Data Lab', planned: true },
  { name: 'Code Generator', planned: true },
  { name: 'Testing Lab', planned: true },
  { name: 'Developer Utilities', planned: true },
]

function App() {
  return (
    <div className="flex h-screen flex-col bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <header className="flex h-12 items-center border-b border-neutral-200 px-4 dark:border-neutral-800">
        <span className="font-semibold">Developer Workbench</span>
        <span className="ml-3 text-sm text-neutral-500">Build. Test. Simulate. Develop.</span>
      </header>
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Modules" className="w-56 border-r border-neutral-200 p-2 dark:border-neutral-800">
          <ul className="space-y-1">
            {MODULES.map((m) => (
              <li key={m.name}>
                <button
                  type="button"
                  disabled={m.planned}
                  className="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-50 dark:hover:bg-neutral-900"
                >
                  {m.name}
                  {m.planned && (
                    <span className="rounded bg-neutral-200 px-1.5 text-xs dark:bg-neutral-800">Planned</span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <main className="flex flex-1 items-center justify-center p-8">
          <div className="max-w-md text-center">
            <h1 className="text-xl font-semibold">API Studio</h1>
            <p className="mt-2 text-sm text-neutral-500">
              The mock engine runs today via the <code>workbench-mock</code> CLI. The visual endpoint editor arrives
              once the management API is in place.
            </p>
          </div>
        </main>
      </div>
    </div>
  )
}

export default App
