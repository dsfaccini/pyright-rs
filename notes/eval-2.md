# eval-2 notes

Share C of step 5: the bodies under `typeEvaluator.ts:7678-15241` in `analyzer/type_evaluator.rs`.

## Signature changes

- `Arg` accessors through `typemap.toml` `[items]`: `arg_category()`, `node()`, `name()`, `type_result()`, `value_expression()`, `active()`, `enforce_iterable()`. TypeScript reads these `ArgBase` members on the `Arg` union; each accessor reads the member from whichever variant holds the argument.

## Shared-file edits

- `tools/stubgen/typemap.toml`: the `Arg` item above.
- `crates/pyright/src/analyzer/type_evaluator_types.rs`: the installed `Arg` item.
- `stubgen --install` drops the hand-added `use crate::{analyzer, common, parser};` line of `type_evaluator.rs`, `types.rs` and `type_printer.rs`; this branch restores it by hand after the install.

## Needs from others

## Doubts
