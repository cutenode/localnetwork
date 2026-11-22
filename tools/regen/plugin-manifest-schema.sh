#!/usr/bin/env bash
# The plugin manifest's JSON schema (crates/loclib-plugin/tests/schema.rs checks it).
set -euo pipefail
LOCALLIBRARY_BLESS=1 cargo test -p loclib-plugin --test schema
