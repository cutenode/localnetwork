#!/usr/bin/env bash
# locallibrary's RPC contract, apps/locallibrary/docs/rpc/ (tests/it/rpc.rs checks it).
set -euo pipefail
cargo run -p locallibrary -- rpc --emit-schema apps/locallibrary/docs/rpc
