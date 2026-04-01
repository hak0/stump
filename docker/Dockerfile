# syntax=docker/dockerfile:1.7

# ------------------------------------------------------------------------------
# PDFium Stage
# ------------------------------------------------------------------------------

FROM debian:bookworm-slim AS pdfium

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends ca-certificates curl tar && \
    rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    mkdir -p /pdfium; \
    case "$(dpkg --print-architecture)" in \
        amd64) curl -fsSL -o /tmp/pdfium.tgz https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/6707/pdfium-linux-x64.tgz ;; \
        arm64) curl -fsSL -o /tmp/pdfium.tgz https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/6707/pdfium-linux-arm64.tgz ;; \
        *) echo "Unsupported architecture: $(dpkg --print-architecture)" >&2; exit 1 ;; \
    esac; \
    tar -xzf /tmp/pdfium.tgz -C /pdfium; \
    rm /tmp/pdfium.tgz

RUN set -eux; \
    case "$(dpkg --print-architecture)" in \
        amd64) echo "e45484bfde8165f566633bc863564fdbd964bd87  /pdfium/lib/libpdfium.so" | sha1sum -c - ;; \
        arm64) echo "6519048a851f97ec86c498d827cf31efe453287b  /pdfium/lib/libpdfium.so" | sha1sum -c - ;; \
    esac

# ------------------------------------------------------------------------------
# Frontend Dependency Stage
# ------------------------------------------------------------------------------

FROM node:20.9.0-alpine3.18 AS frontend-deps
ARG TARGETARCH

WORKDIR /app

RUN apk add --no-cache git

COPY package.json yarn.lock lerna.json .npmrc tsconfig.json tsconfig.options.json tsconfig.eslint.json prettier.config.js ./
COPY apps/desktop/package.json apps/desktop/package.json
COPY apps/expo/package.json apps/expo/package.json
COPY apps/expo/global.css apps/expo/global.css
COPY apps/expo/tailwind.config.js apps/expo/tailwind.config.js
COPY apps/web/package.json apps/web/package.json
COPY docs/package.json docs/package.json
COPY packages/browser/package.json packages/browser/package.json
COPY packages/client/package.json packages/client/package.json
COPY packages/components/package.json packages/components/package.json
COPY packages/i18n/package.json packages/i18n/package.json
COPY packages/sdk/package.json packages/sdk/package.json

RUN --mount=type=cache,target=/usr/local/share/.cache/yarn,id=yarn-${TARGETARCH},sharing=locked \
    yarn config set network-timeout 300000 && \
    YARN_CACHE_FOLDER=/usr/local/share/.cache/yarn yarn install --frozen-lockfile

# ------------------------------------------------------------------------------
# Frontend Build Stage
# ------------------------------------------------------------------------------

FROM frontend-deps AS frontend

COPY apps/web apps/web
COPY packages/browser packages/browser
COPY packages/client packages/client
COPY packages/components packages/components
COPY packages/i18n packages/i18n
COPY packages/sdk packages/sdk

RUN yarn web build && \
    mv ./apps/web/dist ./build && \
    test -d ./build && test -n "$(ls -A ./build)"

# ------------------------------------------------------------------------------
# Cargo Tooling Stage
# ------------------------------------------------------------------------------

FROM rust:1.85-bookworm AS cargo-tools
ARG TARGETARCH
ARG CARGO_CHEF_VERSION=0.1.70
ARG SCCACHE_VERSION=0.14.0

ENV CARGO_HOME=/usr/local/cargo \
    CARGO_NET_GIT_FETCH_WITH_CLI=true

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,id=cargo-git-db-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/checkouts,id=cargo-git-checkouts-${TARGETARCH},sharing=locked \
    cargo install --locked --root /opt/cargo-tools cargo-chef --version ${CARGO_CHEF_VERSION} && \
    cargo install --locked --root /opt/cargo-tools sccache --version ${SCCACHE_VERSION}

# ------------------------------------------------------------------------------
# Rust Base Stage
# ------------------------------------------------------------------------------

FROM rust:1.85-bookworm AS rust-base
ARG TARGETARCH

ENV CARGO_HOME=/usr/local/cargo \
    CARGO_NET_GIT_FETCH_WITH_CLI=true \
    SCCACHE_DIR=/var/cache/sccache

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        cmake \
        curl \
        git \
        libsqlite3-dev \
        libssl-dev \
        pkg-config \
        wget && \
    rm -rf /var/lib/apt/lists/*

COPY --from=cargo-tools /opt/cargo-tools/bin/cargo-chef /usr/local/cargo/bin/cargo-chef
COPY --from=cargo-tools /opt/cargo-tools/bin/sccache /usr/local/cargo/bin/sccache

WORKDIR /app

ENV RUSTC_WRAPPER=/usr/local/cargo/bin/sccache

# ------------------------------------------------------------------------------
# Cargo Planner Stage
# ------------------------------------------------------------------------------

FROM rust-base AS planner

COPY . .

RUN ./scripts/release/utils.sh -w && \
    cargo chef prepare --recipe-path recipe.json

# ------------------------------------------------------------------------------
# Cargo Dependency Cache Stage
# ------------------------------------------------------------------------------

FROM rust-base AS cacher
ARG TARGETARCH

COPY --from=planner /app/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,id=cargo-git-db-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/checkouts,id=cargo-git-checkouts-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/app/target,id=rust-target-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/var/cache/sccache,id=sccache-${TARGETARCH},sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

# ------------------------------------------------------------------------------
# Cargo Build Stage
# ------------------------------------------------------------------------------

FROM rust-base AS builder

ARG GIT_REV
ARG TARGETARCH
ARG RUN_PRISMA_GENERATE
ARG PRISMA_SCHEMA=./core/prisma/schema.sqlite.prisma

ENV GIT_REV=${GIT_REV} \
    RUN_PRISMA_GENERATE=${RUN_PRISMA_GENERATE} \
    PRISMA_SCHEMA=${PRISMA_SCHEMA}

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-registry-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/db,id=cargo-git-db-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git/checkouts,id=cargo-git-checkouts-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/app/target,id=rust-target-${TARGETARCH},sharing=locked \
    --mount=type=cache,target=/var/cache/sccache,id=sccache-${TARGETARCH},sharing=locked \
    ./docker/build_server.sh && \
    cp ./target/release/stump_server ./stump_server

# ------------------------------------------------------------------------------
# Final Stage
# ------------------------------------------------------------------------------

FROM gcr.io/distroless/cc-debian12:debug

RUN ["/busybox/ln", "-s", "/busybox/sh", "/bin/sh"]
RUN ln -s /busybox/env /usr/bin/env

COPY --from=builder /app/stump_server /app/stump
COPY --from=pdfium /pdfium /opt/pdfium
COPY --from=frontend /app/build /app/client
COPY --chmod=755 docker/entrypoint.sh /entrypoint.sh

ENV STUMP_CONFIG_DIR=/config \
    STUMP_CLIENT_DIR=/app/client \
    STUMP_PROFILE=release \
    STUMP_PORT=10801 \
    STUMP_IN_DOCKER=true \
    PDFIUM_PATH=/opt/pdfium/lib/libpdfium.so \
    API_VERSION=v1

WORKDIR /app

ENTRYPOINT ["/entrypoint.sh"]
