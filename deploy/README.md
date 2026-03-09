# Deploy Compose

This directory contains the user-facing Docker Compose setup that pulls the published images from GHCR:

- `ghcr.io/ahstn/oceans-llm-gateway`
- `ghcr.io/ahstn/oceans-llm-admin-ui`

## Files

- `compose.yaml`: compose stack for gateway, admin UI, and Postgres.
- `config/gateway.yaml`: example gateway config mounted into the gateway container.
- `.env.example`: example environment values for image tag selection and secrets.

## Usage

```bash
cp deploy/.env.example deploy/.env
docker compose -f deploy/compose.yaml up -d
```

The gateway is published on `http://localhost:8080` by default, and the admin UI is available through the gateway at `/admin`.

## Database note

The stack includes Postgres because the deployment shape requested a Postgres service, but the current gateway runtime still uses a local libsql/SQLite database file configured at `/data/gateway.db`. The mounted `config/gateway.yaml` reflects the current runtime behavior.
