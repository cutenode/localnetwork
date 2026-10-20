#!/usr/bin/env bash
# Coverage from the Hegel property tests alone, to show the code they never reach. Runs every
# `#[hegel::test]` in the workspace through `cargo coverage` (.cargo/config.toml), so it fails
# below the same thresholds. Extra arguments go to cargo-llvm-cov: `--open` for the HTML report,
# `--lcov --output-path x`. `--print-filter` prints the nextest filtersets that pick those tests
# instead, one per line, for other runs of them: pass each line as its own `-E`, and nextest runs
# every test any of them picks (as locallibrary-scheduled.yml's `hegel` job does).
set -euo pipefail
cd "$(dirname "$0")/.."

# Linux caps each argument at 128 KiB (MAX_ARG_STRLEN), which one filterset naming every test
# outgrows, so the filters are packed into filtersets of at most this many bytes, one `-E` each.
# A single file's filter over it stops the script here, before exec could fail with E2BIG.
max_filterset_bytes=$((96 * 1024))

# Each test's name is the first `fn` after its attribute, which can span several lines. Plain
# tests elsewhere can share the name (`cues::x` beside `props_anlz::x`), so each filter also
# names the test's file: its module (`app` for src/tui/app.rs, `x` for x/mod.rs, `app::tests` for
# src/tui/app/tests.rs), or for an integration test's own file (in a package's tests/), its
# package and binary (several crates have a tests/props.rs). A crate root's src/tests.rs is only
# `tests`, which every plugin crate has, so it names the package too. The loop prints each test's
# filter up to its name, and the name; the tests that share a filter become one, with their names
# as alternatives.
package_of() { sed -n 's/^name = "\(.*\)"$/\1/p' "$1/Cargo.toml" | head -n 1; }
filtersets=$(
  while IFS=: read -r path name; do
    stem=$(basename "$path" .rs)
    [[ $stem == mod ]] && stem=$(basename "$(dirname "$path")")
    if [[ $path == */tests/$stem.rs && -f ${path%/tests/*}/Cargo.toml ]]; then
      head="binary_id(=$(package_of "${path%/tests/*}")::$stem) & test(/^(\w+::)*"
    elif [[ $stem == tests ]]; then
      parent=$(basename "$(dirname "$path")")
      if [[ $parent == src ]]; then
        head="package(=$(package_of "${path%/src/*}")) & test(/^tests::(\w+::)*"
      else
        head="test(/(^|::)$parent::tests::(\w+::)*"
      fi
    elif [[ $stem == lib || $stem == main ]]; then
      echo "hegel-coverage.sh can't place $name in $path yet: teach it crate roots" >&2
      exit 1
    else
      head="test(/(^|::)$stem::(\w+::)*"
    fi
    printf '%s\t%s\n' "$head" "$name"
  done < <(rg --multiline --with-filename --no-heading --only-matching --replace '$1' \
    '(?s)#\[hegel::test[(\]].*?\bfn (\w+)' --glob '*.rs' apps crates plugins vendor) |
    LC_ALL=C sort -u |
    LC_ALL=C awk -F '\t' -v max="$max_filterset_bytes" '
      function flush() {
        if (names == "") return
        filter = "(" head "(" names ")$/))"
        if (length(filter) > max) {
          printf "hegel-coverage.sh: the filter for %s... is %d bytes, over the %d bytes a filterset may take: move some of those tests to a module of another name\n",
            head, length(filter), max > "/dev/stderr"
          failed = 1
          exit 1
        }
        if (set != "" && length(set) + 3 + length(filter) > max) { print set; set = "" }
        set = set (set == "" ? "" : " | ") filter
      }
      $1 != head { flush(); head = $1; names = "" }
      { names = names (names == "" ? "" : "|") $2 }
      END { if (failed) exit 1; flush(); if (set != "") print set }
    '
)

if [[ ${1:-} == --print-filter ]]; then
  echo "$filtersets"
  exit
fi
args=()
while IFS= read -r set; do args+=(-E "$set"); done <<<"$filtersets"
exec cargo coverage "${args[@]}" "$@"
