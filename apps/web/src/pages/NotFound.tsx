import { Compass } from 'lucide-react'
import { Link } from 'react-router'

import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/misc'

export function NotFound() {
  return (
    <EmptyState
      icon={<Compass />}
      title="Page not found"
      className="h-full"
      action={
        <Button asChild>
          <Link to="/">Back to the dashboard</Link>
        </Button>
      }
    >
      The address does not match any page.
    </EmptyState>
  )
}
