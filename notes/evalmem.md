# evalmem notes

Shrink the live evaluator data that sets pydantic-ai's peak RSS: types, function details and the evaluator's caches.

## Signature changes

- `TypeBase::set_special_form`, `set_instantiable_depth`, `set_type_alias_info`, `set_type_form` and `set_condition` write `None` without allocating `Type.props`, and `TypeBase::clone_type` leaves the clone's props unallocated when the source's are empty. Every read of `props` is `props?.<field>` in the TypeScript, so an empty props object and none read the same.
- `TypeBase::clone_type_as_instance`, `clone_type_as_instantiable` and `clone_with_type_form` apply their props writes to the clone's copy before storing it (`clone_type_with_props`), so a clone whose props end up empty allocates none.
- `FunctionDetailsSharedCold.doc_string` and `ClassDetailsShared.doc_string` are `Option<Rc<str>>`, shared by the copies `{ ...shared }` makes. `FunctionType::create`, `create_instance`, `create_instantiable`, `clone_with_doc_string` and `ClassType::create_instantiable` take `Option<Rc<str>>`.
- The cold half of a handle struct (`TypeResult`, `FunctionDetailsShared`) is `jsrt::OptionRefCell<<Struct>Cold>`, one pointer: `cold()` returns `jsrt::cell::OptionRefOr`, which derefs to the cold struct. `TypeResult` is 32 bytes.
- `FunctionDetailsShared.name`, `full_name` and `module_name` are atoms. `FunctionType::create` and its callers still take `&str`. Take `.to_string()` where a `String` is needed.
- `FunctionParam.name` is `Option<Atom>`: `FunctionParam` is 40 bytes. `.as_deref()` still gives `Option<&str>`.
- The type caches (`TypeEvaluator.type_cache`, `return_type_inference_type_cache`, `SpeculativeCache::Type`) are `TypeCacheMap`, a `jsrt::IdMap<StoredTypeCacheEntry>`: `get`, `insert` and `remove` take the node id by value. `StoredTypeCacheEntry::new(entry)` packs a `TypeCacheEntry` into 16 bytes, `.get()` unpacks it. `incomplete_gen_count` must stay below 2^31 (asserted).
- `Type` is a header type (docs/porting.md, "memory"): a type is allocated as `TypeAlloc<D>`, the 24-byte header followed by the details of its category, so it takes the size of its own details (class 96 bytes, function 104, union 104, type variable 96, overloaded 64). Typed handles' `d()` read the details through the handle (`P::as_ptr`); `cast()` tests `category`. `types.rs` builds types through `Type::alloc_eval` and `alloc_program` from a `TypeInit`.
- `TypeResult.class_type` lives in `TypeResultCold`: read `tr.cold().class_type`. `TypeResult` is 24 bytes.
- `MemberAccessDeprecationInfo` and `MagicMethodDeprecationInfo` are held as `Rc` (`[modes]` "rc"): `TypeResultCold` is 112 bytes, down from 240.

## Shared-file edits

- `crates/jsrt/src/collections.rs`: `IdMap`, a map keyed by a dense `u32` id in pages of 32 slots.
- `crates/jsrt/src/cell.rs`: `OptionRefCell::borrow_or`, `borrow_mut_or_default`, `OptionRefOr`.
- `tools/stubgen`: `emit.ts` emits a handle's cold half as `jsrt::OptionRefCell`; `typemap.toml` records the doc strings and the deprecation infos; `out/` regenerated. `docs/evaluator-design.md`, "cold fields".
- Function names: `checker.rs`, `constructor_transform.rs`, `data_classes.rs`, `function_transform.rs`, `type_evaluator.rs`, `type_utils.rs`.
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
- The type cache map at `main` d91164c: 3.71M entries, 7.34M buckets (264 MiB), node ids up to 13.3M. Keys fill 60% of the 16-id pages they touch, 59% at 32, 56% at 64, 51% at 256, 45% at 1024. 17 return-type inference caches over the run, 505 entries in all.
- At 67ce98a: 570K live props boxes still empty, nearly all from `clone_type_as_instance` (copy the props, then clear `type_form`); `clone_type_as_instantiable` and `clone_with_type_form` next. No live `TypeResult` sets `class_type` or `is_asymmetric_accessor`; 373K set `is_required` and `is_not_required`, 93K `is_read_only`.
- 539K `TypeResult` cold halves (240 bytes each): 438K for `overloadsUsedForCall` alone, 98K for `expectedTypeDiagAddendum`.

