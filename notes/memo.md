# memo notes

Cut evaluator work that pyright repeats with the same result, with caches and shortcuts pyright lacks (`AGENTS.md`, "gates").

## Signature changes

- `jsrt::switch`: `SwitchMode` (`Off`, `On`, `Check`) and `jsrt::switch_mode!("PYRIGHT_RS_...")`, which reads a switch once per process.
- `type_utils::get_class_iterator` takes `specialize: bool` and yields `(mro_class, Option<specialized>)`; `None` when `specialize` is false.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `switch` module.
- `docs/porting.md`: "switches" table and the `jsrt::switch_mode!` rule.

## Needs from others

## Doubts

## Attempts

Instructions are `bench/measure.sh <corpus> 1`, `release`, on devbox.

| change | switch | pydantic-ai | samples |
|---|---|---|---|
| specialize an MRO class only when a member is found in it | `PYRIGHT_RS_DEFER_MRO_SPECIALIZATION` | 245.33 G → 236.29 G (−3.7%) | 9.32 G → 9.23 G (−1.0%) |
