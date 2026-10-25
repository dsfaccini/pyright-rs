# The 2026-10-16 Go port attempt

Research report from 2026-10-20, extracted from Claude Code session `ff304c5a` (project `/Users/david/pydantic/ai/pyright`). All branches and files from that attempt were deleted; only the transcripts remain.

## 1. Why Go over Rust

Message uuid 2278b576-2044-42d3-8405-698f4f0bf6b7: Microsoft ported TypeScript's checker to Go line by line (TS 7). A port keeps algorithms and evaluation order; Rust's ownership model needs arenas, IDs and RefCell, and behaviour drifts. Pyright uses cyclic GC graphs and re-enters the evaluator with half-written caches, which Go handles unchanged. pyo3 does not fit; a Go binary can ship as a wheel (as ty does). The architect memory file (uuid db5e8de9-7e97-4616-8839-dd87265f9865) records Go and the full port, not the recommended spike.

## 2. Design

Target pyright main f2e1a53, CLI path only (~130k lines), mirror TypeScript file for file. Package layout: one Go package `pyright` under `packages/pyright-go/pyright/`, shared runtime in `packages/pyright-go/internal/`. PORTING.md was never written; only its planned topics appear (uuid 55b30933-8eb2-4693-b2c8-45b9de588482). No closure-to-evaluator mapping was found.

JavaScript-compatibility helpers in commit 934f83c: jsstr (1,449 cases pass), jsnum, jssort (V8 13.6 TimSort, matches Node on 225 cases), collections, jsre (RE2-based, 12,132 lines of cases pass).

Cache-empty finding (uuid 513e9290-4a34-4f6c-9d9b-ade9a4069c92): main's output changes when type-cache empties occur, which a port cannot reproduce; reference dumps must use a big heap.

## 3. Oracle

Format oracle-v1 (`oracle/FORMAT.md`, commit 9dd8188). Environment variables `PYRIGHT_ORACLE_DUMP`, `PYRIGHT_ORACLE_PHASES` (tok, tree, bind, types, flow, diag), `PYRIGHT_ORACLE_SCOPE`. Hooks in `src/oracle/oracleDumper.ts`, `program.ts`, `pyright.ts`, `codeFlowEngine.ts` (5 lines). Compare tool `oracle/compare` (Python, first divergence per file and phase).

Gates passed: diagnostics identical with dumps off and on; two-run determinism identical. Expectation extractor: 554 cases; 22 jest suites pass with the recorder on. The 1,444-case samples run reached 300/1,444 and never finished.

## 4. How far it got

Architect: last commit 934f83c; untracked `pyright/` and `tools/` files; no tokenizer or parser written. Stopped by laptop-sleep API errors at 06:42, 08:09 and 10:10. Oracle agent: commits f71fdfd, d944fc8, 9dd8188 and one around 14:22; ended 14:31 with exit code 137. David stopped the effort at 14:32.

## 5. Numbers

Measured: 23% CPU cut from the integrated performance fork on an M4; hot-path changes about −20%; type-cache wipes 7 to 1; oracle dumps 30 MB of diagnostics and 28 MB for typeshed.
Stated without a label: ~130k lines on the CLI path (analyzer 108k, parser 16k), `typeEvaluator.ts` 31k lines, type checking about 88% of a run.
Estimated (labelled "my estimate" in the transcript): native Go about 3x on one core.
Not produced: a Go-vs-V8 ratio, a porting rate, defect counts, cost or time.
