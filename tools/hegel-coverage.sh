#!/usr/bin/env bash
# Coverage from the Hegel property tests alone, to show the code they never reach. Runs every
# `#[hegel::test]` in the workspace through `cargo coverage` (.cargo/config.toml), so it fails
# below the same thresholds. Extra arguments go to cargo-llvm-cov: `--open` for the HTML report,
# `--lcov --output-path x`. `--print-filter` prints the nextest filter that picks those tests
# instead, for other runs of them (`cargo nextest run -E "$(tools/hegel-coverage.sh --print-filter)"`).
set -euo pipefail
cd "$(dirname "$0")/.."

# Each test's name is the first `fn` after its attribute, which can span several lines. Plain
# tests elsewhere can share the name (`cues::x` beside `props_anlz::x`), so each filter also
# names the test's file: its module (`app` for src/tui/app.rs, `x` for x/mod.rs, `app::tests` for
# src/tui/app/tests.rs), or for an integration test's own file, its package and binary (several
# crates have a tests/props.rs). A crate root's src/tests.rs is only `tests`, which every plugin
# crate has, so it names the package too.
package_of() { sed -n 's/^name = "\(.*\)"$/\1/p' "$1/Cargo.toml" | head -n 1; }
filter=""
while IFS=: read -r path name; do
  stem=$(basename "$path" .rs)
  [[ $stem == mod ]] && stem=$(basename "$(dirname "$path")")
  if [[ $path == */tests/$stem.rs ]]; then
    test="binary_id(=$(package_of "${path%/tests/*}")::$stem) & test(/^(\w+::)*$name\$/)"
  elif [[ $stem == tests ]]; then
    parent=$(basename "$(dirname "$path")")
    if [[ $parent == src ]]; then
      test="package(=$(package_of "${path%/src/*}")) & test(/^tests::(\w+::)*$name\$/)"
    else
      test="test(/(^|::)$parent::tests::(\w+::)*$name\$/)"
    fi
  elif [[ $stem == lib || $stem == main ]]; then
    echo "hegel-coverage.sh can't place $name in $path yet: teach it crate roots" >&2
    exit 1
  else
    test="test(/(^|::)$stem::(\w+::)*$name\$/)"
  fi
  filter+="${filter:+ | }($test)"
done < <(rg --multiline --with-filename --no-heading --only-matching --replace '$1' \
  '(?s)#\[hegel::test[(\]].*?\bfn (\w+)' --glob '*.rs' apps crates plugins vendor)

if [[ ${1:-} == --print-filter ]]; then
  echo "$filter"
  exit
fi
exec cargo coverage -E "$filter" "$@"
