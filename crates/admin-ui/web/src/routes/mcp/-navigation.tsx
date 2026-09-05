import { Link } from '@tanstack/react-router'
import { Button } from '@/components/ui/button'

export function McpNavigation({ current }: { current: 'servers' | 'toolsets' | 'access' }) {
  return (
    <nav aria-label="MCP sections" className="flex flex-wrap items-center gap-2 border-b pb-4">
      <Button asChild variant={current === 'servers' ? 'secondary' : 'ghost'}>
        <Link
          to="/mcp"
          search={{ tab: 'servers' }}
          aria-current={current === 'servers' ? 'page' : undefined}
        >
          Servers
        </Link>
      </Button>
      <Button asChild variant={current === 'toolsets' ? 'secondary' : 'ghost'}>
        <Link
          to="/mcp/toolsets"
          search={{}}
          aria-current={current === 'toolsets' ? 'page' : undefined}
        >
          Tool Sets
        </Link>
      </Button>
      <Button asChild variant={current === 'access' ? 'secondary' : 'ghost'}>
        <Link
          to="/mcp"
          search={{ tab: 'access' }}
          aria-current={current === 'access' ? 'page' : undefined}
        >
          Access
        </Link>
      </Button>
    </nav>
  )
}
