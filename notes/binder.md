# binder notes

Binder group: milestone 1, second half (`PLAN.md` step 4).

## binder entry point (for infra's `SourceFile.bind`)

`analyzer/analyzer_node_info.rs` and `analyzer/binder.rs`:

```rust
let session = node_info_context.begin_write(parse_tree);            // P<AnalyzerNodeInfoBindingSessionImpl>
let node_info = create_analyzer_node_info_accessor(session.get(), Some(session.get()));
for record in import_info { node_info.set_import_info(record.node, record.import_result); }
node_info.set_file_info(parse_tree, file_info);                      // file_info: P<AnalyzerFileInfo>
let mut binder = Binder::new(file_info, config_options.index_generation_mode, None, node_info);
binder.bind_module(parse_tree);
let bind_diagnostics = file_info.diagnostic_sink.fetch_and_clear();
let module_scope = node_info.get_scope(parse_tree).unwrap();
node_info_context.publish(session);
```

- The context is `AnalyzerNodeInfoContextImpl::new() -> P<AnalyzerNodeInfoContextImpl>`; its trait is `AnalyzerNodeInfoContext`.
- The cell-chain index (notebooks) is always `None` in the CLI.

## API shapes

- Flow nodes: `FlowNode { flags: Cell<FlowFlags>, id: u32, d: FlowNodeDetails }` in the arena as `P<FlowNode>`. `d` names the interface the binder built (`Plain`, `Label`, `BranchLabel`, `Assignment`, ...). Typed handles `FlowLabel`, `FlowBranchLabel`, `FlowAssignment`, ... with `cast` and `d()`. `FlowLabel::d()` works for loop, branch and context-manager labels.
- Declarations: `Declaration { type_, node, uri, range, module_name, is_in_except_suite, is_in_inlined_typed_dict, d: DeclarationDetails }` as `P<Declaration>`; `decl.variable()`, `decl.alias()`, ... return the details. Optional TypeScript fields are `Cell<Option<_>>`: the oracle prints a field only when set.
- `ModuleLoaderActionsLike`: the structural overlap of `AliasDeclaration` and `ModuleLoaderActions` that the binder relies on.
- `SymbolTable` is a Copy handle to a shared `Map<string, Symbol>` (`get`, `set`, `has`, `delete`, `for_each`, `entries`).
- `Scope` methods take `&self`; `scope.handle()` is `this` as a `P<Scope>`.
- Shared `Set<string>` objects (code flow expressions, label `affectedExpressions`) are `P<RefCell<JsSet<String>>>`: the binder keeps adding to a set after storing it.
- `AnalyzerNodeInfoReader::get` takes `P<ParseNode>`. Call it on a `P<...>` holder as `(*holder).get(node)`: `P::get` shadows it.
- Walkers: `ParseTreeVisitor<T>` and `ParseTreeWalker` are traits; implement `default_visit_result` and override `visit_*`.

## Signature changes

- `AnalyzerNodeInfoReader`, `AnalyzerNodeInfoWriter`, `AnalyzerNodeInfoContext` are traits; readers and writers are `&'static dyn` (every store, session and context lives in the arena).
- `analyzerNodeInfo.ts` free functions take `node: impl Into<P<ParseNode>>`.
- `UnresolvedModuleMarker` is `unresolved_module_marker()`: one thread-local constant Uri (constant Uris compare by identity).
- `getInfoReader` (provider overloads) is not ported: Rust callers name the reader directly.

## Shared-file edits

- `crates/pyright/src/analyzer/analyzer_file_info.rs`: infra's WIP copied, plus `AnalyzerFileInfo.builtins_scope: Option<P<Scope>>` and `ImportLookupResult.symbol_table: SymbolTable`. Infra keeps those two fields and takes its own file otherwise.
- `crates/pyright/src/analyzer/source_file.rs`: SHIM `IPythonMode` enum. Take infra's version.
- `crates/pyright/src/parser/parse_tree_utils.rs`: frontend's `getChildNodes` (commit c73b668 on `frontend`), copied verbatim.
- `crates/pyright/src/analyzer/mod.rs`: `pub mod declaration_utils;`, `pub mod types;`.
- `crates/pyright/src/analyzer/declaration_utils.rs` (new): only `hasTypeForDeclaration` and `areDeclarationsSame`, which `symbol.ts` calls. The evaluator step ports the rest.
- `crates/pyright/src/analyzer/types.rs` (new): placeholder `Type` (for `SynthesizedTypeInfo`) and `TypeVarScopeId`. The evaluator step replaces it.

## Needs from others

- frontend: the parser (`parser.ts`, `ParseFileResults`) to run the oracle and the parse-tree unit tests; `parseNodeUtils.ts` maps for `printExpression`.
- infra: `analyzer_file_info.rs` and `source_file.rs` with the shapes above.

## Doubts

- Code flow reference keys are `String`. A key built from a string subscript with a lone surrogate (`a["\ud800"]`) prints U+FFFD where pyright prints `\ud800`.
- `SymbolTable::for_each` walks by index, like a JS `Map` for entries added during the walk; an entry deleted before the cursor shifts the rest one place (JS does not skip). No binder path deletes during a walk.
