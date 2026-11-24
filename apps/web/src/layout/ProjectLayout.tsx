import { useState } from 'react'
import { Outlet, useParams } from 'react-router'
import { useDefaultLayout, usePanelRef } from 'react-resizable-panels'

import { RequestHistory } from '@/components/RequestHistory'
import { ErrorNotice, Spinner } from '@/components/ui/misc'
import { ResizableGroup, ResizableHandle, ResizablePanel } from '@/components/ui/resizable'
import { useProject } from '@/lib/queries'

/** Project pages above, the request history panel below. */
export function ProjectLayout() {
  const { projectId } = useParams()
  const project = useProject(projectId)
  const panel = usePanelRef()
  const [collapsed, setCollapsed] = useState(false)
  const layout = useDefaultLayout({ id: 'workbench.project-layout', storage: localStorage })

  if (project.isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    )
  }
  if (project.error) return <ErrorNotice error={project.error} className="m-4" />

  return (
    <ResizableGroup
      orientation="vertical"
      defaultLayout={layout.defaultLayout}
      onLayoutChanged={layout.onLayoutChanged}
    >
      <ResizablePanel id="content" minSize="25%">
        <div className="h-full overflow-hidden">
          <Outlet />
        </div>
      </ResizablePanel>
      <ResizableHandle />
      <ResizablePanel
        id="history"
        panelRef={panel}
        defaultSize="30%"
        minSize="120px"
        collapsible
        collapsedSize="36px"
        onResize={() => setCollapsed(panel.current?.isCollapsed() ?? false)}
      >
        <RequestHistory
          projectId={projectId!}
          compact
          collapsed={collapsed}
          onToggle={() => (panel.current?.isCollapsed() ? panel.current.expand() : panel.current?.collapse())}
        />
      </ResizablePanel>
    </ResizableGroup>
  )
}
