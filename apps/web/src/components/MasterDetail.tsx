import { Plus } from 'lucide-react'
import { Link } from 'react-router'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface ListItem {
  id: string
  title: React.ReactNode
  subtitle?: React.ReactNode
  dim?: boolean
}

/** A list of items on the left and the selected item's editor on the right. */
export function MasterDetail({
  title,
  items,
  activeId,
  basePath,
  onNew,
  newLabel,
  children,
}: {
  title: string
  items: ListItem[]
  activeId?: string
  basePath: string
  onNew: () => void
  newLabel: string
  children: React.ReactNode
}) {
  return (
    <div className="flex h-full min-h-0">
      <nav aria-label={title} className="flex w-56 shrink-0 flex-col border-r bg-sidebar lg:w-64">
        <div className="flex items-center justify-between border-b px-3 py-2">
          <h1 className="text-sm font-semibold">{title}</h1>
          <Button size="icon-sm" aria-label={newLabel} onClick={onNew}>
            <Plus />
          </Button>
        </div>
        <ul className="min-h-0 flex-1 overflow-y-auto p-1">
          {items.map((item) => (
            <li key={item.id}>
              <Link
                to={`${basePath}/${item.id}`}
                aria-current={item.id === activeId ? 'page' : undefined}
                className={cn(
                  'flex flex-col rounded-md px-2 py-1.5 text-sm',
                  item.id === activeId ? 'bg-accent text-accent-foreground' : 'hover:bg-accent/60',
                  item.dim && 'opacity-55',
                )}
              >
                <span className="truncate font-medium">{item.title}</span>
                {item.subtitle && <span className="truncate text-xs text-muted-foreground">{item.subtitle}</span>}
              </Link>
            </li>
          ))}
        </ul>
      </nav>
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto">{children}</div>
    </div>
  )
}
