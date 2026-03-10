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

## Database policy

- This deploy stack uses PostgreSQL by default through `POSTGRES_URL`.
- Local libsql/SQLite remains the supported lightweight option for plain local development, but not for production or pre-production deploys.
- The gateway applies migrations and idempotent startup seed data on boot; the Postgres container is intentionally not preloaded with application rows through `docker-entrypoint-initdb.d`.

The mounted `config/gateway.yaml` reflects the production-shaped runtime behavior.
