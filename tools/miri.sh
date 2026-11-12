#!/usr/bin/env bash
# Run the code that reaches pyright-rs's unsafe blocks under Miri, with Stacked
# Borrows and with Tree Borrows (docs/porting.md, "unsafe code").
#
# Usage: tools/miri.sh OUTDIR [SUITE...]
#   OUTDIR must not exist yet; delete it when done. It holds one log per suite and
#   model, and the Miri build directories.
#   Suites (default: all four):
#     jsrt      jsrt's unit tests: the collector, P, the cells, regions.
#     pyright   the pyright unit tests that tokenize, parse, and build flow nodes and
#               evaluator types, and parse a file on a parse worker.
#     check     pyright-rs checks tools/miri/project against the stub typeshed in
#               tools/miri, collecting before every file, and must print what the
#               native build prints.
#     gc-check  the same check in the collector's check mode (PYRIGHT_RS_GC=check, two
#               marking threads), with the census, whose exit step drops every object.
#   Every token region is retired when dropped (PYRIGHT_RS_TOKENS=check), so a stale
#   token is a use after free to Miri. Every suite runs under both models at once:
#   up to eight Miri processes, one core each.
# Needs nightly Miri: rustup toolchain install nightly --component miri
# Exit status: 0 when every run passes.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1; shift
suites=${*:-jsrt pyright check gc-check}
mkdir "$OUT" || exit 1
OUT=$(realpath "$OUT")
# The program heap and the arenas are never freed, so Miri's leak check does not apply.
common="-Zmiri-strict-provenance -Zmiri-ignore-leaks"
pyright_tests=(
  parser:: analyzer::code_flow_utils analyzer::parse_tree_utils analyzer::analyzer_node_info
  analyzer::types_tests analyzer::type_printer_tests analyzer::type_utils_tests
  analyzer::type_cache_utils_tests analyzer::static_expressions analyzer::source_mapper_utils_tests parse_ahead::
)
export PYRIGHT_RS_TOKENS=check
# The check suites' environment and features, for both the Miri and the native run.
check_env=(PYRIGHT_RS_MODULE_PATH="$ROOT/tools/miri" PYRIGHT_RS_GC_MIN_MIB=0)
gc_check_env=("${check_env[@]}" PYRIGHT_RS_GC=check PYRIGHT_RS_GC_THREADS=2 PYRIGHT_RS_GC_CENSUS=1)
gc_check_features=jsrt/gc-check,pyright/count-malloc

# The output without its timings.
strip_times() {
  python3 -c '
import json, sys
d = json.load(open(sys.argv[1]))
d.pop("time", None)
d["summary"].pop("timeInSec", None)
print(json.dumps(d, indent=1))' "$1"
}

run() {
  local suite=$1 model=$2 flags=$common
  [ "$model" = tree ] && flags="$flags -Zmiri-tree-borrows"
  export CARGO_TARGET_DIR=$OUT/target-$suite-$model
  case $suite in
    jsrt)
      # The census reads /proc, which isolation refuses; it then reports zeros.
      MIRIFLAGS="$flags -Zmiri-isolation-error=warn-nobacktrace" \
        cargo +nightly miri test --manifest-path "$ROOT/Cargo.toml" -p jsrt --lib ;;
    pyright)
      MIRIFLAGS="$flags -Zmiri-disable-isolation" \
        cargo +nightly miri test --manifest-path "$ROOT/Cargo.toml" -p pyright --lib -- "${pyright_tests[@]}" ;;
    check | gc-check)
      local vars=("${check_env[@]}") features=()
      [ "$suite" = gc-check ] && vars=("${gc_check_env[@]}") features=(--features "$gc_check_features")
      MIRIFLAGS="$flags -Zmiri-disable-isolation" env "${vars[@]}" \
        cargo +nightly miri run -q --manifest-path "$ROOT/Cargo.toml" "${features[@]}" --bin pyright-rs -- \
        --outputjson -p "$ROOT/tools/miri/project" > "$OUT/$suite-$model.json"
      # pyright-rs exits 1 when it reports errors, as Miri does on undefined behavior,
      # so Miri's findings are told apart by their "error:" lines.
      [ $? -le 1 ] && ! grep -q "^error" "$OUT/$suite-$model.log" || return 1
      diff <(strip_times "$OUT/$suite-native.json") <(strip_times "$OUT/$suite-$model.json") \
        || { echo "miri.sh: the output under Miri differs from the native build's"; return 1; } ;;
  esac
}

# The native runs to compare with. Miri cannot start a Python interpreter, so neither may they.
for suite in check gc-check; do
  [[ " $suites " == *" $suite "* ]] || continue
  vars=("${check_env[@]}") features=()
  [ "$suite" = gc-check ] && vars=("${gc_check_env[@]}") features=(--features "$gc_check_features")
  CARGO_TARGET_DIR=$OUT/target-$suite-native cargo build -q --manifest-path "$ROOT/Cargo.toml" "${features[@]}" \
    --bin pyright-rs || exit 1
  env "${vars[@]}" PATH=/nonexistent "$OUT/target-$suite-native/debug/pyright-rs" \
    --outputjson -p "$ROOT/tools/miri/project" > "$OUT/$suite-native.json" 2> "$OUT/$suite-native.log"
done
pids=()
for suite in $suites; do
  for model in stacked tree; do
    (run "$suite" "$model" > "$OUT/$suite-$model.log" 2>&1; echo "exit $?" >> "$OUT/$suite-$model.log") &
    pids+=($!)
  done
done
wait "${pids[@]}"
status=0
for suite in $suites; do
  for model in stacked tree; do
    result=$(tail -1 "$OUT/$suite-$model.log")
    echo "$suite, $model borrows: $([ "$result" = "exit 0" ] && echo pass || echo "FAIL (see $OUT/$suite-$model.log)")"
    [ "$result" = "exit 0" ] || status=1
  done
done
exit $status
