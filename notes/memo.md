# memo notes

Cut evaluator work that pyright repeats with the same result, with caches and shortcuts pyright lacks (`AGENTS.md`, "gates").

## Signature changes

- `jsrt::switch`: `SwitchMode` (`Off`, `On`, `Check`) and `jsrt::switch_mode!("PYRIGHT_RS_...")`, which reads a switch once per process.
- `type_utils::get_class_iterator` takes `specialize: bool` and yields `(mro_class, Option<specialized>)`; `None` when `specialize` is false.
- `type_evaluator::uncached_evaluation_count()` and `note_uncached_evaluation()`: a per-thread count of evaluations whose results can change later. `get_type_of_expression` counts every call that answers from no complete main type cache entry (a miss, an incomplete entry, a speculative or TypeForm entry), and `SynthesizeMethodsDeferred::call` counts each synthesis. A computation that leaves the count unchanged read only entries that stay.
- `DiagnosticAddendum::add_message_format(message, params)`: `add_message(message.format(params))` with the formatting deferred to the first read (`get_messages`, `get_string`). The hot `assign_type`, `assign_class`, `assign_type_args`, `assign_to_union_type`, `assign_from_union_type` and `get_type_of_class_member_name` sites use it. `ParameterizedString` derives `Trace`.
- `type_evaluator::note_type_mutation()` and `type_mutation_epoch()`: a per-thread epoch that ends every `print_type` memo entry. Call `note_type_mutation()` before any write that changes, in place, a field the printer reads on a type object that may already be printed. Writes to an object during its own construction need none: `TypeConstruction::enter()` (a guard held by `get_type_of_class`, `get_type_of_function`, `get_type_of_function_predecorated`, `get_type_of_lambda_with_expected_type` and `get_type_of_type_param`) keeps the memo from storing anything while a construction runs.
- `TypeEvaluator.print_memo`: 1,024 slots of `PrintMemoEntry` (type, flags, epoch, text).
- `TypeEvaluator` has two Rust-only fields: `abstract_symbols_memo` (class details to `AbstractSymbolsMemoEntry`: the MRO `Rc` it was computed from and the symbols) and `stashed_speculative_stacks` (how many `disable_speculative_mode` calls hold a nonempty speculative stack). `AbstractSymbol` derives `Trace`.

## Shared-file edits

- `crates/jsrt/src/lib.rs`: `switch` module.
- `analyzer/types.rs`: `SynthesizeMethodsDeferred::call` counts an uncached evaluation.
- `analyzer/type_evaluator_types.rs`: `AbstractSymbol` derives `Trace`.
- `note_type_mutation` calls: `type_utils::map_subtypes` and `map_subtypes_expand_type_vars` (an alias source added to a union `combine_types` returned unchanged), `decorators::apply_function_decorator` (the overload bit set on an existing function), `data_classes.rs` (`update_named_tuple_base_class` replaced the base classes), `pattern_matching::get_sequence_pattern_info` (the tuple splices, which can edit a class's own tuple args), and in `type_evaluator.rs`: `_get_inferred_return_type_result`'s write, the recursive alias `bound_type` writes, `find_scoped_type_var`'s `declared_variance` copy, `initialize_prefetched_types`, `apply_loader_actions_to_module_type`.
- `common/diagnostic.rs`: `AddendumMessage` and `add_message_format`. `localization/localize.rs`: `ParameterizedString` derives `Trace`.
- `docs/porting.md`: "switches" table and the `jsrt::switch_mode!` rule.

## Needs from others

## Doubts

- `print_type` memo: its correctness rests on the list of in-place writes above. It came from an audit of pyright 1.1.414 (every write to a field `typePrinter.ts` reads, directly or through `isTypeSame`, `combineTypes`, `isSameGenericClass` and the return type callback). Fields read only under print flags the CLI never sets (`PrintTypeVarVariance`, `ExpandTypedDictArgs`, `PythonSyntax`) were left out: variance inference's `computedVariance` and the lazy `typedDictEntries`. A new in-place write to a printed field needs a `note_type_mutation()` call, or the memo can return stale text; check mode finds it only where a corpus prints the changed object twice. Probes for the audited writes (a `match` on a `tuple[int, str, bytes]` subclass, `@overload` on a function an inner decorator returns, and three others) give pyright's output with the switch on, off and at check.
- The abstract-symbols memo adds about 0.05 G instructions on samples (+0.5%) although samples calls `get_abstract_symbols` only 400 to 500 times: the cost is in mimalloc's slow paths. With the memo filled or not, samples prints the same 31,786 strings, makes the same `assign_type` calls and the same evaluator allocations. The 27cdb3a message says "about 440" calls; the count measured is 400 to 499.

## Attempts

`bench/measure.sh <corpus> 3`, `release`, on devbox; before is the parent commit.

| change | switch | pydantic-ai | samples |
|---|---|---|---|
| specialize an MRO class only when a member is found in it (15fb597) | `PYRIGHT_RS_DEFER_MRO_SPECIALIZATION` | 245.02 G → 236.25 G (−3.6%) | 9.31 G → 9.23 G (−0.9%) |
| remember `get_abstract_symbols` per class (27cdb3a) | `PYRIGHT_RS_ABSTRACT_SYMBOLS_MEMO` | 233.23 G → 229.50 G (−1.6%) | 9.20 G → 9.25 G (+0.5%, see Doubts) |
| format addendum messages when read | `PYRIGHT_RS_LAZY_ADDENDUM_FORMAT` | 227.72 G → 224.72 G (−1.3%) | 9.14 G → 9.09 G (−0.5%) |
| remember `print_type` text per type object and flags | `PYRIGHT_RS_PRINT_TYPE_MEMO` | 224.72 G → 216.92 G (−3.5%) | 9.09 G → 9.01 G (−0.9%) |

Measured, not done yet:
- Member types specialized per access (`get_type_of_member_internal` → `partially_specialize_type`, 6.1% of pydantic-ai instructions in the profile after 27cdb3a): 944,720 calls, 431,991 distinct (member type, MRO class, its type arguments, self class); 353,527 return the member type unchanged. pyright builds a new object on every call, so a memo would hand one object to callers that pyright gives distinct ones; any identity comparison on the result (`===`, identity-keyed maps) would need an audit first.
- Narrowing in reachability walks (`get_flow_node_reachability`, conditional flow nodes; about 2.4% of pydantic-ai instructions outside first-time evaluation): 81,487 narrowings of 23,933 distinct flow nodes, 6,379 in speculative contexts. pyright caches a walk's result only under the node it started from, so walks from different statements narrow the same conditions again. A memo per flow node needs a check that the narrowing wrote nothing (it reaches symbol inference, the effective type cache and variance inference, which the uncached-evaluation count does not see) and that no symbol resolution in progress made the result circular.
