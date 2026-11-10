# oracle notes

Step 1 of `PLAN.md`: the comparison tool. Usage and gate results: `oracle/README.md`. Format: `oracle/FORMAT.md`.

## Signature changes

None in Rust. The pyright fork adds `TypeEvaluator.oracleReadTypeCacheEntry` and `SourceFile.getOracleDiagnosticLists`; the Rust port does not port them.

## Shared-file edits

None.

## Needs from others

- The Rust dumper in `crates/pyright/src/oracle/` (not part of this step) writes `oracle-run.txt` with `cacheEmptyCount: 0`; `oracle/corpus/run.py` reads it.
- The typeshed corpus needs the Rust side's bundled typeshed directory in `ORACLE_TYPESHED`.

## Doubts

- The types stage reads the main type cache at the end of the run. A file checked early can gain entries while later files are checked, so a types difference does not always point at that file's own check. Per-file dumps right after each check would localize better but miss lazily evaluated library files; the end-of-run dump covers both.
- Return types that were never inferred print as `Unknown` in the types stage, because printing must not run inference.
