import { ArrowLeft01Icon, ArrowRight01Icon, Search01Icon } from '@hugeicons/core-free-icons'

import { AppIcon } from '@/components/icons/app-icon'
import { Button } from '@/components/ui/button'
import { InputGroup, InputGroupAddon, InputGroupInput } from '@/components/ui/input-group'
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { ModelPageView } from '@/types/api'

export function ModelSearch({
  query,
  onQueryChange,
}: {
  query: string
  onQueryChange: (query: string) => void
}) {
  return (
    <InputGroup className="w-full md:max-w-xs">
      <InputGroupInput
        type="search"
        aria-label="Search models"
        placeholder="Search models…"
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
      />
      <InputGroupAddon>
        <AppIcon icon={Search01Icon} aria-hidden />
      </InputGroupAddon>
    </InputGroup>
  )
}

export function ModelListPagination({
  modelPage,
  onPageChange,
  isPending,
}: {
  modelPage: ModelPageView
  onPageChange: (page: number, pageSize: number) => void
  isPending: boolean
}) {
  const { page, page_size: pageSize, total, items } = modelPage
  const totalPages = Math.max(1, Math.ceil(total / pageSize))
  const first = items.length === 0 ? 0 : (page - 1) * pageSize + 1
  const last = items.length === 0 ? 0 : Math.min(first + items.length - 1, total)
  const pageSizes = [...new Set([10, 20, 30, 50, 100, pageSize])].sort((a, b) => a - b)

  return (
    <nav
      aria-label="Model pagination"
      aria-busy={isPending}
      className="text-muted-foreground flex min-w-0 flex-wrap items-center justify-end gap-x-6 gap-y-3 text-sm"
    >
      <div className="flex items-center gap-2">
        <span>Rows per page</span>
        <Select
          value={String(pageSize)}
          onValueChange={(value) => onPageChange(1, Number(value))}
          disabled={isPending}
        >
          <SelectTrigger size="sm" aria-label="Rows per page">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              {pageSizes.map((size) => (
                <SelectItem key={size} value={String(size)}>
                  {size}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>
      <span className="tabular-nums" role="status">
        {first}–{last} of {total}
      </span>
      <div className="flex items-center gap-3">
        <Button
          type="button"
          variant="outline"
          size="icon-sm"
          aria-label="Previous page"
          onClick={() => onPageChange(page - 1, pageSize)}
          disabled={isPending || page <= 1}
        >
          <AppIcon icon={ArrowLeft01Icon} aria-hidden />
        </Button>
        <span className="whitespace-nowrap tabular-nums">
          Page {page} of {totalPages}
        </span>
        <Button
          type="button"
          variant="outline"
          size="icon-sm"
          aria-label="Next page"
          onClick={() => onPageChange(page + 1, pageSize)}
          disabled={isPending || page >= totalPages}
        >
          <AppIcon icon={ArrowRight01Icon} aria-hidden />
        </Button>
      </div>
    </nav>
  )
}
