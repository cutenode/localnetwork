#!/usr/bin/env bash
# loclib-equipment's table as JSON and its schema (tests/json.rs checks them).
set -euo pipefail
LOCALLIBRARY_BLESS=1 cargo test -p loclib-equipment --all-features --test json
