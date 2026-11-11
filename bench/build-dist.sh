#!/usr/bin/env bash
# Build the fastest pyright-rs: the `dist` cargo profile, optimized with the
# profile of an instrumented build (PGO), then laid out by BOLT when BOLT is
# available. Both profiles come from checking the training corpora.
#
# Usage: bench/build-dist.sh [CORPUS...]
#   Default training corpora: every recorded corpus except pydantic-ai.
# Environment:
#   DIST_CPU      target-cpu (default x86-64-v2, the oldest CPU the binary must
#                 run on; set native to tune for this machine)
#   DIST_RUSTFLAGS  extra rustc flags for both builds
#   BOLT_DIR      directory holding llvm-bolt, perf2bolt and merge-fdata
#                 (default: found on PATH; without them the BOLT step is skipped)
#   DIST_BOLT     0 skips the BOLT step, e.g. on a CPU without a last-branch record
# Output: target/dist/pyright-rs
# A pydantic-ai training run holds /tmp/pyright-rs-pydantic-ai.lock, as
# bench/measure.sh does, and runs under a 30G memory cap.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
corpora=${*:-samples typeshed primer-attrs primer-bidict primer-more-itertools primer-packaging primer-python-chess}
# --target keeps these flags off build scripts and proc macros; a static relocation
# model cannot link them.
target=$(rustc -vV | sed -n 's/^host: //p')
flags="-C target-cpu=${DIST_CPU:-x86-64-v2} -C relocation-model=static ${DIST_RUSTFLAGS:-}"
profdata=$(rustc --print sysroot)/lib/rustlib/$target/bin/llvm-profdata
[ -x "$profdata" ] || { echo "llvm-profdata missing: rustup component add llvm-tools" >&2; exit 1; }
bolt_dir=${BOLT_DIR:-$(dirname "$(command -v llvm-bolt || echo .)")}
bolt=${DIST_BOLT:-1}
[ "$bolt" = 1 ] && [ ! -x "$bolt_dir/llvm-bolt" ] && { echo "llvm-bolt not found: skipping BOLT" >&2; bolt=0; }
work=$ROOT/target/dist-work
rm -rf "$work"; mkdir -p "$work" "$ROOT/target/dist"
export PYRIGHT_RS_MODULE_PATH=${PYRIGHT_RS_MODULE_PATH:-$HOME/pyright-oracle/packages/pyright/dist}

# train BIN [WRAPPER...]: check every training corpus with BIN, run under WRAPPER.
# The wrapper may name $corpus, which train sets before each run.
train() {
  local bin=$1; shift
  for corpus in $corpora; do
    local cmd=$HOME/pyright-rs-data/reference/$corpus/command.json cwd args lock=()
    cwd=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['cwd'])" "$cmd")
    mapfile -t args < <(python3 -c "import json,sys; print('\n'.join(json.load(open(sys.argv[1]))['args']))" "$cmd")
    [ "$corpus" = pydantic-ai ] && lock=(flock /tmp/pyright-rs-pydantic-ai.lock)
    echo "training on $corpus"
    # pyright exits 1 when it reports errors.
    (cd "$cwd" && "${lock[@]}" systemd-run --user --scope -q -p MemoryMax=30G -p MemorySwapMax=0 \
      "${@//\$corpus/$corpus}" "$bin" "${args[@]}" > /dev/null) || [ $? = 1 ]
  done
}

# PGO. Indirect calls in the evaluator need more value-profile counters than the default.
RUSTFLAGS="$flags -C profile-generate=$work/pgo -C llvm-args=-vp-counters-per-site=4" \
  CARGO_TARGET_DIR=$ROOT/target/dist-gen \
  cargo build -q --profile dist --target "$target" --manifest-path "$ROOT/Cargo.toml"
train "$ROOT/target/dist-gen/$target/dist/pyright-rs" env
"$profdata" merge -o "$work/raw.profdata" "$work/pgo"
# On the samples corpus the instrumented build counts blocks of num_bigint's mac3 and
# to_radix_le 10^12 times or more per call, more than the run executes instructions.
# Counts that large make every other function look cold, so drop such functions.
bogus=$("$profdata" show --all-functions --counts "$work/raw.profdata" | awk '
  /^  [^ ].*:$/ { fn = substr($1, 1, length($1) - 1) }
  /Block counts:/ { gsub(/[][,]/, ""); for (i = 3; i <= NF; i++) if ($i > 1e12) { print fn; break } }' | paste -sd'|')
"$profdata" merge ${bogus:+--no-function="^($bogus)\$"} -o "$work/pgo.profdata" "$work/raw.profdata"

# BOLT needs the relocations the linker would otherwise drop.
relocs=; [ "$bolt" = 1 ] && relocs="-C link-arg=-Wl,--emit-relocs"
RUSTFLAGS="$flags -C profile-use=$work/pgo.profdata $relocs" CARGO_TARGET_DIR=$ROOT/target/dist-use \
  cargo build -q --profile dist --target "$target" --manifest-path "$ROOT/Cargo.toml"
pgo_bin=$ROOT/target/dist-use/$target/dist/pyright-rs
if [ "$bolt" = 0 ]; then
  cp "$pgo_bin" "$ROOT/target/dist/pyright-rs"
else
  # BOLT reads taken branches from the CPU's branch record (LBR); the period keeps
  # sampling under the kernel's default rate limit.
  train "$pgo_bin" perf record -q -N -e cycles:u -c 100003 -j any,u -o "$work/\$corpus.perf"
  for corpus in $corpora; do
    "$bolt_dir/perf2bolt" -p "$work/$corpus.perf" -o "$work/$corpus.fdata" "$pgo_bin" > /dev/null 2>&1
    rm "$work/$corpus.perf"
  done
  "$bolt_dir/merge-fdata" "$work"/*.fdata > "$work/bolt.fdata" 2> /dev/null
  "$bolt_dir/llvm-bolt" "$pgo_bin" -o "$ROOT/target/dist/pyright-rs" -data="$work/bolt.fdata" \
    -reorder-blocks=ext-tsp -reorder-functions=cdsort -split-functions -split-all-cold -split-eh \
    -icf=all -use-gnu-stack -update-debug-sections > "$work/bolt.log" 2>&1
fi
echo "$ROOT/target/dist/pyright-rs (trained on: $corpora)"
