# trigger notes

Lower pydantic-ai's peak RSS at the default settings without the instructions that collecting more often costs.

## Signature changes

- `jsrt::heap::collect_if_due(left: f64)` takes the share of the check left, from 1 at the first user file to 0 at the last. `Program::analyze` passes it.
- `jsrt::heap::epoch()`: collections finished on the thread's heap. `Tracer::young_epoch()`: in a minor collection, the epoch whose writes a logged map must trace; `None` traces everything.
- `jsrt::LoggedMap<M>` and `jsrt::LogKeyed`: a map keyed by a `u32` id that logs its writes. Read through `Deref`; change the value of a key only through `write(key)`.
- `IdMap` has a written `Trace` impl that stamps pages with the epoch of their last insert.
- `TypeEvaluator.code_flow_analyzer_cache`, `effective_type_cache` and `expected_type_cache`, `SpeculativeTypeTracker._speculative_type_cache` and `CodeFlowEngine.reachability_cache` are `LoggedMap`s.

## Shared-file edits

- `crates/jsrt/src/heap.rs`: epoch, `Tracer::young_epoch`, `PYRIGHT_RS_GC_WRITE_LOGS`, `PYRIGHT_RS_GC_TAPER`, card scan eight cards at a time, sweep skips words with nothing dead, a unit test.
- `crates/jsrt/src/collections.rs`, `lib.rs`: `IdMap` page stamps, `LoggedMap`.
- `crates/pyright/src/analyzer/type_evaluator.rs`, `type_cache_utils.rs`, `code_flow_engine.rs`: the five caches above and their write sites.
- `crates/pyright/src/analyzer/program.rs`: the share of user files left.
- `docs/collector-design.md` ("write logs", "sweep", "trigger"), `docs/porting.md` ("memory", "switches").

## Needs from others

## Doubts

- `PYRIGHT_RS_GC_TAPER` reads progress as the share of user files left. On pydantic-ai the files are uneven: the last 30% allocate 8% of the evaluator slots, and files 1,228 to 1,433 of 2,046 allocate about 1 GiB. None of the seven small corpora collects at the default floor, so pydantic-ai is the only corpus the taper changes.

## Where the peak comes from

`main` f02fe2a, pydantic-ai, `count-malloc` build with `PYRIGHT_RS_GC_LOG=1` and the census:

- 10 collections (2 major). Each 512 MiB interval leaves about 400 MiB of garbage slots and 370 MiB of garbage buffers; the collection log's malloc column falls 340 to 470 MiB per sweep.
- The resident set does not fall after a sweep: freed slots and malloc pages stay resident and the next interval reuses them. It peaks at the last full interval (collection 10, before file 1,780 of 2,046): live set plus that interval's garbage.
- End of check, default against `PYRIGHT_RS_GC_MIN_MIB=64`: evaluator range 1,484 against 1,186 MiB (garbage 166, free slots 155 and warm blocks 52, against 56 and 8), malloc heap 2,795 against 2,501 MiB. Half the 0.6 GiB gap is free memory left resident by earlier, larger intervals, not garbage.
- Live set and garbage follow a rising live set, so every interval ends at a new high. A trigger on resident-set growth collects when the freed capacity is used up; each interval then shrinks by its survivors (about 35%), so it converges to the floor's cost. A trigger can only lower the peak cheaply by spacing collections unevenly: wide early, close at the end.

## What collecting more often cost

`main` at 64 MiB costs 12.2 G instructions (5.7%) more than at 512. IBS op samples, 64 against 512: the five large caches 4.1% of the base, other traces 0.75%, `mark_eval` 0.59%, the sweep loop 0.40%, `drain` 0.32%, the card scan 0.29%.

Serial root tracing per minor collection late in the check (one marking thread, 64 MiB): 77 ms, of which the type cache 28 ms, `TypeEvaluator` 26 ms, the speculative type tracker 14 ms, 52 K `Symbol`s 5 ms. Each minor collection traced these caches whole because every file writes them, which marks their cards. Sizes at the end of the check: type cache 3.71 M entries; expected-type cache 656 K (786 K candidates); effective-type cache 192 K symbols, 534 K entries; code-flow-analyzer cache 55 K nodes, 119 K entries; speculative type cache 424 K nodes, 564 K entries.

With write logs, late minor collections at 64 MiB take 46 ms instead of 90 (one marking thread), and the logs hold about 4% of each map's entries.

## Attempts

pydantic-ai, release build, one run unless noted; instructions repeat within 0.1%. Peak RSS varies by up to about 0.05 GiB between runs. "LOG" runs had `PYRIGHT_RS_GC_LOG=1`, which adds a few tens of millions of instructions per collection.

