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

## after the second optimization round

`main` 232d44d on a quiet devbox (load average 0.3 to 1.8), nothing else running. `bench/compare.py target/release/pyright-rs,target/dist/pyright-rs <corpora>`: median of 3 wall and CPU times, maximum peak RSS; pyright is the recorded checker with dumps off. Both builds match pyright on all eight corpora, output and all 40,176 pydantic-ai dumps.

| corpus | pyright | release | dist | wall speedup |
|---|---|---|---|---|
| pydantic-ai | 136.13 s, 211.63 s CPU, 9.94 GiB | 24.92 s, 26.80 s CPU, 5.68 GiB | 20.53 s, 22.44 s CPU, 5.70 GiB | 5.5x, 6.6x |
| samples | 5.47 s, 9.63 s CPU, 0.62 GiB | 0.76 s, 0.76 s CPU, 0.45 GiB | 0.59 s, 0.58 s CPU, 0.46 GiB | 7.2x, 9.3x |
| typeshed | 3.32 s, 5.66 s CPU, 0.62 GiB | 0.53 s, 0.52 s CPU, 0.37 GiB | 0.41 s, 0.41 s CPU, 0.37 GiB | 6.3x, 8.1x |
| primer-attrs | 1.35 s, 2.59 s CPU, 0.29 GiB | 0.15 s, 0.15 s CPU, 0.13 GiB | 0.13 s, 0.12 s CPU, 0.14 GiB | 9.0x, 10.4x |
| primer-bidict | 0.61 s, 1.01 s CPU, 0.20 GiB | 0.06 s, 0.06 s CPU, 0.06 GiB | 0.05 s, 0.05 s CPU, 0.07 GiB | 10.2x, 12.2x |
| primer-more-itertools | 1.35 s, 2.57 s CPU, 0.28 GiB | 0.17 s, 0.17 s CPU, 0.15 GiB | 0.14 s, 0.13 s CPU, 0.15 GiB | 7.9x, 9.6x |
| primer-packaging | 1.48 s, 2.80 s CPU, 0.29 GiB | 0.18 s, 0.17 s CPU, 0.14 GiB | 0.14 s, 0.13 s CPU, 0.15 GiB | 8.2x, 10.6x |
| primer-python-chess | 1.89 s, 3.41 s CPU, 0.32 GiB | 0.24 s, 0.23 s CPU, 0.18 GiB | 0.19 s, 0.19 s CPU, 0.18 GiB | 7.9x, 9.9x |

pyright's pydantic-ai peak varied between measurements: 8.65 GiB in the reference run, 9.94 GiB here.

`bench/measure.sh`, one run each, instructions:

| build | pydantic-ai | samples |
|---|---|---|
| `release` | 213.35 G | 7.55 G |
| `dist` | 175.09 G | 6.04 G |

From the first round's baseline (308f470: 388.7 G, 52.3 s, 11.8 GiB on pydantic-ai), `dist` runs 55% fewer instructions and peaks 52% lower. From the second round's start (cc3de3f, `dist`: 217.1 G, 28.5 s, 7.29 GiB), 19% fewer instructions and a 22% lower peak.

## collection trigger

The `trigger` agent (765231c, `notes/trigger.md`): minor collections trace only the cache entries written since the last collection, the card scan tests eight cards at a time, and the sweep skips bitmap words with nothing dead. pydantic-ai, `release`, 3 runs: 213.36 G → 212.99 G instructions at the default, peak 5.68 GiB either way; with `PYRIGHT_RS_GC_MIN_MIB=64`, 225.53 G → 218.28 G at 5.07 GiB.

`PYRIGHT_RS_GC_TAPER=1` shrinks the trigger's floor as the check nears its end: pydantic-ai peaks at 5.19 GiB for 0.24% more instructions than f02fe2a, but 2 to 3% more wall time in paired runs. It stays off by default: speed comes first, and the default already peaks 43% below pyright. A trigger that also counts the buffers garbage holds left the peak unchanged, since the peak comes from the last interval's size.

## third optimization round progress

`bench/measure.sh`, `release`, as each agent measured it (instructions repeat to within 0.1%; wall times are from a loaded devbox):

