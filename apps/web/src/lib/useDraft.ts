import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

/**
 * Local edits of a saved value with an explicit save. Resets when the
 * saved value changes (another tab, a reload), and saves on Ctrl/⌘+S.
 */
export function useDraft<T>(source: T, save: (draft: T) => Promise<unknown>) {
  const [draft, setDraft] = useState<T>(source)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const base = useRef(source)

  useEffect(() => {
    if (JSON.stringify(source) !== JSON.stringify(base.current)) {
      base.current = source
      setDraft(source)
      setError(null)
    }
  }, [source])

  const dirty = JSON.stringify(draft) !== JSON.stringify(source)
  const draftRef = useRef(draft)
  useLayoutEffect(() => {
    draftRef.current = draft
  })

  const commit = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      await save(draftRef.current)
      return true
    } catch (e) {
      setError(e)
      return false
    } finally {
      setSaving(false)
    }
  }, [save])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      if (dirty) void commit()
    }
  }

  return {
    draft,
    setDraft,
    set: (patch: Partial<T>) => setDraft((d) => ({ ...d, ...patch })),
    dirty,
    saving,
    error,
    commit,
    reset: () => setDraft(source),
    onKeyDown,
  }
}
