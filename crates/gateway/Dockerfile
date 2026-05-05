# syntax=docker/dockerfile:1.7

ARG RUST_VERSION=1.93.1

FROM rust:${RUST_VERSION}-slim-bookworm AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends build-essential ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --locked --release -p gateway \
    && cp /app/target/release/gateway /usr/local/bin/gateway

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin gateway

WORKDIR /app

COPY --from=builder /usr/local/bin/gateway /usr/local/bin/gateway

USER gateway

EXPOSE 8080

CMD ["gateway"]
