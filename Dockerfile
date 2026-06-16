# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Stage 1: build the React + kumo admin web UI (web/) so it can be embedded
# into the binary. Output lands in resources/admin.
# ---------------------------------------------------------------------------
FROM node:20-bookworm-slim AS webui
WORKDIR /app
COPY web ./web
RUN cd web \
    && npm install --no-audit --no-fund \
    && npx vite build
# vite outDir is ../resources/admin -> /app/resources/admin

# ---------------------------------------------------------------------------
# Stage 2: build the Rust binary with all assets embedded (rust-embed).
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
COPY --from=webui /app/resources/admin ./resources/admin
RUN cargo build --release -p rustdesk-console

# ---------------------------------------------------------------------------
# Stage 3: minimal runtime. The binary is self-contained (frontend + i18n +
# templates are embedded); only config and the sqlite data dir live on disk.
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/rustdesk-console /app/rustdesk-console
COPY --from=builder /app/conf /app/conf
RUN mkdir -p /app/data /app/runtime
VOLUME /app/data
EXPOSE 21114
CMD ["./rustdesk-console", "-c", "./conf/config.yaml"]
