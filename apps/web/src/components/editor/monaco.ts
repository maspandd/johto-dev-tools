// Monaco is bundled with the app (no CDN), so the editor also works
// offline and in the desktop shell. This module is only imported lazily.

import { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor/editor/editor.api'
import { jsonDefaults } from 'monaco-editor/languages/features/json/register'

import './features'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'
import JsonWorker from 'monaco-editor/language/json/json.worker?worker'

self.MonacoEnvironment = {
  getWorker(_id: string, label: string) {
    return label === 'json' ? new JsonWorker() : new EditorWorker()
  },
}

loader.config({ monaco })

export { jsonDefaults, monaco }
