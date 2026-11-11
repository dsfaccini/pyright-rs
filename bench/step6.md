# Step 6: first full check

Measured 2026-11-10 on devbox (16 cores, 62 GB), at `main` 27af4b8, the first commit where the checker runs by default.

## correctness

Command: `oracle/corpus/check.sh <outdir>`. It compares `--outputjson` (diagnostics in order, and the summary without `timeInSec`) and every dump stage: tokens, tree, bind, flow, types, diag.

| corpus | `--outputjson` diagnostics | output | dump files per stage | differing |
|---|---|---|---|---|
| samples | 6,546 | identical | 1,429 | 0 |
| typeshed | 623 | identical | 787 | 0 |
| primer-attrs | 106 | identical | 66 | 0 |
| primer-bidict | 12 | identical | 30 | 0 |
| primer-more-itertools | 34 | identical | 38 | 0 |
| primer-packaging | 31 | identical | 75 | 0 |
| primer-python-chess | 6 | identical | 78 | 0 |
| pydantic-ai | — | killed at a 36 GB memory cap after 48 s | — | — |

pydantic-ai: the evaluator heap never frees. RSS grows by about 0.75 GB/s; pyright peaks at 9 GB. Allocation stats at 16 GiB of evaluator heap: `~/pyright-rs-data/bench/step6-pydantic-ai-alloc-16g.txt`. `Type` (584 bytes each) is 9.4 GB of it, `TypeResult` 3.4 GB.

## time and memory

Dumps off, median of three runs, peak RSS the maximum. pyright: the fork, `node --max-old-space-size=16384`, which matches upstream with dumps off. Load average 2 to 3 from other work during the runs.

| corpus | pyright wall | pyright-rs wall | ratio | pyright CPU | pyright-rs CPU | pyright peak RSS | pyright-rs peak RSS |
|---|---|---|---|---|---|---|---|
| samples | 5.68 s | 1.96 s | 2.9x | 9.96 s | 1.95 s | 0.67 GB | 1.18 GB |
| typeshed | 3.53 s | 1.39 s | 2.5x | 6.36 s | 1.38 s | 0.62 GB | 1.00 GB |
| primer-attrs | 1.40 s | 0.43 s | 3.3x | 2.68 s | 0.43 s | 0.29 GB | 0.34 GB |
| primer-bidict | 0.62 s | 0.13 s | 4.8x | 1.05 s | 0.12 s | 0.20 GB | 0.09 GB |
| primer-more-itertools | 1.39 s | 0.47 s | 3.0x | 2.65 s | 0.47 s | 0.28 GB | 0.34 GB |
| primer-packaging | 1.53 s | 0.42 s | 3.6x | 2.85 s | 0.42 s | 0.30 GB | 0.31 GB |
| primer-python-chess | 1.92 s | 0.57 s | 3.4x | 3.46 s | 0.57 s | 0.33 GB | 0.45 GB |

- pyright's CPU time exceeds its wall time because V8 collects garbage on helper threads. pyright-rs runs on one thread.
- No memory work has landed yet: no collector, and `Type` is 584 bytes.

## progress on pydantic-ai memory

| `main` | change | `size_of::<Type>()` | result under a 30 GB cap | RSS growth |
|---|---|---|---|---|
| 27af4b8 | none | 584 bytes | killed after about 40 s | about 0.73 GB/s |
| 897d847 | `Type` shrunk (`shrink` agent) | 144 bytes | killed after 58 s | about 0.53 GB/s |
| 69f39c5 | also `TypeResult` 104 bytes, constraint sets and `DiagnosticAddendum` 16–32 bytes | 144 bytes | completes: 1:02 wall, 27.8 GB peak; pyright 2:32, 9.1 GB | about 0.45 GB/s |
| a687113 | all of `shrink` (sizes below) | 128 bytes | completes: 0:55 wall, 25.6 GB peak, output identical | about 0.47 GB/s |
| 2990cef | collector on by default (`collector` agent) | 128 bytes | completes: 1:06 wall, 12.5 GB peak (collector agent's run; off: 0:54, 25.7 GB) | bounded by collections |

At 897d847, 12 GiB into the evaluator heap: `TypeResult` 4.5 GB (17.0M × 264 bytes), `Type` 2.9 GB (20.1M × 144), `ConstraintSet` 1.6 GB, `ConstraintSolutionSet` 1.2 GB, `DiagnosticAddendum` 1.0 GB.

At 69f39c5 the `--outputjson` output on pydantic-ai is identical (0 diagnostics, `filesAnalyzed` 2,046 on both sides).
A run with the types and diag dumps on was killed at 30 GB after 1:49, 965 of 6,696 files dumped. In those files the diag stage matches, and the types stage differs in 2 files: a string literal with a lone surrogate prints as U+FFFD where pyright prints `\ud800` (`LiteralValue::String` holds a `String`, not a `JsString`).

`shrink` agent, final `size_of` in bytes, each locked by a compile-time assert: `Type` 584 → 128, `TypeResult` 264 → 40, `ConstraintSet` 128 → 32, `ConstraintSolutionSet` 64 → 16, `DiagnosticAddendum` 88 → 24, `VirtualParamDetails` 88 → 80, `FunctionDetailsShared` 280 → 192, `ConstraintTracker` 32 → 24, `FunctionParam` 56 → 48.

At a687113 the arena stats count about 7 GB of evaluator objects at the end of the run, but RSS peaks at 25.6 GB. Most of the rest is buffers those objects own on the system heap (`Vec`, `Box`, maps), which the stats do not count. The collector frees them when it drops dead objects.

## pydantic-ai gate at 2990cef

`CHECK_MEMORY_MAX=30G oracle/corpus/check.sh <dir> pydantic-ai`, run by the main session: `--outputjson` identical, and all 40,176 dump files identical (6,696 files × tokens, tree, bind, flow, types, diag). With the dumps on, 2:57 wall and 18.4 GB peak. pyright-rs output now matches pyright 1.1.414 on all eight corpora.
