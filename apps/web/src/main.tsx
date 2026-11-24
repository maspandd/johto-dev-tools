import { QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'

import { ThemedToaster } from '@/components/ThemedToaster'
import { TooltipProvider } from '@/components/ui/overlay'
import { AppShell } from '@/layout/AppShell'
import { ProjectLayout } from '@/layout/ProjectLayout'
import { queryClient } from '@/lib/queries'
import { ThemeProvider } from '@/lib/theme'
import { Dashboard } from '@/pages/Dashboard'
import { DatasetsPage } from '@/pages/Datasets'
import { EndpointsPage } from '@/pages/Endpoints'
import { EnvironmentsPage } from '@/pages/Environments'
import { ImportExportPage } from '@/pages/ImportExport'
import { NotFound } from '@/pages/NotFound'
import { ProjectSettingsPage } from '@/pages/ProjectSettings'
import { ResourcesPage } from '@/pages/Resources'
import { RouteError } from '@/pages/RouteError'
import { ServersPage } from '@/pages/Servers'
import { SettingsPage } from '@/pages/Settings'

import './index.css'

const router = createBrowserRouter([
  {
    path: '/',
    element: <AppShell />,
    errorElement: <RouteError />,
    children: [
      { index: true, element: <Dashboard /> },
      { path: 'settings', element: <SettingsPage /> },
      {
        path: 'p/:projectId',
        element: <ProjectLayout />,
        errorElement: <RouteError />,
        children: [
          { index: true, element: <Navigate to="endpoints" replace /> },
          { path: 'endpoints/:endpointId?', element: <EndpointsPage /> },
          { path: 'resources/:resourceId?', element: <ResourcesPage /> },
          { path: 'datasets/:datasetId?', element: <DatasetsPage /> },
          { path: 'environments/:environmentId?', element: <EnvironmentsPage /> },
          { path: 'servers', element: <ServersPage /> },
          { path: 'import-export', element: <ImportExportPage /> },
          { path: 'settings', element: <ProjectSettingsPage /> },
        ],
      },
      { path: '*', element: <NotFound /> },
    ],
  },
])

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={300}>
          <RouterProvider router={router} />
          <ThemedToaster />
        </TooltipProvider>
      </QueryClientProvider>
    </ThemeProvider>
  </StrictMode>,
)
