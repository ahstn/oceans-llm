import type { ServiceTierView as ServiceTier } from '@/types/api'

const SERVICE_TIER_LABELS: Record<ServiceTier, string> = {
  default: 'Standard (default)',
  flex: 'Flex',
  priority: 'Fast (priority)',
  ultrafast: 'Ultrafast',
}

/** The tier's display name. Providers call the priority tier Fast. */
export function serviceTierLabel(tier: ServiceTier): string {
  return SERVICE_TIER_LABELS[tier]
}
