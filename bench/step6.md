# Step 6: first full check

Measured 2026-11-10 on devbox (16 cores, 62 GiB), at `main` 27af4b8, the first commit where the checker runs by default.

Memory is in GiB (2^30 bytes) throughout. pyright's peak RSS on pydantic-ai is 9,073,044 KiB, 8.65 GiB (`~/pyright-rs-data/reference/pydantic-ai/gate-nodump.time.txt`).

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
| pydantic-ai | — | killed at a 36 GiB memory cap after 48 s | — | — |

pydantic-ai: the evaluator heap never frees. RSS grows by about 0.75 GiB/s; pyright peaks at 8.65 GiB. Allocation stats at 16 GiB of evaluator heap: `~/pyright-rs-data/bench/step6-pydantic-ai-alloc-16g.txt`. `Type` (584 bytes each) is 8.76 GiB of it, `TypeResult` 3.21 GiB.

## time and memory

Dumps off, median of three runs, peak RSS the maximum. pyright: the fork, `node --max-old-space-size=16384`, which matches upstream with dumps off. Load average 2 to 3 from other work during the runs.

| corpus | pyright wall | pyright-rs wall | ratio | pyright CPU | pyright-rs CPU | pyright peak RSS | pyright-rs peak RSS |
|---|---|---|---|---|---|---|---|
| samples | 5.68 s | 1.96 s | 2.9x | 9.96 s | 1.95 s | 0.67 GiB | 1.18 GiB |
| typeshed | 3.53 s | 1.39 s | 2.5x | 6.36 s | 1.38 s | 0.62 GiB | 1.00 GiB |
| primer-attrs | 1.40 s | 0.43 s | 3.3x | 2.68 s | 0.43 s | 0.29 GiB | 0.34 GiB |
| primer-bidict | 0.62 s | 0.13 s | 4.8x | 1.05 s | 0.12 s | 0.20 GiB | 0.09 GiB |
| primer-more-itertools | 1.39 s | 0.47 s | 3.0x | 2.65 s | 0.47 s | 0.28 GiB | 0.34 GiB |
| primer-packaging | 1.53 s | 0.42 s | 3.6x | 2.85 s | 0.42 s | 0.30 GiB | 0.31 GiB |
| primer-python-chess | 1.92 s | 0.57 s | 3.4x | 3.46 s | 0.57 s | 0.33 GiB | 0.45 GiB |

- pyright's CPU time exceeds its wall time because V8 collects garbage on helper threads. pyright-rs runs on one thread.
- No memory work has landed yet: no collector, and `Type` is 584 bytes.

## progress on pydantic-ai memory

