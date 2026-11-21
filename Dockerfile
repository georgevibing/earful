# syntax=docker/dockerfile:1.7

# ---------- 1. Web assets (admin UI + widget) ----------
FROM node:24-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54 AS web
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
FROM rust:1-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS server
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
    && cargo build --release --locked -p earful \
    && rm -rf crates/server/src

# rust-embed bakes these folders into the release binary at compile time.
COPY --from=web /app/apps/admin/dist apps/admin/dist
COPY --from=web /app/packages/widget/dist packages/widget/dist
COPY crates/server crates/server
RUN touch crates/server/src/main.rs crates/server/src/lib.rs \
    && cargo build --release --locked -p earful \
    && cp target/release/earful /earful

# Data dir owned by the distroless nonroot user (uid/gid 65532).
RUN mkdir -p /out/data && chown 65532:65532 /out/data

# ---------- 3. Embedding model (Model2Vec potion-base-8M, MIT, ~31 MB) ----------
# Pinned revision and sha256 so builds are reproducible and tamper evident.
# Independent of the source tree, so this layer stays cached across code edits.
FROM debian:bookworm-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc AS model
ARG MODEL_REPO=minishlab/potion-base-8M
ARG MODEL_REVISION=bf8b056651a2c21b8d2565580b8569da283cab23
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /models/potion-base-8M
RUN for f in config.json model.safetensors tokenizer.json; do \
        curl -fsSL --retry 3 -o "$f" "https://huggingface.co/${MODEL_REPO}/resolve/${MODEL_REVISION}/$f"; \
    done \
    && printf '%s  %s\n' \
        2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553 config.json \
        f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2 model.safetensors \
        e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50 tokenizer.json \
        | sha256sum -c - \
    && chmod 0444 ./*

# ---------- 4. Runtime ----------
FROM gcr.io/distroless/cc-debian12:nonroot@sha256:9dac0a79194e45a7da0158a9c6da57b217585af0786db3845d1f0ec1a0dd182f
COPY --from=server /earful /earful
COPY --from=server --chown=65532:65532 /out/data /data
# Read only for everyone; the server never writes to the model dir.
COPY --from=model /models /models

# 0.0.0.0 here is the in-container bind only. Coolify publishes no host port and
# reaches the app over its private Docker network; docker-compose.yml publishes
# to 127.0.0.1 only. Never publish this port on a public interface.
ENV EARFUL_BIND=0.0.0.0:8080 \
    EARFUL_DATABASE_PATH=/data/earful.db \
    EARFUL_TRUSTED_PROXY_HOPS=1 \
    EARFUL_EMBED_MODEL_PATH=/models/potion-base-8M

VOLUME /data
# Documentation only: EXPOSE does not publish anything.
EXPOSE 8080
USER nonroot
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/earful", "healthcheck"]
ENTRYPOINT ["/earful"]
CMD ["serve"]
