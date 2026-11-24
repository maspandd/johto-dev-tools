import { Save, Undo2 } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Kbd, Spinner } from '@/components/ui/misc'
import { modKey } from '@/lib/utils'

/** Save / discard buttons with the unsaved-changes state. */
export function SaveBar({
  dirty,
  saving,
  blocked,
  onSave,
  onReset,
}: {
  dirty: boolean
  saving: boolean
  blocked?: boolean
  onSave: () => void
  onReset: () => void
}) {
  return (
    <div className="flex items-center gap-2">
      <span role="status" className="text-xs text-muted-foreground">
        {saving ? 'Saving…' : blocked ? 'Fix errors to save' : dirty ? 'Unsaved changes' : 'Saved'}
      </span>
      <Button variant="ghost" size="sm" disabled={!dirty || saving} onClick={onReset}>
        <Undo2 /> Discard
      </Button>
      <Button size="sm" disabled={!dirty || saving || blocked} onClick={onSave}>
        {saving ? <Spinner className="text-current" /> : <Save />} Save <Kbd>{modKey} S</Kbd>
      </Button>
    </div>
  )
}
