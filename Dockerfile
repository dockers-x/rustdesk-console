# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Stage 1: build the admin web UI (lejianwen/rustdesk-api-web) so it can be
# embedded into the binary. The output `dist/` becomes resources/admin.
# ---------------------------------------------------------------------------
FROM node:20-alpine AS webui
ARG WEBCLIENT_SOURCE=https://github.com/lejianwen/rustdesk-api-web
RUN apk add --no-cache git
WORKDIR /web
RUN git clone --depth=1 "${WEBCLIENT_SOURCE}" . \
    && (corepack enable || true) \
    && (yarn install --frozen-lockfile || yarn install || npm ci || npm install) \
    && (yarn build || npm run build)

# ---------------------------------------------------------------------------
# Stage 2: build the Rust binary with all assets embedded (rust-embed).
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS builder
WORKDIR /app
COPY . .
# Drop in the freshly-built admin UI so it is embedded at compile time.
RUN rm -rf resources/admin && mkdir -p resources/admin
COPY --from=webui /web/dist/ ./resources/admin/
RUN cargo build --release -p rustdesk-api-server

# ---------------------------------------------------------------------------
# Stage 3: minimal runtime. The binary is self-contained (frontend + i18n +
# templates are embedded); only config and the sqlite data dir live on disk.
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/rustdesk-api-server /app/rustdesk-api-server
COPY --from=builder /app/conf /app/conf
RUN mkdir -p /app/data /app/runtime
VOLUME /app/data
EXPOSE 21114
CMD ["./rustdesk-api-server", "-c", "./conf/config.yaml"]
