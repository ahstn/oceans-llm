# Oceans LLM Gateway

Rust-first gateway workspace with an embedded `admin-ui` crate that hosts a TanStack Start control-plane UI.

## Workspace layout

- `crates/gateway`: Rust API front door (`/api/*`, `/healthz`)
- `crates/admin-ui`: Rust reverse proxy integration for `/admin*`
- `crates/admin-ui/web`: TanStack Start + React + shadcn-style UI implementation

## Runtime model

Single-container dual process:

1. Gateway (Rust) listens on `PORT` (default `8080`)
2. Admin UI SSR process (Bun/TanStack Start) runs on internal `3001`
3. Gateway reverse-proxies `/admin*` to `ADMIN_UI_UPSTREAM`

## Environment

- `PORT`: Gateway bind port (default `8080`)
- `ADMIN_UI_BASE_PATH`: UI mount path (default `/admin`)
- `ADMIN_UI_UPSTREAM`: SSR upstream URL (default `http://127.0.0.1:3001`)
- `ADMIN_UI_CONNECT_TIMEOUT_MS`: Proxy connect timeout (default `750`)
- `ADMIN_UI_REQUEST_TIMEOUT_MS`: Proxy request timeout (default `10000`)
- `ADMIN_UI_INTERNAL_PORT`: Internal Bun SSR port used by helper scripts (default `3001`)

## Setup

```bash
eval "$(/Users/ahstn/.local/bin/mise activate zsh)"
mise install
mise run ui-install
```

## Development

Run both UI and gateway together:

```bash
./scripts/start-dev-stack.sh
```

- Gateway/API: `http://localhost:8080`
- Admin UI: `http://localhost:8080/admin`

## Production-style local run

```bash
mise run ui-build
./scripts/start-prod.sh
```

## Quality gates

```bash
mise run check
mise run lint
mise run test
```
