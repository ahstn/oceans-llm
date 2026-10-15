import { useState, useTransition } from 'react'
import { toast } from 'sonner'

import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { discoverExternalMcpServerOauth } from '@/server/admin-data.functions'
import type { McpOauthDiscoveryPayload } from '@/types/api'

const REGISTRATION_LABELS: Record<string, string> = {
  dynamic: 'Dynamic client registration',
  metadata_document: 'Client metadata document',
  manual: 'Admin-supplied client ID',
}

/**
 * Reads an MCP server's OAuth metadata so an admin can register it by URL.
 * Applying the result fills the auth mode and auth config JSON, which the
 * admin can still edit before saving.
 */
export function OauthDiscoveryPanel({
  serverUrl,
  onApply,
}: {
  serverUrl: string
  onApply: (authConfig: Record<string, unknown>) => void
}) {
  const [result, setResult] = useState<McpOauthDiscoveryPayload | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [isPending, startTransition] = useTransition()
  const trimmedUrl = serverUrl.trim()

  function discover() {
    setError(null)
    startTransition(async () => {
      try {
        const response = await discoverExternalMcpServerOauth({ data: { serverUrl: trimmedUrl } })
        setResult(response.data)
      } catch (discoveryError) {
        setResult(null)
        setError(
          discoveryError instanceof Error ? discoveryError.message : 'OAuth discovery failed',
        )
      }
    })
  }

  function apply(discovery: McpOauthDiscoveryPayload) {
    onApply(discovery.suggested_auth_config)
    toast.success('Discovered OAuth settings applied. Review the auth config before saving.')
  }

  return (
    <div className="flex min-w-0 flex-col gap-3" data-testid="mcp-oauth-discovery">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-muted-foreground text-sm">
          Read the server&apos;s OAuth metadata to fill in per-user OAuth settings.
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!trimmedUrl || isPending}
          onClick={discover}
        >
          {isPending ? 'Discovering…' : 'Discover OAuth'}
        </Button>
      </div>
      {error ? (
        <Alert variant="destructive">
          <AlertTitle>OAuth discovery failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      ) : null}
      {result ? <DiscoveryResult discovery={result} onApply={() => apply(result)} /> : null}
    </div>
  )
}

function DiscoveryResult({
  discovery,
  onApply,
}: {
  discovery: McpOauthDiscoveryPayload
  onApply: () => void
}) {
  const scopes = discovery.default_scopes.length
    ? discovery.default_scopes.join(' ')
    : 'Provider default'
  const rows: Array<[string, string]> = [
    ['Resource', discovery.resource],
    ['Authorization server', discovery.issuer],
    ['Authorization endpoint', discovery.authorization_endpoint],
    ['Token endpoint', discovery.token_endpoint],
    ['Registration endpoint', discovery.registration_endpoint ?? 'Not offered'],
    ['Default scopes', scopes],
    ['Resource indicator', discovery.resource_indicator ? 'Sent' : 'Not sent'],
  ]
  if (discovery.callback_url) {
    rows.push(['Callback URL', discovery.callback_url])
  }
  return (
    <div className="border-border flex min-w-0 flex-col gap-3 border-t pt-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="secondary">
          {REGISTRATION_LABELS[discovery.recommended_client_registration] ??
            discovery.recommended_client_registration}
        </Badge>
        {discovery.resource_metadata_url ? (
          <Badge variant="outline">Protected-resource metadata</Badge>
        ) : null}
      </div>
      <dl className="divide-border grid min-w-0 divide-y text-xs">
        {rows.map(([label, value]) => (
          <div key={label} className="grid min-w-0 gap-1 py-1.5 sm:grid-cols-3">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="font-mono break-all sm:col-span-2">{value}</dd>
          </div>
        ))}
      </dl>
      {discovery.warnings.length ? (
        <Alert>
          <AlertTitle>Check before saving</AlertTitle>
          <AlertDescription>
            <ul className="list-disc space-y-1 pl-4">
              {discovery.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      ) : null}
      <div className="flex justify-end">
        <Button type="button" size="sm" onClick={onApply}>
          Use discovered settings
        </Button>
      </div>
    </div>
  )
}
