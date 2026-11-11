# memo notes

Cut evaluator work that pyright repeats with the same result, with caches and shortcuts pyright lacks (`AGENTS.md`, "gates").

## Signature changes

- `jsrt::switch`: `SwitchMode` (`Off`, `On`, `Check`) and `jsrt::switch_mode!("PYRIGHT_RS_...")`, which reads a switch once per process.
- `type_utils::get_class_iterator` takes `specialize: bool` and yields `(mro_class, Option<specialized>)`; `None` when `specialize` is false.
- `type_evaluator::uncached_evaluation_count()` and `note_uncached_evaluation()`: a per-thread count of evaluations whose results can change later. `get_type_of_expression` counts every call that answers from no complete main type cache entry (a miss, an incomplete entry, a speculative or TypeForm entry), and `SynthesizeMethodsDeferred::call` counts each synthesis. A computation that leaves the count unchanged read only entries that stay.
- `TypeEvaluator` has two Rust-only fields: `abstract_symbols_memo` (class details to `AbstractSymbolsMemoEntry`: the MRO `Rc` it was computed from and the symbols) and `stashed_speculative_stacks` (how many `disable_speculative_mode` calls hold a nonempty speculative stack). `AbstractSymbol` derives `Trace`.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `switch` module.
- `analyzer/types.rs`: `SynthesizeMethodsDeferred::call` counts an uncached evaluation.
- `analyzer/type_evaluator_types.rs`: `AbstractSymbol` derives `Trace`.
- `docs/porting.md`: "switches" table and the `jsrt::switch_mode!` rule.

## Needs from others

## Doubts

## Attempts

Instructions are `bench/measure.sh <corpus> 1`, `release`, on devbox.

| change | switch | pydantic-ai | samples |
|---|---|---|---|
| specialize an MRO class only when a member is found in it | `PYRIGHT_RS_DEFER_MRO_SPECIALIZATION` | 245.33 G → 236.29 G (−3.7%) | 9.32 G → 9.23 G (−1.0%) |
| remember `get_abstract_symbols` per class | `PYRIGHT_RS_ABSTRACT_SYMBOLS_MEMO` | 236.25 G → 232.57 G (−1.6%) | 9.23 G → 9.22 G |
