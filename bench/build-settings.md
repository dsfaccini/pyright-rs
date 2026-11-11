# Build settings

Compiler, linker and allocator settings for pyright-rs, each measured alone, then combined. No source changes.
Measured 2026-11-11 on devbox (AMD Ryzen 7 7700, 8 cores and 16 threads, 62 GB) from `main` b110b3b, rustc 1.99.1 (LLVM 23).

## what ships

| profile | settings | build time | build command |
|---|---|---|---|
| `release` | fat LTO, one codegen unit | 72 s clean, 63 s after an edit to `pyright` (was 58 s and 51 s) | `cargo build --release` |
| `dist` | `release` plus `panic = "abort"`, PGO, BOLT, `target-cpu=x86-64-v2`, static relocation model | 2 to 3.5 min: two builds, two training passes, BOLT | `bench/build-dist.sh` |

| corpus | build | instructions | wall | peak RSS |
|---|---|---|---|---|
| pydantic-ai | `main` b110b3b | 388.7 G | 71.6 s (59.9 to 76.2) | 11.7 GB |
| | `release` | 370.6 G (−4.6%) | 61.6 s (53.2 to 62.3) | 11.7 GB |
| | `dist` | 307.1 G (−21.0%) | 46.3 s (44.7 to 56.2) | 11.8 GB |
| samples | `main` b110b3b | 13.95 G | 1.35 s | 0.68 GB |
| | `release` | 13.40 G (−4.0%) | 1.25 s (−7%) | 0.68 GB |
| | `dist` | 11.23 G (−19.5%) | 1.00 s (−26%) | 0.69 GB |

pydantic-ai: median of 3 runs, range in brackets, under load average 4 to 9.5 from other agents.
`main` and `dist` runs alternated; in each pair `dist` took 21%, 41% and 23% less time.
`release` ran in other windows; against `main` in the same window it took 14 to 15% less.
Samples: median of 8 runs, the three builds interleaved. `dist` here trained on samples among other corpora; see the training cross-check for corpora it never saw.

Check each `dist` build like `release`: `PYRIGHT_RS_BIN=target/dist/pyright-rs oracle/corpus/check.sh <dir> [corpus...]`.
BOLT's profile is sampled, so no two `dist` builds are byte-identical.
The build measured here matches pyright on all eight corpora, output and every dump.

### what `dist` needs

- `rustup component add llvm-tools`, for `llvm-profdata`.
- BOLT, optional: `llvm-bolt`, `perf2bolt` and `merge-fdata` on `PATH` or in `BOLT_DIR`.
  devbox has no BOLT and no root. Ubuntu's package unpacks without root, and its tools link LLVM statically:
  `apt-get download bolt-21 && dpkg-deb -x bolt-21_*.deb <dir>`, then `BOLT_DIR=<dir>/usr/lib/llvm-21/bin`.
  BOLT 21 reads rustc's LLVM 23 output, and `-update-debug-sections` keeps file and line in panic backtraces.
- BOLT samples the CPU's last-branch record with `perf record -j any,u`.
  devbox allows it unprivileged (`kernel.perf_event_paranoid` is 1; Zen 4 records 16 branches). `DIST_BOLT=0` skips BOLT elsewhere.
- `target-cpu=x86-64-v2` keeps the binary runnable on any x86-64-v2 machine. `DIST_CPU=native` tunes for the build machine instead.
- Training checks the seven small corpora twice, instrumented and then under `perf`. Name corpora to train on others; a pydantic-ai training run waits on the shared lock.

## method

- Same command, memory cap and lock as `bench/measure.sh`, plus `cycles:u` from the same `perf stat`.
  Instructions vary under 0.1% between runs. Cycles and wall time vary with the load other agents put on devbox:
  identical pydantic-ai runs of `main` took 283 G to 346 G cycles and 60 s to 76 s.
  So builds are compared on runs interleaved in one window, and small differences in cycles count only when they repeat.
- Output: every run's `--outputjson` diagnostics and summary hash to the same value as `main`'s.

## each lever alone

Change against `main` b110b3b. Samples: median of 5 to 10 interleaved runs. pydantic-ai: one run each; its cycles and wall time are within the noise above unless the change is large.

