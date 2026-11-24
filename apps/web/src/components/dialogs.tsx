import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Field, Input } from '@/components/ui/form'
import { ErrorNotice, Spinner } from '@/components/ui/misc'
import { Dialog, DialogContent, DialogFooter } from '@/components/ui/overlay'

/** Asks before something that cannot be undone. */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Delete',
  destructive = true,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: React.ReactNode
  confirmLabel?: string
  destructive?: boolean
  onConfirm: () => Promise<unknown> | void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) setError(null)
        onOpenChange(o)
      }}
    >
      <DialogContent title={title} description={description}>
        <ErrorNotice error={error} />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            variant={destructive ? 'destructive' : 'default'}
            disabled={busy}
            onClick={async () => {
              setBusy(true)
              setError(null)
              try {
                await onConfirm()
                onOpenChange(false)
              } catch (e) {
                setError(e)
              } finally {
                setBusy(false)
              }
            }}
          >
            {busy && <Spinner className="text-current" />}
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Asks for one name, e.g. to create or rename something. */
export function NameDialog({
  open,
  onOpenChange,
  title,
  label = 'Name',
  initial = '',
  submitLabel = 'Create',
  onSubmit,
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  label?: string
  initial?: string
  submitLabel?: string
  onSubmit: (name: string) => Promise<unknown>
  children?: React.ReactNode
}) {
  const [name, setName] = useState(initial)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (o) setName(initial)
        setError(null)
        onOpenChange(o)
      }}
    >
      <DialogContent title={title}>
        <form
          className="flex flex-col gap-4"
          onSubmit={async (e) => {
            e.preventDefault()
            if (!name.trim()) return
            setBusy(true)
            setError(null)
            try {
              await onSubmit(name.trim())
              onOpenChange(false)
            } catch (err) {
              setError(err)
            } finally {
              setBusy(false)
            }
          }}
        >
          <Field label={label}>
            {(id) => <Input id={id} autoFocus value={name} onChange={(e) => setName(e.target.value)} maxLength={200} />}
          </Field>
          {children}
          <ErrorNotice error={error} />
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !name.trim()}>
              {busy && <Spinner className="text-current" />}
              {submitLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
