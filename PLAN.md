# Port plan

Goal: `pyright-rs --outputjson` output identical to pyright's on pydantic-ai, then measure the speedup.
Method: copied from oxc-project/tsrs (see `docs/research/tsrs-method.md`), adapted to a TypeScript-to-Rust port.
tsrs ported from Go code that was already statically typed; this port goes from TypeScript directly, so evaluation order is the main risk.

## size

The check path is about 135k lines of TypeScript (see `docs/research/pyright-map.md`):
- about 105k lines, parser through checker, need a line-by-line port;
- about 28k lines (utilities, import resolution, program, CLI) can be rewritten from behavior;
- typeshed and the English message strings ship as data.

## steps

1. **Comparison tool.** A TypeScript-side dumper in a pyright fork, plus a compare tool that reports the first difference per file and stage.
   - Stages: tokens, parse tree, scopes and symbols, flow nodes, the printed type of every expression (read from the cache only), diagnostics.
   - Corpus: pyright's 1,353 test sample files, typeshed, pydantic-ai, a slice of mypy_primer projects.
2. **Ground rules in code.** Arena handles (`jsrt::P<T>`, a pointer into a leak arena; its API hides the width, so 32-bit compression can come later as a measured change) and `RefCell` fields for types; insertion-ordered maps everywhere; JavaScript string semantics (UTF-16 positions, lone surrogates); helpers for V8's sort, pyright's `hashString`, `String.replace` with `$&`, NFKC; the type evaluator as one struct with `&self` methods.
3. **Stub generator.** TypeScript signatures to Rust stubs with `todo!()` bodies and `// file.ts:LINE` markers.
4. **Milestone 1: tokenizer, parser, binder** (about 30k lines).
   - Done when dumps and parse/bind diagnostics match on the whole corpus.
   - Checkpoint: compare Rust parse+bind time against TypeScript, and report to David before step 5.
5. **Evaluator, helpers and code flow as one block** (about 69k lines), split by line range: one agent per range, one worktree each, the crate always compiles. Then the checker. David sets the number of parallel agents at the checkpoint.
6. **Fix rounds** by failure cluster, then the full pydantic-ai check and benchmarks.
   - Memory and speed work, ranked in `docs/research/memory-advice.md`: a collector for the evaluator heap, free tokens after parse, exact-size variants, a dense type cache, 32-bit handles, parallel parse.
7. **Upstream sync.** About 3k changed core lines a month, rising. A daily job ports upstream commits after the pin.

## hazards to settle in step 2

See `docs/research/pyright-map.md`, section 2: evaluation order and speculative rollback, mutable cyclic types, the closure-based evaluator, UTF-16 semantics, non-total sort comparators, Map insertion order, JS-only output paths, heap-driven cache emptying, `String.replace` patterns.

## prior attempt

A Go port started on 2026-10-16 and stopped before the tokenizer. Its comparison-tool design is in `docs/research/go-attempt.md`.
