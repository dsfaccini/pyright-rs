# evalmem notes

Shrink the live evaluator data that sets pydantic-ai's peak RSS: types, function details and the evaluator's caches.

## Signature changes

- `TypeBase::set_special_form`, `set_instantiable_depth`, `set_type_alias_info`, `set_type_form` and `set_condition` write `None` without allocating `Type.props`, and `TypeBase::clone_type` leaves the clone's props unallocated when the source's are empty. Every read of `props` is `props?.<field>` in the TypeScript, so an empty props object and none read the same.
- `FunctionDetailsSharedCold.doc_string` and `ClassDetailsShared.doc_string` are `Option<Rc<str>>`, shared by the copies `{ ...shared }` makes. `FunctionType::create`, `create_instance`, `create_instantiable`, `clone_with_doc_string` and `ClassType::create_instantiable` take `Option<Rc<str>>`.
- The cold half of a handle struct (`TypeResult`, `FunctionDetailsShared`) is `jsrt::OptionRefCell<<Struct>Cold>`, one pointer: `cold()` returns `jsrt::cell::OptionRefOr`, which derefs to the cold struct. `TypeResult` is 32 bytes.
- `MemberAccessDeprecationInfo` and `MagicMethodDeprecationInfo` are held as `Rc` (`[modes]` "rc"): `TypeResultCold` is 112 bytes, down from 240.

## Shared-file edits

- `crates/jsrt/src/cell.rs`: `OptionRefCell::borrow_or`, `borrow_mut_or_default`, `OptionRefOr`.
- `tools/stubgen`: `emit.ts` emits a handle's cold half as `jsrt::OptionRefCell`; `typemap.toml` records the doc strings and the deprecation infos; `out/` regenerated. `docs/evaluator-design.md`, "cold fields".
- Call sites of the deprecation infos in `operations.rs` and `type_evaluator.rs`; of the doc strings in `constructor_transform.rs`, `type_evaluator.rs`, `type_guards.rs`, `type_utils.rs`.

## Needs from others

## Doubts

## Census

`PYRIGHT_RS_GC_CENSUS=1`, `--features pyright/count-malloc`, pydantic-ai, at exit after a major collection. Slot and buffer bytes of the live evaluator objects, MiB.

| item | `main` d91164c |
|---|---|
| evaluator total (slots + buffers) | 3,411 |
| `Type` | 862 + 664 |
| `FunctionDetailsShared` | 149 + 465 |
| `TypeResult` | 156 + 127 |
| the type cache map | 264 |
| `FunctionParam` vectors | 25 + 196 |
| `TypeEvaluator` | 103 |

Field counts on the same run (a temporary diagnostic, not committed):

- 7.54M live types: 4.91M classes, 1.30M functions, 0.91M unions, 0.32M type variables. 4.02M have `props` allocated, 2.78M of them empty (88 bytes each). 2.36M have `cached` allocated.
- 892K `FunctionDetailsShared` from 61K declarations: 215K hold a doc string, 429 MiB of them, deep copies made by each `{ ...type.shared }` clone.
- 539K `TypeResult` cold halves (240 bytes each): 438K for `overloadsUsedForCall` alone, 98K for `expectedTypeDiagAddendum`.

## Attempts

| change | pydantic-ai instructions | pydantic-ai peak RSS | samples instructions | samples peak RSS |
|---|---|---|---|---|
| `main` d91164c | 245.01 G | 7.10 GiB | 9.31 G | 0.50 GiB |
| empty `props` stay unallocated | 242.42 G (−1.06%) | 6.83 GiB | 9.26 G | 0.50 GiB |
| doc strings shared (`Rc<str>`) | 242.29 G (−0.05%) | 6.37 GiB | 9.26 G | 0.50 GiB |
| `TypeResult` 40 → 32 bytes, `TypeResultCold` 240 → 112 bytes | 241.17 G (−0.46%) | 6.16 GiB | 9.26 G | 0.48 GiB |