| change | instructions | peak RSS | collections |
|---|---|---|---|
| `main` f02fe2a, default (3 runs) | 213.36 G | 5.69 GiB | 10 |
| `main`, `PYRIGHT_RS_GC_MIN_MIB=64` (3 runs) | 225.53 G | 5.07 GiB | 72 |
| write logs, `LoggedMap` traced in full once its log reaches its entry count | 213.68 G (3 runs) | 5.69 GiB | 10 |
| write logs, full trace past an eighth of the entries | 213.06 G | 5.68 GiB | 10 |
| the same, also `reachability_cache` (kept, 9f149c0) | 213.02 G; 64 MiB: 218.93 G | 5.68; 64 MiB: 5.05 GiB | 10; 72 |
| card scan eight at a time, sweep skips clean words (kept, afedf76; 3 runs interleaved) | 212.99 G; 64 MiB: 218.25 G | 5.68; 64 MiB: 5.06 GiB | 10; 72 |
| dropped: the sweep's free-slot test folded into its word loop | 213.04 G; 64 MiB: 218.42 G | 5.67 GiB | 10 |
| afedf76, `PYRIGHT_RS_GC_MIN_MIB=256` | 215.01 G | 5.36 GiB | |
| afedf76, `PYRIGHT_RS_GC_MIN_MIB=128` | 216.87 G | 5.16 GiB | |
| taper on `main`'s collector, k = 4 (LOG) | 214.17 G | 5.53 GiB | 12 |
| the same, k = 2.5 (LOG) | 214.42 G | 5.43 GiB | 13 |
| the same, k = 1.8 (LOG) | 214.95 G | 5.35 GiB | 15 |
| the same, k = 1.3 (LOG) | 215.94 G | 5.27 GiB | 18 |
| taper on afedf76, k = 1.3 | 213.90 G | 5.27 GiB | 18 |
| taper, k = 1 (kept as `PYRIGHT_RS_GC_TAPER=1`, a8a9129; 3 runs) | 213.87 G | 5.19 GiB (median 5.18) | 22 |
| taper, k = 0.8 | 214.51 G | 5.16 GiB | 26 |
| taper ending at 32 MiB instead of 64, k = 1 | 214.05 G | 5.18 GiB | |
| the same, k = 1.2 | 213.97 G | 5.23 GiB | |
| taper floor × (k × share left)^p, k = 1.6, p = 1.5 (LOG) | 213.77 G | 5.26 GiB | 19 |
| the same, k = 1.25, p = 1.5 (LOG) | 214.05 G | 5.21 GiB | 23 |
| the same, k = 1.4, p = 2 (LOG) | 214.58 G | 5.23 GiB | 24 |
| buffer-aware: slot bytes plus net malloc growth since the last collection (`CountingAlloc`), floor 1 GiB; its `count-malloc` base is 214.52 G, 5.67 GiB, 10 collections | 212.97 G | 5.74 GiB | 12 |
| the same, floor 512 MiB | 219.38 G | 5.54 GiB | 23 |
| `MIMALLOC_PURGE_DELAY=0` on `main` | 213.28 G | 5.62 GiB, wall +1.2 s | 10 |
| `MIMALLOC_ALLOW_THP=0` on `main` | 213.39 G | 5.59 GiB | 10 |

- Write logs and the per-collection cuts lower the default by 0.18% and the 64 MiB row by 3.2%; 64 MiB still costs 2.5% more than the default, mostly survivors marked more often and the sweep's fixed cost.
- The buffer-aware trigger at an equal number of collections changes no peak: it spaces collections evenly by slots and buffers, but the peak comes from the last interval's size. Its floor-1 GiB run is cheaper because it skipped the second major collection (1.5 G); the arena's 1 GiB chunk allocation triggered one 51 MiB collection.
- The taper with k = 1 has the 64 MiB row's peak to within 0.12 GiB for 0.24% more instructions than `main`, 0.41% more than afedf76. Its intervals shrink from about 450 MiB after the first collection to 64 MiB at the end.

Wall time, 3 runs interleaved: `main` 26.20 s, afedf76 26.02 s, taper 26.74 s; another pair, `main` 25.39 s, taper 26.15 s. Collection pauses add 0.21 to 0.24 s (22 collections against 10). Cycle samples on the main thread: mutator +0.78%, collector +0.64%; the mutator allocates into swept, cache-cold memory instead of fresh pages more often (`_mi_page_malloc_zero` +9%, `P::new_eval`, `TypeBase::clone_type_with`). So the taper is off by default.

samples, 8 runs interleaved: `main` 7.548 G, write logs 7.551 G, afedf76 7.551 G, taper 7.550 G, peak 0.45 GiB throughout. None of the seven small corpora collects at the default floor.

Correctness beyond the gates: the seven corpora identical with `PYRIGHT_RS_GC_MIN_MIB=0`, and in check mode with an 8 MiB floor; pydantic-ai in check mode (`PYRIGHT_RS_GC_MIN_MIB=128`, tapered) ran 57 minor collections, each followed by a full mark, with no miss.

## Remaining

Estimates, not measured:

- Majors: the buffer-aware run skipped one major and saved 1.5 G. A later major (`PYRIGHT_RS_GC_MAJOR_RATIO` above 1) trades instructions for old garbage at the peak.
- A progress signal closer to the allocation left than the files left would place the taper's collections better on pydantic-ai, whose files 1,228 to 1,433 allocate about 1 GiB.
- The taper's wall cost is mostly the mutator's reuse of swept memory; fewer, better-placed collections are the lever, not cheaper ones.
