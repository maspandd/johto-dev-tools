import { createContext, useCallback, useState } from 'react'

/** Lets JSON fields report invalid text so the form can refuse to save. */
export const InvalidFields = createContext<(key: string, error: string | null) => void>(() => {})

/** Problems reported by fields inside an `InvalidFields` provider. */
export function useInvalidFields(): [Map<string, string>, (key: string, error: string | null) => void] {
  const [invalid, setInvalid] = useState<Map<string, string>>(new Map())
  const report = useCallback((key: string, error: string | null) => {
    setInvalid((prev) => {
      if ((prev.get(key) ?? null) === error) return prev
      const next = new Map(prev)
      if (error) next.set(key, error)
      else next.delete(key)
      return next
    })
  }, [])
  return [invalid, report]
}
