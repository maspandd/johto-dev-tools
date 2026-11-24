import type * as React from 'react'
import { Group, Panel, Separator } from 'react-resizable-panels'

import { cn } from '@/lib/utils'

export { Panel as ResizablePanel }

export function ResizableGroup({ className, ...props }: React.ComponentProps<typeof Group>) {
  return <Group className={cn('flex h-full w-full', className)} {...props} />
}

/** A thin, keyboard-accessible drag handle between panels. */
export function ResizableHandle({ className, ...props }: React.ComponentProps<typeof Separator>) {
  return (
    <Separator
      className={cn(
        'relative shrink-0 bg-border outline-none transition-colors hover:bg-primary/60 focus-visible:bg-primary data-[separator=active]:bg-primary',
        'aria-[orientation=vertical]:w-px aria-[orientation=horizontal]:h-px',
        'after:absolute after:inset-0 aria-[orientation=horizontal]:after:-inset-y-1 aria-[orientation=vertical]:after:-inset-x-1',
        className,
      )}
      {...props}
    />
  )
}
