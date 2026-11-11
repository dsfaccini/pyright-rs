#!/usr/bin/env bash
# Run pyright-rs on oracle corpora and compare with the reference runs:
# the --outputjson diagnostics in order, and every dump stage.
#
# Usage: oracle/corpus/check.sh OUTDIR [CORPUS...]
#   OUTDIR must not exist yet; delete it when done.
#   Default corpora: every corpus except pydantic-ai, which needs far more memory.
# Environment:
#   CHECK_MEMORY_MAX  memory cap per run, as systemd MemoryMax (default 28G);
#                     a run over the cap is killed and reported as unreadable.
#   CHECK_FEATURES    cargo features for the build, e.g. jsrt/gc-check for
#                     PYRIGHT_RS_GC=check (docs/collector-design.md).
# Exit status: 0 when every corpus matches, 1 otherwise.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
REF=$HOME/pyright-rs-data/reference
OUT=$1; shift
corpora=${*:-samples typeshed primer-attrs primer-bidict primer-more-itertools primer-packaging primer-python-chess}
export PYRIGHT_RS_MODULE_PATH=${PYRIGHT_RS_MODULE_PATH:-$HOME/pyright-oracle/packages/pyright/dist}
cargo build -q --release ${CHECK_FEATURES:+--features "$CHECK_FEATURES"} --manifest-path "$ROOT/Cargo.toml" || exit 1
status=0
for c in $corpora; do
  echo "== $c"
  ORACLE_CHECKER="$ROOT/target/release/pyright-rs" ORACLE_OUT="$OUT/$c" \
    ORACLE_TYPESHED="$PYRIGHT_RS_MODULE_PATH/typeshed-fallback" \
    systemd-run --user --scope -q -p MemoryMax="${CHECK_MEMORY_MAX:-28G}" -p MemorySwapMax=0 \
    python3 "$ROOT/oracle/corpus/run.py" "$ROOT/oracle/corpus/$c.json" > /dev/null 2>&1
  grep -E "Elapsed|Maximum resident" "$OUT/$c/time.txt" 2>/dev/null | sed 's/^\s*/  pyright-rs /'
  grep -E "Elapsed|Maximum resident" "$REF/$c/gate-nodump.time.txt" | sed 's/^\s*/  pyright    /'
  python3 - "$REF/$c/output.json" "$OUT/$c/output.json" <<'EOF' || status=1
import json, sys
ref = json.load(open(sys.argv[1]))
try:
    cand = json.load(open(sys.argv[2]))
except (OSError, ValueError) as e:
    print(f"  outputjson: unreadable ({e})")
    sys.exit(1)
strip = lambda summary: {k: v for k, v in summary.items() if k != "timeInSec"}
same = ref["generalDiagnostics"] == cand["generalDiagnostics"] and strip(ref["summary"]) == strip(cand["summary"])
print(f"  outputjson: {'identical' if same else 'differs'} ({len(ref['generalDiagnostics'])} reference diagnostics, {len(cand['generalDiagnostics'])} candidate)")
sys.exit(0 if same else 1)
EOF
  python3 "$ROOT/oracle/compare.py" --max-reports 0 "$REF/$c/dump" "$OUT/$c/dump" > "$OUT/$c/compare.txt" 2>&1 || status=1
  tail -8 "$OUT/$c/compare.txt" | sed 's/^/  /'
done
exit $status
