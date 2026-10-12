#!/usr/bin/env bash
# Coverage from the Hegel property tests alone, to show the code they never reach. Runs every
# `#[hegel::test]` in the workspace under cargo-llvm-cov, with no thresholds. Extra arguments go
# to cargo-llvm-cov: `--open` for the HTML report, `--lcov --output-path x`.
set -euo pipefail
cd "$(dirname "$0")/.."

# Each test's name is the first `fn` after its attribute, which can span several lines. Plain
# tests elsewhere can share the name (`cues::x` beside `props_anlz::x`), so each filter also
# names the test's file: its module (`app` for src/tui/app.rs, `x` for x/mod.rs), or for an
# integration test's own file, its package and binary (several crates have a tests/props.rs).
filter=""
while IFS=: read -r path name; do
  stem=$(basename "$path" .rs)
  [[ $stem == mod ]] && stem=$(basename "$(dirname "$path")")
  if [[ $path == */tests/$stem.rs ]]; then
    package=$(sed -n 's/^name = "\(.*\)"$/\1/p' "${path%/tests/*}/Cargo.toml" | head -n 1)
    test="binary_id(=$package::$stem) & test(/^(\w+::)*$name\$/)"
  elif [[ $stem == lib || $stem == main ]]; then
    echo "hegel-coverage.sh can't place $name in $path yet: teach it crate roots" >&2
    exit 1
  else
    test="test(/(^|::)$stem::(\w+::)*$name\$/)"
  fi
  filter+="${filter:+ | }($test)"
done < <(rg --multiline --with-filename --no-heading --only-matching --replace '$1' \
  '(?s)#\[hegel::test[(\]].*?\bfn (\w+)' --glob '*.rs' apps crates vendor)

exec cargo llvm-cov nextest --workspace --locked --no-fail-fast -E "$filter" "$@"
