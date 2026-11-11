#!/usr/bin/env bash
# Measure pyright-rs on one recorded corpus: user-space instructions, wall time
# and peak RSS, median of N runs, dumps off. Instructions stay stable while other
# work loads the machine; wall time does not.
#
# Usage: bench/measure.sh [CORPUS] [RUNS]   (defaults: pydantic-ai 3)
# Environment:
#   PYRIGHT_RS_BIN  binary to measure (default: this tree's target/release/pyright-rs,
#                   built first)
#   Any PYRIGHT_RS_* switch is passed through, e.g. PYRIGHT_RS_GC=off.
# pydantic-ai runs hold /tmp/pyright-rs-pydantic-ai.lock, as oracle/corpus/check.sh does,
# and run under a 30G memory cap.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
corpus=${1:-pydantic-ai}; runs=${2:-3}
if [ -z "${PYRIGHT_RS_BIN:-}" ]; then
  cargo build -q --release --manifest-path "$ROOT/Cargo.toml" || exit 1
  PYRIGHT_RS_BIN=$ROOT/target/release/pyright-rs
fi
export PYRIGHT_RS_MODULE_PATH=${PYRIGHT_RS_MODULE_PATH:-$HOME/pyright-oracle/packages/pyright/dist}
cmd=$HOME/pyright-rs-data/reference/$corpus/command.json
cwd=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['cwd'])" "$cmd")
mapfile -t args < <(python3 -c "import json,sys; print('\n'.join(json.load(open(sys.argv[1]))['args']))" "$cmd")
lock=()
[ "$corpus" = pydantic-ai ] && lock=(flock /tmp/pyright-rs-pydantic-ai.lock)
tmp=$(mktemp -d)
for i in $(seq "$runs"); do
  (cd "$cwd" && "${lock[@]}" systemd-run --user --scope -q -p MemoryMax=30G -p MemorySwapMax=0 \
    perf stat -x, -e instructions:u -o "$tmp/perf$i" \
    /usr/bin/time -f "%e %M" -o "$tmp/time$i" "$PYRIGHT_RS_BIN" "${args[@]}" > /dev/null 2>&1)
done
python3 - "$tmp" "$runs" "$corpus" <<'EOF'
import pathlib, statistics, sys
tmp, runs, corpus = pathlib.Path(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
ins, wall, rss = [], [], []
for i in range(1, runs + 1):
    try:
        w, m = (tmp / f"time{i}").read_text().split()[-2:]
        n = next(l for l in (tmp / f"perf{i}").read_text().splitlines() if "instructions" in l).split(",")[0]
    except (OSError, StopIteration, ValueError):
        print(f"run {i}: no result (killed at the memory cap?)"); continue
    ins.append(int(n)); wall.append(float(w)); rss.append(int(m) / 1048576)
if ins:
    print(f"{corpus}: {statistics.median(ins) / 1e9:.2f} G instructions, {statistics.median(wall):.2f} s wall, "
          f"{max(rss):.2f} GB peak RSS (median of {len(ins)}; instructions spread "
          f"{(max(ins) - min(ins)) / statistics.median(ins) * 100:.2f}%)")
EOF
rm -r "$tmp"