| `main` | change | `size_of::<Type>()` | result under a 30 GiB cap | RSS growth |
|---|---|---|---|---|
| 27af4b8 | none | 584 bytes | killed after about 40 s | about 0.73 GiB/s |
| 897d847 | `Type` shrunk (`shrink` agent) | 144 bytes | killed after 58 s | about 0.53 GiB/s |
| 69f39c5 | also `TypeResult` 104 bytes, constraint sets and `DiagnosticAddendum` 16–32 bytes | 144 bytes | completes: 1:02 wall, 26.5 GiB peak; pyright 2:32, 8.65 GiB | about 0.45 GiB/s |
| a687113 | all of `shrink` (sizes below) | 128 bytes | completes: 0:55 wall, 24.4 GiB peak, output identical | about 0.47 GiB/s |
| 2990cef | collector on by default (`collector` agent) | 128 bytes | completes: 1:06 wall, about 11.9 GiB peak (collector agent's run, reported as 12.5 "GB" of 10^6 KiB; off: 0:54, about 24.5 GiB) | bounded by collections |

At 897d847, 12 GiB into the evaluator heap: `TypeResult` 4.19 GiB (17.0M × 264 bytes), `Type` 2.69 GiB (20.1M × 144), `ConstraintSet` 1.50 GiB, `ConstraintSolutionSet` 1.15 GiB, `DiagnosticAddendum` 0.94 GiB.

At 69f39c5 the `--outputjson` output on pydantic-ai is identical (0 diagnostics, `filesAnalyzed` 2,046 on both sides).
A run with the types and diag dumps on was killed at 30 GiB after 1:49, 965 of 6,696 files dumped. In those files the diag stage matches, and the types stage differs in 2 files: a string literal with a lone surrogate prints as U+FFFD where pyright prints `\ud800` (`LiteralValue::String` holds a `String`, not a `JsString`).

`shrink` agent, final `size_of` in bytes, each locked by a compile-time assert: `Type` 584 → 128, `TypeResult` 264 → 40, `ConstraintSet` 128 → 32, `ConstraintSolutionSet` 64 → 16, `DiagnosticAddendum` 88 → 24, `VirtualParamDetails` 88 → 80, `FunctionDetailsShared` 280 → 192, `ConstraintTracker` 32 → 24, `FunctionParam` 56 → 48.

At a687113 the arena stats count about 6.5 GiB of evaluator objects at the end of the run, but RSS peaks at 24.4 GiB. Most of the rest is buffers those objects own on the system heap (`Vec`, `Box`, maps), which the stats do not count. The collector frees them when it drops dead objects.

## pydantic-ai gate at 2990cef

`CHECK_MEMORY_MAX=30G oracle/corpus/check.sh <dir> pydantic-ai`, run by the main session: `--outputjson` identical, and all 40,176 dump files identical (6,696 files × tokens, tree, bind, flow, types, diag). With the dumps on, 2:57 wall and 17.5 GiB peak. pyright-rs output now matches pyright 1.1.414 on all eight corpora.

## baseline for the optimization round

`bench/measure.sh`, median of 3, at 308f470 (mimalloc and the collector on by default):

| corpus | instructions | wall | peak RSS | pyright wall | pyright peak RSS |
|---|---|---|---|---|---|
| pydantic-ai | 388.7 G | 52.3 s | 11.8 GiB | 2:32 | 8.65 GiB |
| samples | 13.95 G | 1.24 s | 0.68 GiB | 5.5 s | 0.62 GiB |

Five agents work from this baseline: `hot` (CPU hotspots), `build` (compiler and link settings), `gc` (collection time and peak memory), `progmem` (program heap) and `explore` (architectural levers, research).

## optimization round progress

`bench/measure.sh`, median of 3 (load average 4 to 5 from other agents, so wall times are high):

| `main` | change | pydantic-ai | samples |
|---|---|---|---|
| 308f470 | baseline | 388.7 G instructions, 52.3 s, 11.8 GiB | 13.95 G, 1.24 s, 0.68 GiB |
| 6094c4c | fat LTO (`build`) and 28 hotspot fixes (`hot`) | 282.6 G instructions (−27.3%), 42.7 s, 11.4 GiB | 10.16 G (−27.2%), 1.00 s, 0.65 GiB |
| b906345 | generational collection with a card-marking write barrier, marking on up to 8 threads (`gc`) | 273.1 G instructions (+2.2% over 6fa28c0's 267.2 G), 35.4 s (38.9 s before, same load window), 10.63 GiB (11.26 before) | |
| eb9865f, `dist` | `bench/build-dist.sh`: PGO trained on the seven small corpora, BOLT, `panic = "abort"`, x86-64-v2 | 229.7 G instructions (−40.9%), 39.5 s, 11.4 GiB | 8.18 G (−41.4%), 0.81 s, 0.66 GiB |

At 6094c4c `CHECK_MEMORY_MAX=30G oracle/corpus/check.sh` passes on pydantic-ai: output and all 40,176 dumps identical. The eb9865f `dist` binary passes all eight corpora the same way (`PYRIGHT_RS_BIN=target/dist/pyright-rs`). pydantic-ai never trained the PGO profile.

## after the optimization round's merges

`bench/measure.sh`, median of 3, `main` cc3de3f (load average 2 to 3). Both builds match pyright on all eight corpora, output and all 40,176 pydantic-ai dumps.

| build | pydantic-ai | samples |
|---|---|---|
| `release` | 266.9 G instructions, 32.1 s, 7.29 GiB | 9.85 G, 0.94 s, 0.54 GiB |
| `dist` (`bench/build-dist.sh`) | 217.1 G instructions, 28.5 s, 7.29 GiB | 7.93 G, 0.76 s, 0.55 GiB |
| pyright 1.1.414 | 2:32, 8.65 GiB | 5.5 s, 0.62 GiB |

From the round's baseline (308f470: 388.7 G, 52.3 s, 11.8 GiB on pydantic-ai), `dist` runs 44% fewer instructions, and its peak RSS is 38% lower. `progmem` (cc3de3f) cut the program arena from 4,588 MiB to 1,472 MiB, and freed the tokens that no parse node holds, as pyright drops `tokenizerOutput` after parsing (`sourceFile.ts:781`).

## second optimization round progress

`bench/measure.sh`, `release`, one run (instructions repeat to within 0.1%; wall times are from a loaded devbox):

| `main` | change | pydantic-ai | samples |
|---|---|---|---|
| cc3de3f | start of the round | 266.9 G instructions, 7.29 GiB | 9.85 G, 0.54 GiB |
| 50778f0 | collector sweep and mark work (`gc`); names, symbol tables and code-flow keys as interned atoms, a scope-id cache and a message-key index (`intern`) | 243.7 G instructions (−8.7%), 31.2 s, 7.09 GiB | 9.28 G (−5.8%), 0.94 s, 0.51 GiB |
| d16e837 | collector allocation prefetch (`gc`); smaller `TypeResult`, shared doc strings, empty props unallocated, name atoms, `IdMap` type caches (`evalmem`); deferred MRO specialization, abstract-symbols memo, lazy addendum formatting, `print_type` memo (`memo`); Miri-clean unsafe code (`sound`) | 216.7 G instructions (−18.8%), 26.5 s, 5.79 GiB | 9.01 G (−8.5%), 0.92 s, 0.48 GiB |

At 50778f0 the pydantic-ai gate passes: output and all 40,176 dumps identical, also with `PYRIGHT_RS_SCOPE_ID_CACHE=check` and `PYRIGHT_RS_LOCALIZE_INDEX=check`.

At d16e837 the pydantic-ai gate passes: output and all 40,176 dumps identical, also with `PYRIGHT_RS_DEFER_MRO_SPECIALIZATION`, `PYRIGHT_RS_ABSTRACT_SYMBOLS_MEMO`, `PYRIGHT_RS_LAZY_ADDENDUM_FORMAT` and `PYRIGHT_RS_PRINT_TYPE_MEMO` at `check`.
