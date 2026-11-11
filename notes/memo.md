# memo notes

Cut evaluator work that pyright repeats with the same result, with caches and shortcuts pyright lacks (`AGENTS.md`, "gates").

## Signature changes

- `jsrt::switch`: `SwitchMode` (`Off`, `On`, `Check`) and `jsrt::switch_mode!("PYRIGHT_RS_...")`, which reads a switch once per process.
- `type_utils::get_class_iterator` takes `specialize: bool` and yields `(mro_class, Option<specialized>)`; `None` when `specialize` is false.
- `type_evaluator::uncached_evaluation_count()` and `note_uncached_evaluation()`: a per-thread count of evaluations whose results can change later. `get_type_of_expression` counts every call that answers from no complete main type cache entry (a miss, an incomplete entry, a speculative or TypeForm entry), and `SynthesizeMethodsDeferred::call` counts each synthesis. A computation that leaves the count unchanged read only entries that stay.
- `DiagnosticAddendum::add_message_format(message, params)`: `add_message(message.format(params))` with the formatting deferred to the first read (`get_messages`, `get_string`). The hot `assign_type`, `assign_class`, `assign_type_args`, `assign_to_union_type`, `assign_from_union_type` and `get_type_of_class_member_name` sites use it. `ParameterizedString` derives `Trace`.
- `CodeFlowEngine.reachability_narrowing_memo`: per conditional flow node id, whether narrowing made it unreachable in a reachability walk.
- `type_evaluator::note_type_mutation()` and `type_mutation_epoch()`: a per-thread epoch that ends every `print_type` and `combine_types` memo entry. Call `note_type_mutation()` before any write that changes, in place, a field the printer reads on a type object that may already be printed. Writes to an object during its own construction need none: `TypeConstruction::enter()` (a guard held by `get_type_of_class`, `get_type_of_function`, `get_type_of_function_predecorated`, `get_type_of_lambda_with_expected_type` and `get_type_of_type_param`) keeps the memo from storing anything while a construction runs.
- `TypeEvaluator.print_memo`: 1,024 slots of `PrintMemoEntry` (type, flags, epoch, text).
- `types::combine_types` is Rust-only: it remembers `compute_combined_type` (the port of `combineTypes`, `types.ts:3850`) in `COMBINE_MEMO`, a thread-local collector root of 4,096 `CombineMemoEntry` slots (subtype list, options, epoch, result). `type_evaluator::type_construction_in_progress()` reads the `TypeConstruction` depth.
- `TypeEvaluator` has two Rust-only fields: `abstract_symbols_memo` (class details to `AbstractSymbolsMemoEntry`: the MRO `Rc` it was computed from and the symbols) and `stashed_speculative_stacks` (how many `disable_speculative_mode` calls hold a nonempty speculative stack). `AbstractSymbol` derives `Trace`.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `switch` module.
- `analyzer/types.rs`: `SynthesizeMethodsDeferred::call` counts an uncached evaluation.
- `analyzer/type_evaluator_types.rs`: `AbstractSymbol` derives `Trace`.
- `note_type_mutation` calls: `type_utils::map_subtypes` and `map_subtypes_expand_type_vars` (an alias source added to a union `combine_types` returned unchanged), `decorators::apply_function_decorator` (the overload bit set on an existing function), `data_classes.rs` (`update_named_tuple_base_class` replaced the base classes), `pattern_matching::get_sequence_pattern_info` (the tuple splices, which can edit a class's own tuple args), and in `type_evaluator.rs`: `_get_inferred_return_type_result`'s write, the recursive alias `bound_type` writes, `find_scoped_type_var`'s `declared_variance` copy, `initialize_prefetched_types`, `apply_loader_actions_to_module_type`.
- `analyzer/types.rs`: `combine_types` wraps `compute_combined_type`.
- `common/diagnostic.rs`: `AddendumMessage` and `add_message_format`. `localization/localize.rs`: `ParameterizedString` derives `Trace`.
- `docs/porting.md`: "switches" table and the `jsrt::switch_mode!` rule.

## Needs from others

## Doubts

- `print_type` memo: its correctness rests on the list of in-place writes above. It came from an audit of pyright 1.1.414 (every write to a field `typePrinter.ts` reads, directly or through `isTypeSame`, `combineTypes`, `isSameGenericClass` and the return type callback). Fields read only under print flags the CLI never sets (`PrintTypeVarVariance`, `ExpandTypedDictArgs`, `PythonSyntax`) were left out: variance inference's `computedVariance` and the lazy `typedDictEntries`. A new in-place write to a printed field needs a `note_type_mutation()` call, or the memo can return stale text; check mode finds it only where a corpus prints the changed object twice. Probes for the audited writes (a `match` on a `tuple[int, str, bytes]` subclass, `@overload` on a function an inner decorator returns, and three others) give pyright's output with the switch on, off and at check.
- `combine_types` memo: its correctness rests on the same audit, which covered the fields `combineTypes` reads (the printer reaches it through `removeFromUnion`). Its check mode compares the remembered result with a new combine on every reuse: the same object when the result was one of the inputs, otherwise a new union with the same subtypes in the same order, alias sources and flags.
- The abstract-symbols memo adds about 0.05 G instructions on samples (+0.5%) although samples calls `get_abstract_symbols` only 400 to 500 times: the cost is in mimalloc's slow paths. With the memo filled or not, samples prints the same 31,786 strings, makes the same `assign_type` calls and the same evaluator allocations. The 27cdb3a message says "about 440" calls; the count measured is 400 to 499.

## Attempts

`bench/measure.sh <corpus> 3`, `release`, on devbox; before is the parent commit.

| change | switch | pydantic-ai | samples |
|---|---|---|---|
| specialize an MRO class only when a member is found in it (15fb597) | `PYRIGHT_RS_DEFER_MRO_SPECIALIZATION` | 245.02 G → 236.25 G (−3.6%) | 9.31 G → 9.23 G (−0.9%) |
| remember `get_abstract_symbols` per class (27cdb3a) | `PYRIGHT_RS_ABSTRACT_SYMBOLS_MEMO` | 233.23 G → 229.50 G (−1.6%) | 9.20 G → 9.25 G (+0.5%, see Doubts) |
| format addendum messages when read | `PYRIGHT_RS_LAZY_ADDENDUM_FORMAT` | 227.72 G → 224.72 G (−1.3%) | 9.14 G → 9.09 G (−0.5%) |
| remember `print_type` text per type object and flags | `PYRIGHT_RS_PRINT_TYPE_MEMO` | 224.72 G → 216.92 G (−3.5%) | 9.09 G → 9.01 G (−0.9%) |
| remember reachability narrowing per conditional flow node | `PYRIGHT_RS_REACHABILITY_NARROWING_MEMO` | 216.71 G → 214.36 G (−1.1%) | 9.01 G → 8.99 G (−0.2%) |
| remember `combine_types` results for lists of 9 or more members | `PYRIGHT_RS_COMBINE_TYPES_MEMO` | 214.35 G → 214.13 G (−0.1%); peak RSS 5.78 → 5.81 GiB | 8.99 G → 7.66 G (−14.8%) |

Measured, not done yet:
- Member types specialized per access (`get_type_of_member_internal` → `partially_specialize_type`, 6.3% of pydantic-ai instructions in the profile after d16e837): 944,720 calls, 431,991 distinct (member type, MRO class, its type arguments, self class); 353,527 return the member type unchanged. pyright builds a new object on every call. A prototype memo keyed that way, which hands one object to callers that pyright gives distinct ones, measured 214.95 G → 213.09 G instructions (−0.9%) and 5.80 GiB → 5.99 GiB peak RSS on pydantic-ai, samples 9.01 G → 8.98 G. It changed samples' `callSite4.py` types dump: a shared function object shares its `callSiteReturnTypeCache`, so call-site return type inference ran differently. Leaving out functions with unannotated parameters (the only ones call-site inference touches) kept every corpus identical and the same instruction saving. Not landed: the RSS cost is above 50 MiB, and sharing objects pyright keeps distinct needs an audit of every in-place write to a member type and every identity use of one.
- `is_type_same`, top-level calls (prototype, 4,096 slots keyed by both types and options, epoch-guarded): on pydantic-ai 215.52 G → 215.51 G. On samples it costs more than it saves: 9.25 G → 9.68 G for every category, 9.25 G → 9.32 G for unions, functions and overloads only. Repeated pairs are not where its 5.3% share in the profile after c58203a goes.
- `ClassType::is_derived_from` (prototype, keyed by both classes, epoch-guarded, also remembering the inheritance chain it appends, because `assign_class`, its hottest caller, asks for the chain): pydantic-ai 214.89 G → 214.82 G (−0.03%), samples 9.01 G → 8.94 G. Without the chain the memo never served `assign_class` and saved nothing. Samples makes 440,721 top-level calls on 59,520 distinct class pairs.
- `combine_types` for every list of 2 or more subtypes (prototype of the landed memo): pydantic-ai 215.00 G → 214.90 G and 5.79 → 5.84 GiB, samples 9.02 G → 7.61 G. Samples makes 204,404 calls on 63,652 distinct input lists. In samples, hits on lists of 9 to 32 members held 38 ms of the 40 ms of combining the hits saved; the landed memo keeps lists of 9 or more members, so smaller lists take no slots.

Profile after c58203a, items above 0.5% of pydantic-ai instructions that no memo above covers:
- `clone_type` (4.1%): pyright's own object creation, from `ClassType::specialize`, `clone_with_literal`, `clone_as_instance` and `clone_as_instantiable`. Each call hands out a new object pyright keeps distinct; sharing one is the identity question the member-type memo raised.
- `look_up_class_member` (3.3%, 2.9% under `get_type_of_class_member_name`): its result holds the MRO class specialized for that call (lazily since 15fb597), so a memo would share objects pyright builds per call, as the member-type memo did.
- `is_same_generic_class` (2.5%): called from `is_type_same` and `is_derived_from`, both measured above.
- `get_cache_entry` (1.4%): pyright's own type cache reads.
- `DiagnosticAddendum` (1.0%): pyright's own addendum objects; their messages already format only when read (`PYRIGHT_RS_LAZY_ADDENDUM_FORMAT`).
- reachability walks (2.8%): the narrowing memo covers the conditions; the rest are walks whose visited sets depend on the path.
- type narrowing callbacks (1.9% under `get_type_narrowing_callback`, 0.7% of it `narrow_type_for_instance_or_subclass`): pydantic-ai makes about 23,000 isinstance narrowings on 17,105 distinct (type, filter types, flags), so repeats hold at most about a quarter of that 0.7%.
