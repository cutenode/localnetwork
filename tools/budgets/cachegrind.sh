#!/usr/bin/env bash
# The instructions <command> runs under cachegrind, which counts them exactly, with no environment but
# the <name>=<value> pairs given and a fixed PATH. The environment's size moves where the stack starts,
# and so how aligned the buffers glibc's string functions copy and search are: the same binary's
# `--version` counted 4,516 fewer with 8 more bytes of environment, and 9,020 fewer with 10 more, and
# a CI run's environment holds its branch names and ids (LOC-830). With --toggle-collect, only the
# instructions <function> and what it calls run on its thread count, under callgrind, which can tell
# them apart. The command's output is dropped and what it and valgrind print to stderr goes on to stderr.
#   cachegrind.sh [--toggle-collect=<function>] [<name>=<value>...] <command> [<arg>...]
set -euo pipefail
tool=(--tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null)
if [[ ${1:-} == --toggle-collect=* ]]; then
  tool=(--tool=callgrind --callgrind-out-file=/dev/null --collect-atstart=no "$1") && shift
fi
vars=()
while [[ ${1:-} == *=* ]]; do vars+=("$1") && shift; done
env -i PATH=/usr/bin:/bin "${vars[@]}" valgrind "${tool[@]}" "$@" 2>&1 >/dev/null |
  tee /dev/stderr | sed -n 's/.*I *refs: *//p' | tr -d ,
