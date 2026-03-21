FROM rust:1.90-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release -p nanobot-cli

FROM node:20-bookworm-slim AS bridge-builder

WORKDIR /app/bridge

COPY bridge/package.json bridge/tsconfig.json ./
COPY bridge/src ./src

RUN npm install && npm run build

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/nanobot-cli /usr/local/bin/nanobot-cli
COPY --from=bridge-builder /app/bridge /app/bridge

RUN mkdir -p /root/.nanobot-rs

EXPOSE 18790

ENTRYPOINT ["nanobot-cli"]
CMD ["status"]
