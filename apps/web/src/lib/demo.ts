// The demo project (`examples/demo-project.json`, also used by the
// `workbench-mock` CLI), created through the API so it can be edited.

import demo from '../../../../examples/demo-project.json'

import { api } from './api'
import type { EndpointDef, Id, Project } from './types'

export async function createDemoProject(workspaceId: Id): Promise<Project> {
  const project = await api.createProject({
    workspace_id: workspaceId,
    name: demo.name,
    description: 'Sample endpoints, response rules, auth, delays and two CRUD resources. Safe to edit or delete.',
  })
  for (const endpoint of demo.endpoints) {
    await api.createEndpoint(project.id, endpoint as unknown as Partial<EndpointDef>)
  }
  for (const resource of demo.resources) {
    await api.createResource(project.id, resource as Record<string, unknown>)
  }
  await api.createEnvironment(project.id, {
    name: 'Development',
    variables: { API_VERSION: '1' },
  })
  await api.createDataset(project.id, {
    name: 'Customers',
    description: 'Synthetic customers for testing imports',
    count: 25,
    seed: 42,
    schema: {
      fields: [
        { name: 'id', type: 'uuid' },
        { name: 'name', type: 'full_name' },
        { name: 'email', type: 'email', unique: true },
        { name: 'city', type: 'city' },
        { name: 'joined', type: 'date', from: '2024-01-01', to: '2026-06-30' },
      ],
    },
  })
  return project
}
