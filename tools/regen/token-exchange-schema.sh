#!/usr/bin/env bash
# The token exchange services' JSON schema (apps/web/src/lib/server/token-exchange/services.test.ts checks it).
set -euo pipefail
SCHEMA_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/server/token-exchange/services.test.ts
