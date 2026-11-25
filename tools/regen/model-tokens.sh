#!/usr/bin/env bash
# loclib-model's colours, from packages/tokens/colors.json (tests/tokens.rs checks them).
set -euo pipefail
LOCALLIBRARY_BLESS=1 cargo test -p loclib-model --test tokens
