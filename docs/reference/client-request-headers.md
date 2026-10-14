# Client Request Headers

`See also`: [Tagging](../operations/tagging.md), [Client Harness Configuration](../configuration/client-harness-configuration.md), [Model Routing and APIs](../configuration/model-routing-and-api-behavior.md#keep-sessions-on-one-route), [Observability and Request Logs](../operations/observability-and-request-logs.md), [Request Lifecycle and Failure Modes](request-lifecycle-and-failure-modes.md)

This page lists the request headers that callers use to authenticate, tag, and group data-plane requests, and explains how the gateway handles alias headers.

Oceans accepts some `x-litellm-*` headers as aliases for its own headers. Callers that already send them, such as Claude Code installs configured for a LiteLLM-style proxy, can use Oceans without changing their client configuration. The aliases are input-only: Oceans never sends `x-litellm-*` response headers.

## Source of Truth

- Credential precedence: [../crates/gateway-core/src/auth.rs](../../crates/gateway-core/src/auth.rs)
- HTTP credential extraction: [../crates/gateway/src/http/credentials.rs](../../crates/gateway/src/http/credentials.rs)
- Request tags: [../crates/gateway/src/http/request_tags.rs](../../crates/gateway/src/http/request_tags.rs)
- Routing sessions: [../crates/gateway-service/src/client_session.rs](../../crates/gateway-service/src/client_session.rs)

## Header Summary

| Purpose | Oceans header | Alias | Notes |
| --- | --- | --- | --- |
| Gateway API key | `Authorization: Bearer <key>` | `x-litellm-api-key` | The alias accepts `<key>` or `Bearer <key>`. |
| Gateway API key, Anthropic Messages | `x-api-key` | `x-litellm-api-key` | `x-api-key` is accepted on `/v1/messages`, `/v1/models`, and `/v1/model-metadata`. |
| Gateway API key, MCP, batches, and skills | `x-oceans-api-key` | `x-litellm-api-key` | `x-oceans-api-key` must match any other key header that is sent. |
| Bespoke request tags | `x-oceans-tags` | `x-litellm-tags` | See [Request tags](#request-tags). |
| Universal request tags | `x-oceans-service`, `x-oceans-component`, `x-oceans-env` | none | |
| Routing session | `x-oceans-session-id` | `x-litellm-session-id` | See [Routing sessions](#routing-sessions). |
| Request id | `x-request-id` | none | |

Header names are case-insensitive. Each header listed here may be sent at most once.

## Credential Precedence

Every route that accepts a gateway API key uses one precedence order:

1. `x-litellm-api-key`, when present.
2. `Authorization: Bearer <key>`.
3. `x-oceans-api-key`, on MCP, batches, and skills routes.
4. `x-api-key`, on Anthropic Messages and model discovery routes.

`x-oceans-api-key` is not read on inference routes.

When `x-litellm-api-key` is present it is the gateway credential, and the gateway does not read `Authorization` or `x-api-key`. Clients such as Claude Code may put an unrelated upstream OAuth token or provider key in those headers, so they do not cause a conflict.

`x-oceans-api-key` is an explicit gateway credential. If it is sent with `x-litellm-api-key` or `Authorization`, both values must be the same key; otherwise the gateway returns `401`.

A malformed credential header fails authentication. The gateway does not fall back to a lower-precedence header. For example, `x-litellm-api-key: Bearer` with no key returns `401` even when `Authorization` holds a valid key.

Client credential headers are never forwarded to upstream providers. Request-log payloads drop them, and the redaction policy treats `x-oceans-api-key` and `x-litellm-api-key` as sensitive.

## Request Tags

`x-litellm-tags` is a comma-separated list. Each entry becomes one bespoke request tag:

| Entry | Stored tag |
| --- | --- |
| `team=payments` | `team=payments` |
| `team:payments` | `team=payments` |
| `batch` | `batch=true` |

An entry is split on its first `=`, or on its first `:` when it has no `=`. Entries are trimmed, empty entries are ignored, and repeated identical entries count once. The resulting tags follow the same [validation rules](../operations/tagging.md#validation-rules) as `x-oceans-tags`, and failures return the same `invalid_request` errors.

When a request sends both headers, the tags form one set:

- A tag sent on both headers with the same value is stored once.
- A key sent with different values returns `400`. The gateway does not choose a winner.
- The combined set may contain at most 5 bespoke tags.

```text
x-oceans-tags: cost-center=platform
x-litellm-tags: workload:agent-harness, batch
```

That request stores `cost-center=platform`, `workload=agent-harness`, and `batch=true`.

## Routing Sessions

`x-litellm-session-id` is an alias for `x-oceans-session-id`. Both select the Oceans session namespace for [session affinity](../configuration/model-routing-and-api-behavior.md#keep-sessions-on-one-route). On a model with affinity enabled, a request that sends both, or sends either with a recognized harness session id, must use the same value; otherwise the gateway returns `400` with `conflicting routing session identifiers`.

Like `x-oceans-session-id`, the alias affects routing only. Agent Session Analysis groups requests by the harness-specific session identifiers.

## Request-Log Attribution

Request-log rows record which header supplied each value in `metadata_json.header_sources`. Values are never stored there, only header names:

```json
{
  "header_sources": {
    "credential": "x-litellm-api-key",
    "tags": "x-oceans-tags+x-litellm-tags",
    "session": "x-litellm-session-id"
  }
}
```

`tags` and `session` appear only when the request sent those headers. When a request sends several headers for one value, their names are joined with `+`.
