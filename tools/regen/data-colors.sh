#!/usr/bin/env bash
# The data colours' CSS, from packages/tokens/colors.json (apps/web/src/lib/ui/data-colors.test.ts checks it).
set -euo pipefail
TOKENS_BLESS=1 pnpm -C apps/web vitest run --project server src/lib/ui/data-colors.test.ts
