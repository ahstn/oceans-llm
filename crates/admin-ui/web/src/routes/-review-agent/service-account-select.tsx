import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { ServiceAccountView } from '@/types/api'

import { formatServiceAccount } from './review-agent-meta'

/** Picks the service account whose API key the review action authenticates with. */
export function ServiceAccountSelect({
  id,
  value,
  serviceAccounts,
  onValueChange,
}: {
  id: string
  value: string | undefined
  serviceAccounts: ServiceAccountView[]
  onValueChange: (value: string) => void
}) {
  return (
    <Select value={value} onValueChange={onValueChange}>
      <SelectTrigger id={id}>
        <SelectValue placeholder="Select service account" />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {serviceAccounts.map((account) => (
            <SelectItem key={account.id} value={account.id}>
              {formatServiceAccount(account)}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
