# tools/stubgen

Writes Rust signature stubs from pyright's TypeScript: one module per `.ts` file, every function and method with a `todo!()` body and a `// <file>.ts:<line>` marker.
PLAN.md step 3; the `evaluator` run is the step 5 skeleton. The rules it applies are in `docs/evaluator-design.md`.

## run

```sh
tools/stubgen/stubgen --run binder --check      # one run, then the compile check
tools/stubgen/stubgen --check                   # every run in runs.toml
tools/stubgen/stubgen --run evaluator --check-only
tools/stubgen/stubgen --run evaluator --install       # write the run's stubs into crates/pyright/src
```

`--install` places the stubs the last run wrote to `out/<run>/`; it does not regenerate them. After a change to the table or the generator, run `tools/stubgen/stubgen --run evaluator` first, then `--install`. `--install` works only for a run with `install = true` in `runs.toml`. It keeps every function already ported at its marker, and changes nothing when it would drop ported code (`docs/evaluator-design.md`, section 5, "install").

Needs `bun`, `cargo`, `rustfmt`, and the pinned pyright at `~/pyright` (or `PYRIGHT_ROOT`).
The generator uses that checkout's `typescript` and `smol-toml`: `link-deps` symlinks them into `tools/stubgen/node_modules`.
A run loads the check path (`pyright.ts` and its imports, 171 files), analyzes writes, and generates in about 5 s.

## outputs

`out/<run>/`:

- `stubs/`: the deliverable, laid out like `crates/pyright/src`. Only `--install` writes into `crates/`.
- `deps/`: type-only placeholders for the files the stubs name. The compile check uses them while the real files are skeletons. Never copy them.
- `REPORT.md`: marker coverage, the factory interface check, placeholders, unmapped TypeScript types with their sites, how each class and interface is held and why, renames.
- `CHECK.md`: the `cargo check -p pyright` result of `--check`, and the `jsrt` items the check had to shim.

## files

| file | what |
| --- | --- |
| `runs.toml` | runs: a name, the TypeScript files to stub, whether `--install` may write them, and the heap their code allocates in |
| `typemap.toml` | the mapping table: types, modes, traits, families, merges, factories, per-site overrides |
| `numbers.toml` | one Rust type per TypeScript `number` site; the generator adds new sites as guesses ending in `?` |
| `jsrt_shim.rs` | stand-ins for `jsrt` items `crates/jsrt` does not define yet, used only by the check |
| `src/program.ts` | loads the TypeScript program |
| `src/writes.ts` | write analysis: which fields, params and locals are written or changed in place |
| `src/gen.ts` | classification and the TypeScript-to-IR type mapping |
| `src/ir.ts` | the Rust type IR and its rendering per position |
| `src/emit.ts` | items: structs, families, traits, enums, aliases, functions, namespaces, factories |
| `src/check.ts` | the compile check and `--install` |
| `src/merge.ts` | carries ported functions into a regenerated stub, by marker |
| `src/callbacks.ts` | stored callbacks: a struct per closure, with its captures |
| `src/coverage.ts`, `src/ifacecheck.ts` | the report's coverage checks |

## change a mapping

1. Edit `typemap.toml`, or a value in `numbers.toml`. Drop the `?` from a number site once checked.
2. Run with `--check`. Fix what the check or the report shows.
3. When a rule changes, not just an entry, update `docs/evaluator-design.md` first.

Type-check the generator with `tools/stubgen/node_modules/typescript/bin/tsc -p tools/stubgen` after `link-deps` has run.
