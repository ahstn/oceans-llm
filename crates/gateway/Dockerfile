ARG RUST_VERSION=1.93.1

FROM rust:${RUST_VERSION}-slim-bookworm AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends build-essential ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN cargo build --release -p gateway

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/gateway /usr/local/bin/gateway

EXPOSE 8080

CMD ["gateway"]
