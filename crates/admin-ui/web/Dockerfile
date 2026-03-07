ARG BUN_VERSION=1.3.10

FROM oven/bun:${BUN_VERSION} AS builder

WORKDIR /app

COPY crates/admin-ui/web/package.json ./package.json
COPY crates/admin-ui/web/bun.lock ./bun.lock

RUN bun install --frozen-lockfile

COPY crates/admin-ui/web/ ./

RUN bun run build

FROM oven/bun:${BUN_VERSION}-slim AS runtime

WORKDIR /app

ENV PORT=3000

COPY --from=builder /app/package.json ./package.json
COPY --from=builder /app/bun.lock ./bun.lock

RUN bun install --frozen-lockfile --production

COPY --from=builder /app/dist ./dist
COPY --from=builder /app/server.ts ./server.ts

EXPOSE 3000

CMD ["bun", "run", "server.ts"]
