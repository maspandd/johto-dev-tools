import { useQueryClient } from '@tanstack/react-query'
import { FileArchive, KeyRound } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Field, Input } from '@/components/ui/form'
import { ErrorNotice, MethodBadge, Spinner } from '@/components/ui/misc'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/overlay'
import { api } from '@/lib/api'
import { keys } from '@/lib/queries'
import type { ArchivePreview } from '@/lib/types'

/** Pick a `.zip`, preview what is inside, then import it as a new project. */
export function ArchiveImportDialog({
  workspaceId,
  open,
  onOpenChange,
}: {
  workspaceId: string
  open: boolean
  onOpenChange: (o: boolean) => void
}) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const [file, setFile] = useState<File | null>(null)
  const [preview, setPreview] = useState<ArchivePreview | null>(null)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  const reset = () => {
    setFile(null)
    setPreview(null)
    setName('')
    setError(null)
  }

  const pick = async (f: File | null) => {
    reset()
    if (!f) return
    setFile(f)
    setBusy(true)
    try {
      const p = await api.previewArchive(workspaceId, f)
      setPreview(p)
      setName(p.project.name)
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  const importNow = async () => {
    if (!file) return
    setBusy(true)
    setError(null)
    try {
      const result = await api.importArchive(workspaceId, file, name.trim() || undefined)
      await qc.invalidateQueries({ queryKey: keys.projects(workspaceId) })
      toast.success(`Imported “${result.project.name}”`)
      if (result.secrets_to_set.length) {
        toast.info('Set the secrets listed in each environment; their values are never exported.')
      }
      onOpenChange(false)
      reset()
      navigate(`/p/${result.project.id}`)
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) reset()
        onOpenChange(o)
      }}
    >
      <DialogContent
        title="Import a project archive"
        description="A .zip exported from Developer Workbench. It becomes a new project; nothing existing is changed."
        className="w-[min(92vw,40rem)]"
      >
        <label className="flex cursor-pointer flex-col items-center gap-2 rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground hover:border-primary/50">
          <FileArchive className="size-6" />
          {file ? <span className="font-medium text-foreground">{file.name}</span> : 'Choose a .zip archive'}
          <input
            type="file"
            accept=".zip,application/zip"
            className="sr-only"
            onChange={(e) => void pick(e.target.files?.[0] ?? null)}
          />
        </label>

        {busy && !preview && <Spinner />}
        <ErrorNotice error={error} />

        {preview && (
          <div className="flex flex-col gap-3 text-sm">
            <Field label="Project name">
              {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} />}
            </Field>
            <p className="text-xs text-muted-foreground">
              Exported {new Date(preview.manifest.exported_at).toLocaleString()} by {preview.manifest.generator} ·
              format {preview.manifest.format_version}
            </p>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
              {(
                [
                  ['Endpoints', preview.endpoints.length],
                  ['Resources', preview.resources.length],
                  ['Records', preview.manifest.contents.records],
                  ['Datasets', preview.datasets.length],
                  ['Environments', preview.environments.length],
                ] as const
              ).map(([label, n]) => (
                <div key={label} className="rounded-md border p-2 text-center">
                  <div className="text-lg font-semibold">{n}</div>
                  <div className="text-xs text-muted-foreground">{label}</div>
                </div>
              ))}
            </div>
            {preview.endpoints.length > 0 && (
              <ul className="max-h-40 overflow-auto rounded-md border p-2 text-xs">
                {preview.endpoints.map((e, i) => (
                  <li key={i} className="flex items-center gap-2 py-0.5">
                    <MethodBadge method={e.method} />
                    <code className="font-mono">{e.path}</code>
                    <span className="truncate text-muted-foreground">{e.name}</span>
                    {!e.enabled && <span className="text-muted-foreground">(disabled)</span>}
                  </li>
                ))}
              </ul>
            )}
            {preview.secrets_to_set.length > 0 && (
              <div className="flex items-start gap-2 rounded-md border p-2 text-xs">
                <KeyRound className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                <span>
                  Secret values are never exported. After importing, set:{' '}
                  {preview.secrets_to_set.map((s) => `${s.names.join(', ')} (${s.environment})`).join('; ')}
                </span>
              </div>
            )}
            {preview.warnings.length > 0 && (
              <ul className="list-disc space-y-0.5 pl-5 text-xs text-amber-700 dark:text-amber-300">
                {preview.warnings.map((w, i) => (
                  <li key={i}>{w}</li>
                ))}
              </ul>
            )}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={!preview || busy} onClick={importNow}>
            {busy && preview && <Spinner className="text-current" />}
            Import as new project
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