| lever | samples instructions | samples cycles | pydantic-ai instructions | build |
|---|---|---|---|---|
| fat LTO | −4.0% | −7 to −10% | −4.6% | +14 s clean, +12 s incremental |
| thin LTO | −3.6% | −1 to −5% | −3.8% | +7 s clean, +10 s incremental |
| `panic = "abort"` | −1.7% | none | −2.0% | −4 s |
| `panic = "abort"`, against fat LTO alone | −1.1% | −0.5% | −1.2% | |
| `panic = "abort"`, against `dist` with unwinding | −1.4% | −3% | −1.9% | |
| fat LTO plus PGO trained on the small corpora | −17.4% (−14.0% against fat LTO alone) | −21% | −18.5% (−14.6%) | see `dist` |
| BOLT on `main`'s build, trained on samples | −1.0% | −12% | −1.0% (trained on the small corpora) | +2 s, plus profiling |
| static relocation model (non-PIE) | −0.5% | −3.5% | −0.7% | none |
| `target-cpu=x86-64-v2` | −0.2% | −0.9% | −0.2% | none |
| `target-cpu=x86-64-v3` | −0.6% | −1.4% | −0.7% | none |
| `target-cpu=native` (znver4) | −1.3% | −2.2% | −1.4% | none |
| mimalloc 2.3 instead of 3.3 (`mimalloc/v2`) | none | none | +0.2% | none |
| `MIMALLOC_ALLOW_LARGE_OS_PAGES=1` | none | none | none; page faults unchanged at 1.22 M | none |
| `MIMALLOC_PURGE_DELAY=-1`, against fat LTO alone | none | none | none; page faults −17%, kernel cycles −15% | none |
| static glibc (`+crt-static`) | none | none | not run | none |
| lld identical-code folding (`--icf=all`) | none | none | not run | none |
| `-inline-threshold=500` on fat LTO | −3.1% | −1 to −2% | not run | +11 s |
| `-inline-threshold=1000` on fat LTO | −7.4% | −1% | not run | +33 s |
| BOLT `-hugify` on `dist` | none | none (iTLB misses unchanged) | not run | |

## PGO training cross-check

Both builds: fat LTO plus PGO, nothing else. Each column trains on one set and is measured on every corpus.

| corpus | trained on the seven small corpora | trained on pydantic-ai only |
|---|---|---|
| pydantic-ai | −18.5% instructions | −20.0% |
| samples | −17.4% instructions, −21% cycles | −16.4%, −22% |
| typeshed | −20.0%, −21% | −18.8%, −22% |
| primer-attrs | −19.7%, −20% | −19.3%, −21% |
| primer-bidict | −16.0%, −13% | −15.4%, −12% |
| primer-more-itertools | −19.9%, −20% | −19.5%, −20% |
| primer-packaging | −19.2%, −19% | −18.7%, −21% |
| primer-python-chess | −19.4%, −15% | −18.8%, −19% |

Instructions: a build loses 0.4 to 1.5 points on corpora it never trained on. Cycles (median of 5 interleaved runs): no consistent loss. `dist` trains on the small corpora by default: pydantic-ai stays unseen, and training needs no lock.

## findings

- Instrumentation miscounts two num_bigint functions. On the samples corpus the instrumented build counts blocks of `mac3` and `to_radix_le` 10^12 times or more per call. The two functions run a handful of times, so these counts are false.
  Kept, they dominate the profile's total, so PGO treats every other function as cold: samples instructions fell 4% instead of 17%.
  `bench/build-dist.sh` drops any function with a block count above 10^12 before the optimized build.
- PGO's value profiles ran out of counters at the default budget ("Unable to track new values"); `-vp-counters-per-site=4` removes the warning.
- `panic = "abort"`: the only `catch_unwind` outside tests, in `Heap::collect` (`jsrt/src/heap.rs`), clears mark bits before it re-raises a panic from marking; with abort the process ends instead, so no later collection can see stale marks.
  `pyright::main` maps a panicked analysis thread to exit status 2 (`FatalError`); with abort the process dies of SIGABRT instead. No panic hook is installed.
  Tests ignore the profile's panic setting. `release` keeps unwinding, since abort saves about 1% there.
- pydantic-ai runs at 1.3 instructions per cycle, samples at 2.3: pydantic-ai waits on memory more, so layout work (PGO, BOLT) moves it less than samples.
  A pydantic-ai run has about 60 M data-TLB load misses against 290 G cycles, so huge pages had little to win.
- mimalloc returns freed pages to the OS 1 s after they empty (`purge_delay`). On pydantic-ai each collection frees buffers that later allocations fault back in.
  With `MIMALLOC_PURGE_DELAY=-1` a run took 1.02 M page faults instead of 1.23 M and 12.9 G kernel cycles instead of 15.1 G, at 11.80 GB peak RSS instead of 11.73 GB (one run each; wall 50.8 s against 55.9 s, within the noise).
  mimalloc 3.3 has no compile-time default for this option, so shipping it takes a `mi_option_set` call before the first allocation: a source change, not made here.
