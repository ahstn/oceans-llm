# ADR: SSRF-Safe Outbound Fetch for Client-Chosen URLs

- Date: 2026-10-17
- Status: Accepted
- Issues: #441 (Client ID Metadata Documents), #442 (upstream MCP OAuth discovery)

## Current state

- [MCP Client Setup: OAuth sign-in](../mcp/mcp-client-setup.md#oauth-sign-in)
- [MCP Servers: OAuth for Any MCP Server](../configuration/mcp-servers.md#oauth-for-any-mcp-server)

## Context

Two features make the gateway request URLs that someone other than an admin chose:

- **Upstream OAuth discovery (#442).** An admin enters an MCP server URL. The server's `401` challenge and metadata then name further URLs: protected-resource metadata, the authorization server, and its registration endpoint. A compromised or hostile server controls those.
- **Client ID Metadata Documents (#441).** An MCP client's `client_id` is a URL, and the gateway fetches it during an unauthenticated authorization request.

Discovery already required https, set a timeout and a size limit, and followed no redirects. It did not check where a host resolved. A public hostname that resolves to `169.254.169.254` or `10.0.0.5` would still be fetched. Without that check, either feature lets an outside party make the gateway read cloud credentials or call internal services (server-side request forgery).

## Decision

`gateway_service::outbound_fetch::OutboundFetcher` is the one HTTP client for these requests. Both features use it.

### 1. Address scopes

Each fetcher has a scope that sets which addresses it may connect to. Each scope allows everything the previous one does.

| Scope | Allows |
| --- | --- |
| `Public` | Globally routable addresses only |
| `PrivateNetwork` | Also RFC 1918, carrier-grade NAT (`100.64.0.0/10`), and IPv6 unique-local (`fc00::/7`) |
| `Loopback` | Also `127.0.0.0/8` and `::1`, and plain `http` to a loopback host |

No scope reaches:

- link-local addresses (`169.254.0.0/16`, `fe80::/10`), where AWS, GCP, Azure, and Oracle serve instance metadata
- the metadata addresses outside that range: `100.100.100.200` (Alibaba), `168.63.129.16` (Azure WireServer), and `fd00:ec2::254` (AWS over IPv6)
- unspecified, broadcast, multicast, documentation, benchmarking, and reserved ranges, and Teredo

IPv6 forms that embed an IPv4 address (mapped `::ffff:0:0/96`, NAT64 `64:ff9b::/96`, and 6to4 `2002::/16`) are judged by the embedded address. Mapped `::ffff:169.254.169.254` is therefore blocked, as is 6to4 `2002:a9fe:a9fe::`.

### 2. The check happens at resolution, pinned to the connection

A check on the URL alone fails against DNS rebinding: the name resolves to a public address when checked and to an internal one when connected. The fetcher installs its own DNS resolver in the HTTP client. The resolver looks the name up, rejects it if any answer is outside the scope, and hands back exactly the addresses it checked. The client connects only to those. One disallowed answer rejects the whole name, since a host that mixes public and internal addresses is not one to trust with either.

IP-literal hosts never reach a resolver, so `check_url` classifies them before the request. It also requires https, and allows `http` only to a loopback host in the `Loopback` scope. It rejects user information in the URL.

### 3. Nothing moves the connection after the check

- Redirects are off. A `302` is returned to the caller as a response, never followed.
- Proxies are off, including proxy environment variables, because a proxy resolves the name itself.
- Every request has a connect timeout and a total timeout, 10 seconds by default. The body is read only on request, and reading stops at a size limit, 256 KiB by default. A `Content-Length` over the limit is refused before reading.

### 4. How each feature picks a scope

- **Client ID Metadata Documents** always use `Public`, with a 5 second timeout and a 64 KiB limit. The URL comes from an unauthenticated caller.
- **Upstream discovery and dynamic registration** use the scope of the MCP server the admin entered, from `OutboundAddressScope::of_trusted_url`. That function resolves the server's host and picks `Loopback`, `PrivateNetwork`, or `Public` to match its address. An admin who registers a server on a private network has already chosen to trust that network, and its authorization server often lives there too. A public server cannot point the gateway at private or loopback addresses. Registration at connect time uses the scope of the server's saved `resource` URL.

## Trade-offs

- A gateway that reaches the internet only through an HTTP proxy cannot fetch metadata documents or run discovery against public servers. Following a proxy would hand resolution to it and lose the address check.
- Scoping discovery by the admin-entered server keeps internal deployments working, but an internal server can still point discovery at other internal hosts on the same class of network. Link-local and metadata addresses stay blocked for it.
- Each lookup happens twice when the scope is derived from the server URL: once to pick the scope, once in the resolver. Discovery and registration are rare admin-time and connect-time operations, so this costs little.
- The classification table is maintained by hand. New special-purpose ranges need adding as they are assigned.

## Follow-ups

- Route upstream token and refresh requests through the fetcher too. Their endpoints come from discovery, but an admin reviews and saves them, so they still use the plain client today.
- Use the fetcher for any future feature that fetches a URL a user or upstream supplies, such as logos named in client metadata.
