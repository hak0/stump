#!/bin/bash

PRISMA_SCHEMA=${PRISMA_SCHEMA:-./core/prisma/schema.sqlite.prisma}

if [ "$RUN_PRISMA_GENERATE" = "true" ]; then
  set -ex; \
    cargo prisma generate --schema "$PRISMA_SCHEMA"
fi

set -ex; \
  ./scripts/release/utils.sh -w; \
  ./scripts/release/utils.sh -p; \
  cargo build --package stump_server --bin stump_server --release