## Attempts

Peak RSS at the default trigger moves with the collection phase: a collection runs every 512 MiB of evaluator slots, and the buffers garbage owns are not counted, so the garbage left at the end of the check (where pydantic-ai peaks) varies by up to about 0.6 GiB. `PYRIGHT_RS_GC_MIN_MIB=64` collects every 64 MiB and tracks the live set more closely; both are listed from `main` baa992f on. The exact-size `Type` change cut the live type slots from 862 to 776 MiB, yet its default-trigger peak rose 0.04 GiB.

| change | pydantic-ai instructions | pydantic-ai peak RSS | samples instructions | samples peak RSS |
|---|---|---|---|---|
| `main` d91164c | 245.01 G | 7.10 GiB | 9.31 G | 0.50 GiB |
| empty `props` stay unallocated | 242.42 G (−1.06%) | 6.83 GiB | 9.26 G | 0.50 GiB |
| doc strings shared (`Rc<str>`) | 242.29 G (−0.05%) | 6.37 GiB | 9.26 G | 0.50 GiB |
| `TypeResult` 40 → 32 bytes, `TypeResultCold` 240 → 112 bytes | 241.17 G (−0.46%) | 6.16 GiB | 9.26 G | 0.48 GiB |
| function names are atoms (`FunctionDetailsShared` 168 → 120 bytes) | 241.38 G (+0.09%) | 6.08 GiB | 9.26 G | 0.48 GiB |
| parameter names are atoms (`FunctionParam` 48 → 40 bytes) | 241.30 G (−0.03%) | 6.04 GiB | 9.28 G (+0.2%) | 0.48 GiB |
| type caches as `jsrt::IdMap` pages of 16-byte entries, 256 slots a page | 240.40 G | 5.76 GiB | 9.24 G | 0.51 GiB |
| the same, 64 slots a page | 240.37 G | 5.77 GiB | 9.24 G | 0.49 GiB |
| the same, 32 slots a page (kept) | 240.38 G (−0.38%) | 5.76 GiB | 9.24 G (−0.4%) | 0.48 GiB |
| `main` 96d0cc0 (memo merged) | 231.45 G | 5.74 GiB | 9.17 G | 0.47 GiB |
| clones that clear props build them first (`clone_type_with_props`) | 230.64 G (−0.35%) | 5.72 GiB | 8.93 G (−2.6%) | 0.47 GiB |
| `TypeResult.class_type` in the cold half (`TypeResult` 32 → 24 bytes) | 231.12 G (+0.21%) | 5.70 GiB | 8.93 G | 0.47 GiB |
| `main` baa992f (sound merged); default trigger / `PYRIGHT_RS_GC_MIN_MIB=64` | 227.50 G | 5.76 / 5.25 GiB | 9.14 G | 0.47 GiB |
| the props and `class_type` commits rebased | 227.17 G | 5.69 / 5.16 GiB | 8.89 G | 0.46 GiB |
| `Type` exact-size: header plus the details of its category | 226.80 G (−0.16%) | 5.73 / 5.07 GiB | 8.88 G | 0.45 GiB |
| `LiteralValue` 32 → 16 bytes: class types 104 → 96 bytes | 226.62 G (−0.08%) | 5.58 / 5.03 GiB | 8.87 G | 0.45 GiB |
| dropped: `FunctionDetailsShared` 120 → 96 bytes (`type_params` an `Rc<Vec>`, `type_var_scope_id` a `Cell`) | 231.83 G (+0.16%) | 5.74 GiB | 9.15 G | 0.47 GiB |
