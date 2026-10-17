import { Checkbox } from '@/components/ui/checkbox'
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from '@/components/ui/field'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import {
  type ApiKeyScopesValue,
  SCOPE_DETAILS,
  allowsMcp,
  scopeLabel,
  toggleValue,
} from '@/lib/api-key-scopes'
import type { ApiKeyMcpServerOptionView } from '@/types/api'

/** Capabilities and MCP servers for a key, shared by the create and manage dialogs. */
export function ApiKeyScopeFields({
  availableScopes,
  disabled,
  idPrefix,
  mcpServers,
  value,
  onChange,
}: {
  availableScopes: string[]
  disabled: boolean
  idPrefix: string
  mcpServers: ApiKeyMcpServerOptionView[]
  value: ApiKeyScopesValue
  onChange: (value: ApiKeyScopesValue) => void
}) {
  const selectedScopes = new Set(value.scopes)
  const setScopes = (scopes: string[] | null) => {
    const next = { ...value, scopes }
    // A server list needs an MCP scope, so drop it with the last one.
    onChange(allowsMcp(next) ? next : { ...next, mcp_server_keys: null })
  }

  return (
    <>
      <Field>
        <FieldLabel>Capabilities</FieldLabel>
        <ToggleGroup
          type="single"
          value={value.scopes === null ? 'full' : 'limited'}
          onValueChange={(mode) => {
            if (mode === 'full') {
              setScopes(null)
            } else if (mode === 'limited' && value.scopes === null) {
              setScopes([])
            }
          }}
          disabled={disabled}
          aria-label="Capabilities"
        >
          <ToggleGroupItem value="full" aria-label="Full access">
            Full access
          </ToggleGroupItem>
          <ToggleGroupItem value="limited" aria-label="Limited">
            Limited
          </ToggleGroupItem>
        </ToggleGroup>
        <FieldDescription>
          {value.scopes === null
            ? 'This key can call every route its owner can, including Skills and provider credentials.'
            : 'This key can only call the routes for the capabilities selected below.'}
        </FieldDescription>
      </Field>

      {value.scopes !== null ? (
        <FieldSet>
          <FieldLegend variant="label">Scopes</FieldLegend>
          <FieldGroup data-slot="checkbox-group" className="gap-3">
            {availableScopes.map((scope) => (
              <FieldLabel key={scope} htmlFor={`${idPrefix}-scope-${scope}`}>
                <Field orientation="horizontal" data-disabled={disabled}>
                  <Checkbox
                    id={`${idPrefix}-scope-${scope}`}
                    checked={selectedScopes.has(scope)}
                    disabled={disabled}
                    onCheckedChange={(on) =>
                      setScopes(toggleValue(value.scopes ?? [], scope, on === true))
                    }
                  />
                  <FieldContent>
                    <FieldTitle>
                      {scopeLabel(scope)}{' '}
                      <code className="text-muted-foreground text-xs">{scope}</code>
                    </FieldTitle>
                    <FieldDescription>{SCOPE_DETAILS[scope]?.description}</FieldDescription>
                  </FieldContent>
                </Field>
              </FieldLabel>
            ))}
          </FieldGroup>
        </FieldSet>
      ) : null}

      {allowsMcp(value) ? (
        <McpServerListField
          disabled={disabled}
          idPrefix={idPrefix}
          mcpServers={mcpServers}
          value={value.mcp_server_keys}
          onChange={(mcp_server_keys) => onChange({ ...value, mcp_server_keys })}
        />
      ) : null}
    </>
  )
}

function McpServerListField({
  disabled,
  idPrefix,
  mcpServers,
  value,
  onChange,
}: {
  disabled: boolean
  idPrefix: string
  mcpServers: ApiKeyMcpServerOptionView[]
  value: string[] | null
  onChange: (value: string[] | null) => void
}) {
  // A saved key may list a server that has since been disabled; keep it visible.
  const listed = new Set(mcpServers.map((server) => server.key))
  const options = [...mcpServers]
  for (const key of value ?? []) {
    if (!listed.has(key)) {
      options.push({ key, display_name: `${key} (disabled)` })
    }
  }
  const selected = new Set(value)

  return (
    <Field>
      <FieldLabel>MCP servers</FieldLabel>
      <ToggleGroup
        type="single"
        value={value === null ? 'all' : 'selected'}
        onValueChange={(mode) => {
          if (mode === 'all') {
            onChange(null)
          } else if (mode === 'selected' && value === null) {
            onChange([])
          }
        }}
        disabled={disabled || (options.length === 0 && value === null)}
        aria-label="MCP servers"
      >
        <ToggleGroupItem value="all" aria-label="All granted servers">
          All granted servers
        </ToggleGroupItem>
        <ToggleGroupItem value="selected" aria-label="Selected servers">
          Selected servers
        </ToggleGroupItem>
      </ToggleGroup>
      {value !== null ? (
        <FieldGroup data-slot="checkbox-group" className="gap-2">
          {options.map((server) => (
            <Field key={server.key} orientation="horizontal" data-disabled={disabled}>
              <Checkbox
                id={`${idPrefix}-server-${server.key}`}
                checked={selected.has(server.key)}
                disabled={disabled}
                onCheckedChange={(on) => onChange(toggleValue(value, server.key, on === true))}
              />
              <FieldLabel htmlFor={`${idPrefix}-server-${server.key}`} className="font-normal">
                {server.display_name}{' '}
                <code className="text-muted-foreground text-xs">{server.key}</code>
              </FieldLabel>
            </Field>
          ))}
        </FieldGroup>
      ) : null}
      <FieldDescription>
        {options.length === 0
          ? 'No MCP servers are registered.'
          : 'The owner still needs a grant before a listed server shows any tools.'}
      </FieldDescription>
    </Field>
  )
}
