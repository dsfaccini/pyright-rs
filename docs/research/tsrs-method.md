# tsrs method, extracted for a pyright Rust port

Research report from 2026-10-20. `B` = https://github.com/oxc-project/tsrs/blob/main/ and `C` = https://github.com/oxc-project/tsrs/commit/

Two findings first:
- Upstream tracking has not started. tsrs's TypeScript pin has not moved since 2026-09-30.
- The commit timestamps cannot be trusted as a port duration. The repo was recreated from rewritten history on 2026-10-01.

## 1. Port source and reuse

- Ported line by line from the Go code (`tsc/internal` in microsoft/TypeScript, pinned at `b85298b6`), not from `checker.ts`. "Go is the spec" (B/CONTRIBUTING.md, B/docs/PORTING.md).
- TypeScript's own scanner, parser, binder and AST were ported. There are no `oxc_*` dependencies (every Cargo.toml grepped). The AST is generated from TypeScript's `ast.json` (B/docs/AST.md).
- The only stated reason is fidelity: "Every Rust function should be recognizable next to its Go original", which makes upstream updates reviewable. Inferred: Oxc was skipped because positions, diagnostic order and AST shape must match exactly.

## 2. Process

1. Contracts first: PORTING.md, the arena pointer, and the AST and checker contracts (C/b7daba3b8e56895d7dc7a66f35a1b25a723095a7).
2. Front-end crates in parallel, each with a Go oracle. The scanner matched 12,865/12,865 files (C/6dcff3199fd0fa8ccba09585799ebd35229813f3).
3. Hand-written checker data model.
4. `tools/gosig` generated Rust signature stubs with `todo!()` bodies and `// file.go:LINE` markers. It decided `P<T>` versus `Option<P<T>>` with a nil analysis (https://github.com/oxc-project/tsrs/blob/f4450609b3f5d3d2667523c1f7b2d8d4e7a67204/tools/gosig/README.md).
5. Body wave per BODY_PORTING.md (https://github.com/oxc-project/tsrs/blob/741cb2cb28e849558187738f205294a580fbea95/docs/BODY_PORTING.md):
   - One agent per Go line range, each in its own worktree, with fixed signatures and a rule that the crate compiles.
   - Each agent kept a notes file with "Signature changes / Shared-file edits / Needs from others / Doubts".
   - 21 checker branches and 6 node-builder branches were merged through an integration branch.
6. Fix waves by failure cluster (B/docs/DEBUGGING.md): trunk commits, and `pass.txt` may never shrink.

Porting order (B/docs/STATUS.md):
1. Checker with a placeholder printer: 10,479 passes.
2. Node builder: 13,398.
3. Declaration diagnostics: 13,458.
4. `.types`/`.symbols` baselines.
5. Performance and memory work.
6. LSP.
7. Emit waves, with at most 2 Opus sub-agents at a time (https://github.com/oxc-project/tsrs/blob/34be6024ef9046e488824543318ea0a2a0bd1c23/docs/EMIT.md).
8. Incremental builds, `--build` and watch mode.

Ongoing work:
- B/AGENTS.md says to search the notes before perf work and sets numeric landing bars.
- An issue-triage skill runs reproduce, diagnose, verify and fix stages against the pinned Go (B/.agents/skills/triage/SKILL.md).

Upstream tracking has not started. A 2026-10-08 audit found the pin 57 commits behind and traced all 95 failing tests to specific upstream commits (B/notes/upstream-gaps-2026-10-08.md). The blog promises a daily agent that "fetches diffs, and opens tracking issues" (https://blog.cloudflare.com/typescript-rust-port).

## 3. Conformance oracle

- TypeScript's suite through a ported harness: 13,458/13,462 error baselines, plus all 12,779 `.types`/`.symbols` baselines, which record the type of every expression and the symbol of every identifier (B/docs/COMPATIBILITY.md).
- Go oracles for each layer live in B/tools/oracle.
- Real code: a 38k-file monorepo where symbol, type and instantiation counters match exactly; mutation testing over 4,379 sites with 0 differences (B/notes/fix-mutation.md); top-400 repos, DefinitelyTyped, and parser fuzzing with 320k mutants and 0 divergences (B/docs/RUST.md).
- `fuzz-derived-variance.md` is not a conformance fuzzer. It attacked a shortcut that exists only in tsrs, found it wrong in 46% of generated programs, and the shortcut was deleted.
- Triage rule: if tsrs differs from Go, tsrs is wrong, even when its output looks better. "Port the quirk." Every cache that exists only in tsrs has an off switch and a shadow mode that panics on any disagreement (B/docs/DEBUGGING.md).
- History dependence: tsgo's output depends on which checker gets which file. `--checkerAssignment go` reproduces tsgo exactly, and every harness runs that way. The default canonical mode fixes specific caches so output depends only on the program, checked with random assignments (B/notes/perf-order-independence.md, B/notes/fix-history-dependent-diagnostics.md). Cases still open are tsgo's own (B/notes/open-history-dependence.md).

## 4. Rust architecture

- Handles: `P<T>` is a Copy, 32-bit handle. Equality is by identity, there are no lifetimes, and there is one leak arena per thread. Compression saved 14–15% peak memory for +6.5% instructions (B/notes/mem-pointer-compression.md).
- Mutability: checker arena fields are `Cell`/`RefCell`. The AST and binder output use `OwnedCell`/`FrozenCell`: written once, then frozen and shared.
- Links: `LinkStore<K,V>` is keyed by arena object and created on first use. It returns a handle, so no borrow of the checker is held.
- Checker: `&mut self`, with callbacks that receive the checker as an argument (B/docs/CHECKER.md).
- Parallelism: each checker owns its types, idle checkers steal work, and lazy members are on with an exact opt-out (B/notes/lazy-members.md).
- Rejected after measurement: global interning (+9% parse time), per-scope bump regions, speculative rollback and a tracing GC (B/docs/RUST.md).

## 5. Timeline and effort

- Dates as recorded in git: first commit 2026-09-30 05:44 UTC, about 7 hours after the pinned TypeScript commit; 13,458 conformance passes logged the same day; 0.18.0 shipped 10-19. These timestamps are unverified because of the rewritten history (B/docs/STATUS.md).
- 1,796 commits. Max Schwenk made about 1,292, github-actions 304 and Boshen about 135. PR numbers reach #643.
- "Written largely by AI coding agents, directed and reviewed by Max Schwenk" (B/CONTRIBUTING.md). Opus sub-agents ran under a coordinator (B/notes/mem-round4.md). Inferred: Codex was also used, given the `codex/*` branches.
- Budget mattered: emit was paused because "the usage budget went to the LSP port".
- The Rust is 0.7–1.3x the size of the Go.

## 6. Other checkers and lessons

- tsrslint ports tsgolint onto tsrs with the same identical-output rule (https://github.com/maschwenk/tsrslint).
- Upstream PRs came out of tsrs prototypes: microsoft/TypeScript#64600 and #64601 (B/upstream/README.md).
- Lessons: see the "Measured and rejected" list (B/notes/perf-round2-followups.md). A cache, thread or invariant needs a clear margin over the landing bar (B/AGENTS.md).

## What transfers to a pyright port (ranked)

1. TypeScript pyright at a pin is the spec. One `.rs` file per `.ts` file and `// typeEvaluator.ts:LINE` markers. `typeEvaluator.ts` (31,218 lines) is about the size of `checker.go`, so split it by line range.
2. Contracts, then generated stubs, then body waves with the notes file. Inferred: TypeScript's `| undefined` gives optionality directly, so stub generation is simpler than gosig's nil analysis.
3. Build a dense oracle before porting. Pyright's tests only check error counts (`validateResults`), which is far too weak. Freeze full diagnostics for the 1,353 samples, add a dump of every expression's evaluated type, then real projects and mutation testing.
4. Fix waves by cluster, with a gate that the pass list never shrinks.
5. Arena, identity handles and a `LinkStore` for `typeCache` and `AnalyzerNodeInfo`. The evaluator becomes `&mut self`.
6. Keep the reference evaluation order as the only mode. Pyright's incomplete types depend on that order.
7. Shadow modes for any cache that exists only in Rust.
8. Daily upstream tracking from day one.

Not transferable: tsgo's multi-checker machinery.
