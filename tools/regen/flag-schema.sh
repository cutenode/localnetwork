#!/usr/bin/env bash
# The flag catalog's JSON schema (apps/web/src/lib/server/flags/catalog.test.ts checks it).
set -euo pipefail
SCHEMA_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/server/flags/catalog.test.ts
