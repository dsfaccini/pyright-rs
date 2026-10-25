# pyright-rs

A faithful Rust port of pyright's command-line check.
The goal: `pyright-rs --outputjson` prints the same output as pyright on the same project and config, and runs faster.

## the spec

The spec is pyright at commit `f2e1a5344576ae07730fcca2c4497a9453a7ab11` (1.1.414), checked out at `~/pyright`.
When the Rust output differs from pyright's, treat the Rust code as wrong.
Port pyright's quirks too, even where pyright's output looks wrong.

Mirror pyright's layout: write one `.rs` file per ported `.ts` file.
Mark each ported function with `// <file>.ts:<line>` of its TypeScript original.
The markers keep each Rust function reviewable next to its original when upstream changes.

## scope

In scope: the check path that starts at `packages/pyright-internal/src/pyright.ts`, with `--outputjson`.
Out of scope: the language server, `--watch`, `--threads`, `--verifytypes` and `--createstub`.

## reference output

Produce reference output only from pyright runs that never empty the type cache.
Run pyright with `--max-old-space-size=16384`, and check that `--verbose` prints no "Emptying type cache".
Pyright's own output changes after a cache empty, so such a run is not a valid reference.

## gates

Never land a change that turns a passing case into a failing one.
Give every cache or shortcut that pyright lacks a switch that turns it off.
Give it a check mode that panics when its result differs from the uncached path.

## plan

`PLAN.md` holds the port plan and its milestones. `docs/research/` holds the research behind it.
