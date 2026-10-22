# syntax=docker/dockerfile:1.7

# ---------- 1. Web assets (admin UI + widget) ----------
FROM node:24-slim AS web
ENV PNPM_HOME=/pnpm \
    PATH=/pnpm:$PATH \
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 \
    COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN corepack enable
WORKDIR /app

# Manifests first so the dependency layer is cached until a lockfile or manifest changes.
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/admin/package.json apps/admin/
COPY packages/widget/package.json packages/widget/
COPY e2e/package.json e2e/
RUN pnpm install --frozen-lockfile

COPY apps/admin apps/admin
COPY packages/widget packages/widget
RUN pnpm build:web

# ---------- 2. Rust binary ----------
FROM rust:1-slim-bookworm AS server
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake perl pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Build dependencies against a dummy crate so they stay cached across source edits.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/server/Cargo.toml crates/server/
RUN mkdir -p crates/server/src \
    && echo 'fn main() {}' > crates/server/src/main.rs \
    && echo '' > crates/server/src/lib.rs \
    && cargo build --release --locked -p feedback-harvest \
    && rm -rf crates/server/src

# rust-embed bakes these folders into the release binary at compile time.
COPY --from=web /app/apps/admin/dist apps/admin/dist
COPY --from=web /app/packages/widget/dist packages/widget/dist
COPY crates/server crates/server
RUN touch crates/server/src/main.rs crates/server/src/lib.rs \
    && cargo build --release --locked -p feedback-harvest \
    && cp target/release/feedback-harvest /feedback-harvest

# Data dir owned by the distroless nonroot user (uid/gid 65532).
RUN mkdir -p /out/data && chown 65532:65532 /out/data

# ---------- 3. Runtime ----------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=server /feedback-harvest /feedback-harvest
COPY --from=server --chown=65532:65532 /out/data /data

# 0.0.0.0 here is the in-container bind only. Coolify publishes no host port and
# reaches the app over its private Docker network; docker-compose.yml publishes
# to 127.0.0.1 only. Never publish this port on a public interface.
ENV FH_BIND=0.0.0.0:8080 \
    FH_DATABASE_PATH=/data/feedback-harvest.db \
    FH_TRUSTED_PROXY_HOPS=1

VOLUME /data
# Documentation only: EXPOSE does not publish anything.
EXPOSE 8080
USER nonroot
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/feedback-harvest", "healthcheck"]
ENTRYPOINT ["/feedback-harvest"]
CMD ["serve"]
