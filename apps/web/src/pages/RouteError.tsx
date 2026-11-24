import { AlertOctagon } from 'lucide-react'
import { Link, isRouteErrorResponse, useRouteError } from 'react-router'

import { Button } from '@/components/ui/button'

/** Shown instead of a page that failed to render, keeping the app usable. */
export function RouteError() {
  const error = useRouteError()
  const message = isRouteErrorResponse(error)
    ? `${error.status} ${error.statusText}`
    : error instanceof Error
      ? error.message
      : 'Unknown error'
  return (
    <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 p-8 text-center">
      <AlertOctagon className="size-8 text-destructive" />
      <h1 className="text-base font-semibold">This page hit an error</h1>
      <p className="max-w-md font-mono text-xs break-all text-muted-foreground">{message}</p>
      <div className="flex gap-2">
        <Button onClick={() => window.location.reload()}>Reload</Button>
        <Button variant="outline" asChild>
          <Link to="/">Dashboard</Link>
        </Button>
      </div>
    </div>
  )
}
