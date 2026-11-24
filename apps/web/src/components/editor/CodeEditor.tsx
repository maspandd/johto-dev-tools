import Editor, { type OnMount } from '@monaco-editor/react'
import { useLayoutEffect, useRef, useState } from 'react'

import { useTheme } from '@/lib/theme'

import { jsonDefaults } from './monaco'

export interface CodeEditorProps {
  value: string
  onChange?: (value: string) => void
  language?: 'json' | 'plaintext'
  readOnly?: boolean
  /** CSS height; default fills the parent. */
  height?: string | number
  /** JSON Schema used to validate and complete the text. */
  schema?: object
  ariaLabel?: string
  onSave?: () => void
}

let schemaCounter = 0

export default function CodeEditor({
  value,
  onChange,
  language = 'json',
  readOnly = false,
  height = '100%',
  schema,
  ariaLabel,
  onSave,
}: CodeEditorProps) {
  const { resolved } = useTheme()
  const [modelUri] = useState(() => `inmemory://workbench/${++schemaCounter}.json`)
  const saveRef = useRef(onSave)
  useLayoutEffect(() => {
    saveRef.current = onSave
  })

  const onMount: OnMount = (editor, monaco) => {
    editor.addAction({
      id: 'workbench.save',
      label: 'Save',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
      run: () => saveRef.current?.(),
    })
    if (schema) {
      const json = jsonDefaults
      const others = (json.diagnosticsOptions.schemas ?? []).filter((s: { uri: string }) => s.uri !== `${modelUri}#schema`)
      json.setDiagnosticsOptions({
        validate: true,
        allowComments: false,
        schemas: [...others, { uri: `${modelUri}#schema`, fileMatch: [modelUri], schema }],
      })
    }
  }

  return (
    <div className="h-full min-h-0 overflow-hidden rounded-md border" style={{ height }}>
      <Editor
        value={value}
        language={language}
        path={language === 'json' ? modelUri : undefined}
        theme={resolved === 'dark' ? 'vs-dark' : 'vs'}
        onChange={(v) => onChange?.(v ?? '')}
        onMount={onMount}
        options={{
          readOnly,
          minimap: { enabled: false },
          fontSize: 12.5,
          lineNumbersMinChars: 3,
          scrollBeyondLastLine: false,
          tabSize: 2,
          automaticLayout: true,
          wordWrap: 'on',
          renderLineHighlight: readOnly ? 'none' : 'line',
          ariaLabel,
          padding: { top: 6 },
          fixedOverflowWidgets: true,
        }}
      />
    </div>
  )
}