| `main` | change | pydantic-ai | samples |
|---|---|---|---|
| 04ac81d | start of the round | 212.99 G instructions, 5.69 GiB | 7.55 G, 0.45 GiB |
| 3920e3e | constraint sets, `TypeResult`'s cold half and addendum content as evaluator objects, so dead ones need no drop (`temps`) | 212.79 G, 5.52 GiB; pauses 2.08 → 1.57 s | 7.56 G, 0.45 GiB |
| fa2471f | allocation prefetch 512 bytes ahead (`temps`) | 211.82 G (−0.47%) | 7.54 G |
| 00fc773 | tokenizer, parser, binder and import resolution hot paths (`bind`) | −5.3% against 3920e3e | −4.5% |
| 13a4fa8 | dead class types need no drop (`temps`) | 200.45 → 197.62 G (−1.4%), 5.47 GiB | −1.3% |
| 3a9a365 | parse workers read, tokenize and parse files ahead of the main thread (`frontpar`, `PYRIGHT_RS_PARSE_AHEAD`) | +1.35%, +0.22 GiB | +1.8%, +0.04 GiB |
| 393676b | second front-end batch (`bind`) | 200.34 → 198.76 G (−0.8%), 5.56 GiB | 7.21 G |

At 3a9a365 the seven corpora match with 1 and 8 parse workers and with `PYRIGHT_RS_PARSE_AHEAD=check`; the pydantic-ai gate passes with 8 workers; `tools/miri.sh` passes all four suites under both models. At 393676b the `dist` build passes the seven corpora and the pydantic-ai gate.

## after the third optimization round

`main` 393676b on a quiet devbox (load average 1.2 to 1.9, other workspaces idle but for light jobs). `bench/compare.py target/release/pyright-rs,target/dist/pyright-rs <corpora>`, as above.

| corpus | pyright | release | dist | wall speedup |
|---|---|---|---|---|
| pydantic-ai | 135.90 s, 179.07 s CPU, 9.95 GiB | 23.29 s, 26.75 s CPU, 5.53 GiB | 19.65 s, 22.88 s CPU, 5.53 GiB | 5.8x, 6.9x |
| samples | 5.53 s, 9.57 s CPU, 0.63 GiB | 0.79 s, 0.87 s CPU, 0.47 GiB | 0.61 s, 0.67 s CPU, 0.48 GiB | 7.0x, 9.1x |
| typeshed | 3.30 s, 5.70 s CPU, 0.62 GiB | 0.55 s, 0.61 s CPU, 0.39 GiB | 0.44 s, 0.49 s CPU, 0.39 GiB | 6.0x, 7.5x |
| primer-attrs | 1.35 s, 2.59 s CPU, 0.28 GiB | 0.16 s, 0.16 s CPU, 0.15 GiB | 0.13 s, 0.14 s CPU, 0.16 GiB | 8.4x, 10.4x |
| primer-bidict | 0.60 s, 1.00 s CPU, 0.20 GiB | 0.06 s, 0.06 s CPU, 0.09 GiB | 0.05 s, 0.05 s CPU, 0.09 GiB | 10.0x, 12.0x |
| primer-more-itertools | 1.39 s, 2.66 s CPU, 0.28 GiB | 0.18 s, 0.19 s CPU, 0.16 GiB | 0.15 s, 0.15 s CPU, 0.17 GiB | 7.7x, 9.3x |
| primer-packaging | 1.49 s, 2.81 s CPU, 0.29 GiB | 0.18 s, 0.19 s CPU, 0.17 GiB | 0.14 s, 0.15 s CPU, 0.18 GiB | 8.3x, 10.6x |
| primer-python-chess | 1.90 s, 3.42 s CPU, 0.32 GiB | 0.24 s, 0.25 s CPU, 0.21 GiB | 0.19 s, 0.20 s CPU, 0.22 GiB | 7.9x, 10.0x |

`bench/measure.sh`, one run each, instructions:

| build | pydantic-ai | samples |
|---|---|---|
| `release` | 198.69 G | 7.22 G |
| `dist` | 162.72 G | 5.79 G |

Parse workers, `dist`, `PYRIGHT_RS_PARSE_AHEAD=on` against `off` in interleaved runs on the same quiet devbox:

| corpus | runs | wall, on → off | CPU, on → off | peak RSS, on → off |
|---|---|---|---|---|
| pydantic-ai | 6 pairs | 19.17 → 19.42 s (on is faster in every pair, by 0.11 to 0.34 s) | 22.4 → 21.3 s | 5.54 → 5.35 GiB |
| samples | 7 pairs | 0.61 → 0.61 s | 0.68 → 0.60 s | 0.48 → 0.45 GiB |
| typeshed | 7 pairs | 0.43 → 0.42 s | 0.48 → 0.42 s | 0.39 → 0.36 GiB |

The workers stay on by default: speed comes first, as for the trigger's taper. They cost the small corpora 0.03 GiB of peak and no wall time, and the higher small-corpus peaks in the table above are theirs.

From the last quiet measurement (232d44d, `dist`: 175.09 G, 20.53 s, 5.70 GiB on pydantic-ai), `dist` runs 7.1% fewer instructions, 4.3% less wall time and peaks 3% lower. From the first round's baseline (308f470: 388.7 G, 52.3 s, 11.8 GiB), 58% fewer instructions, 62% less wall time and a 53% lower peak.
