# syntax=docker/dockerfile:1.7

ARG BUN_VERSION=1.3.10

FROM oven/bun:${BUN_VERSION} AS deps

WORKDIR /app

COPY crates/admin-ui/web/package.json ./package.json
COPY crates/admin-ui/web/bun.lock ./bun.lock

RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun install --frozen-lockfile

FROM oven/bun:${BUN_VERSION} AS builder

WORKDIR /app

ENV NODE_ENV=production

COPY --from=deps /app/node_modules ./node_modules
COPY crates/admin-ui/web/ ./

RUN bun run build

FROM oven/bun:${BUN_VERSION} AS prod-deps

WORKDIR /app

ENV NODE_ENV=production

COPY crates/admin-ui/web/package.json ./package.json
COPY crates/admin-ui/web/bun.lock ./bun.lock

RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun install --frozen-lockfile --production

FROM oven/bun:${BUN_VERSION}-slim AS runtime

WORKDIR /app

ENV NODE_ENV=production
ENV PORT=3000

RUN chown bun:bun /app

COPY --from=prod-deps --chown=bun:bun /app/package.json ./package.json
COPY --from=prod-deps --chown=bun:bun /app/bun.lock ./bun.lock
COPY --from=prod-deps --chown=bun:bun /app/node_modules ./node_modules
COPY --from=builder --chown=bun:bun /app/dist ./dist
COPY --from=builder --chown=bun:bun /app/server.ts ./server.ts

USER bun

EXPOSE 3000

CMD ["bun", "server.ts"]
