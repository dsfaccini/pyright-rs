# sound notes

Audit the unsafe code for undefined behavior with Miri, under Stacked Borrows and Tree Borrows, and fix what it finds. Rules: `docs/porting.md`, "unsafe code". Rerun: `tools/miri.sh`.

## Signature changes

- `jsrt::P<T>` holds a `NonNull<T>` with the whole object's provenance, not a `&'static T`. `P::get` and `Deref` are unchanged. New: `P::as_ptr`. `P::to_dyn` takes `fn(*const T) -> *const U`; `|x| x` still compiles.
- `jsrt::Header`: a header type implements it, and `P<T>::d()` returns its details. `ParseNode`, `Token`, `FlowNode` and `Declaration` have no `d(&self)` any more: call `d()` on the handle.
- Header methods that read details take the handle: `Token::comments(token)`, `Token::shared_comments(token)`, `ParseNode::shallow_copy(node)`. `Declaration::variable()` and its kin moved to `DeclarationDetails` (`decl.d().variable()`); `FlowNode::antecedent()` moved to `FlowNodeDetails` (`node.d().antecedent()`). `ModuleLoaderActionsLike` is implemented for `P<Declaration>`, not `Declaration`.
- Functions that read details take handles: `get_unescaped_string(P<Token>, ..)`, `has_type_for_declaration(P<Declaration>)`, `are_declarations_same(P<Declaration>, P<Declaration>, ..)`, and the oracle dumpers `token_line`, `write_token`, `declaration_value`, `flow_node_value`, `flow_references`, `tree::fields`.
- `Token` and `FlowNode` have no `_written_through_shared` field.
- `jsrt::heap::prefetch` takes a pointer. `Region::for_each_used` passes chunk pointers.

## Shared-file edits

- `crates/jsrt/src/{vm.rs (new), heap.rs, heap/census.rs, region.rs, arena.rs, trace.rs, cell.rs, lib.rs}`.
- Header types: `parser/parse_nodes.rs`, `parser/tokenizer_types.rs`, `analyzer/code_flow_types.rs`, `analyzer/declaration.rs`.
- Call sites of the moved methods: `binder.rs`, `checker.rs`, `code_flow_engine.rs`, `code_flow_utils.rs`, `comment_utils.rs`, `data_classes.rs`, `declaration_utils.rs`, `analyzer/parse_tree_utils.rs`, `protocols.rs`, `scope.rs`, `source_file.rs`, `symbol.rs`, `symbol_utils.rs`, `type_evaluator.rs`, `type_guards.rs`, `typed_dicts.rs`, `parser.rs`, `string_token_utils.rs`, `oracle/{bind,flow,tokens,tree}.rs`, `tokenizer_tests.rs`, `tokenizer_unescape_tests.rs`.
- `parser/characters.rs`: the fast table and the full maps build separately, as in TypeScript.
- `main.rs`, `common/full_access_host.rs`: `cfg(miri)` stand-ins.
- `docs/porting.md`: "unsafe code"; the header-type bullet under "memory".

## Needs from others

## Doubts

- Miri cannot see a use of a swept evaluator object: the heap is one allocation to Miri, and the sweep only drops and poisons slots inside it. Check mode (`PYRIGHT_RS_GC=check`) stays the guard for that.
