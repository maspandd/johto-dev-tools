import { Check, ChevronsUpDown, Plus } from 'lucide-react'
import { useNavigate } from 'react-router'

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/overlay'
import { useProjects } from '@/lib/queries'
import type { Project } from '@/lib/types'

export function ProjectSwitcher({ project }: { project: Project }) {
  const projects = useProjects(project.workspace_id)
  const navigate = useNavigate()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="flex min-w-0 items-center gap-1 rounded px-1.5 py-1 font-medium text-foreground hover:bg-accent">
        <span className="truncate">{project.name}</span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="max-h-80 overflow-y-auto">
        <DropdownMenuLabel>Projects in this workspace</DropdownMenuLabel>
        {(projects.data ?? []).map((p) => (
          <DropdownMenuItem key={p.id} onSelect={() => navigate(`/p/${p.id}`)}>
            <Check className={p.id === project.id ? 'opacity-100' : 'opacity-0'} />
            <span className="truncate">{p.name}</span>
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => navigate('/')}>
          <Plus /> New or other project…
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
