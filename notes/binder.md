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
- `evaluateStaticBoolExpression` / `evaluateStaticBoolLikeExpression` take the alias lists as `Option<&[String]>`.
- parseTreeUtils:
  - `get_full_statement_range(statement_node, &ParseFileResults, include_trailing_blank_lines: Option<bool>)`: the `options?` object is `Option<bool>`.
  - `is_blank_line(&TokenizerOutput, text: &[u16], line)`: `ParseFileResults.text` is UTF-16.
  - `get_parent_node_of_type` and `get_first_ancestor_or_self_of_kind` return `Option<P<ParseNode>>`; the caller casts. `get_module_node -> Option<ModuleNode>`.
  - `print_operator` and `print_parse_node_type` return `&'static str`. `get_doc_string -> Option<JsString>`.
  - `get_node_depth -> u32`, `get_type_source_id -> i32`.
  - `get_call_node_and_active_param_index -> Option<CallNodeAndActiveParamIndex>`: a named struct for the anonymous TS return type, so it has no marker.
  - Callbacks are `&dyn Fn`; `get_next_matching_token`'s `exit: None` is TS's default `() => false`.
  - Token functions take `&TextRangeCollection<P<Token>>` and return `Option<P<Token>>`. `get_comments_at_token_index -> Option<&'static [Comment]>`.
  - `NameNodeWalker` and `CallNodeWalker` are generic over the callback (`FnMut`).
  - `get_ancestors_including_self` returns a lazy `impl Iterator` for the generator.
  - `EvaluationScopeInfo` keeps `use_proxy_scope` and `use_chained_module_level_scopes` as `Option<bool>`.
  - `PrintExpressionFlags` is a `u32` newtype. TS default parameters are explicit arguments.
- scopeUtils: `get_scope_hierarchy(node, stop_scope: Option<P<Scope>>, node_info) -> Option<Vec<P<Scope>>>`.

## Shared-file edits

- `crates/pyright/src/analyzer/analyzer_file_info.rs`: adds `AnalyzerFileInfo.builtins_scope: Option<P<Scope>>` and `ImportLookupResult.symbol_table: SymbolTable`.
- `crates/pyright/src/analyzer/source_file.rs`: `build_file_info` sets `builtins_scope: None` (STAND-IN). Infra passes `bind`'s `builtinsScope` when it wires `run_binder`.
- `crates/pyright/src/analyzer/program.rs`: the `skipParsing` dummy `ImportLookupResult` gets `symbol_table: SymbolTable::new()`, as pyright's `new Map()`.
- `crates/pyright/src/analyzer/import_statement_utils.rs`: adds `getWildcardImportNames` next to infra's `getDirectoryLeadingDotsPointsTo`.
- `crates/pyright/src/analyzer/mod.rs`: `cell_chain_index`, `declaration_utils`, `types`.
- `crates/pyright/src/analyzer/cell_chain_index.rs` (new): only the `CellChainIndexProvider` trait the binder takes.
- `crates/pyright/src/analyzer/declaration_utils.rs` (new): only `hasTypeForDeclaration` and `areDeclarationsSame`, which `symbol.ts` calls. The evaluator step ports the rest.
- `crates/pyright/src/analyzer/types.rs` (new): placeholder `Type` (for `SynthesizedTypeInfo`) and `TypeVarScopeId`. The evaluator step replaces it.

## Oracle harness

`crates/pyright/examples/oracle_bind.rs` binds the files of a reference dump and writes bind and flow dumps for `oracle/compare.py`, until infra's `Program` runs the binder:

```
cargo run --release --example oracle_bind -- [--path PREFIX]... [--python-version 3.11] [--mode strict] [--rule name=level]... <reference dump dir> <out dir>
python3 oracle/compare.py <reference dump dir> <out dir> --stage bind --stage flow
```

- It replays each file's `importInfo` and `isInPyTypedPackage` from the reference dump; import lookups bind their target on demand, builtins first, as `Program._bindFile` does.
- It compares bind diagnostics with the `bind:` list of the reference diag dump and prints the count of files that differ.
- pydantic-ai needs `--python-version 3.11 --mode strict --rule reportMissingTypeStubs=false --rule reportUnnecessaryIsInstance=false --rule reportMissingModuleSource=false` (its `[tool.pyright]`). The other corpora use the defaults.

## Needs from others

- infra: wire `run_binder` in `SourceFile.bind` with the entry point above, pass `builtinsScope`, and fill `ImportLookupResult.symbol_table` in `Program._lookUpImport`. Then the oracle runs through `Program` and the harness can go.

## Doubts

- Code flow reference keys are `String`. A key built from a string subscript with a lone surrogate (`a["\ud800"]`) prints U+FFFD where pyright prints `\ud800`.
- `SymbolTable::for_each` walks by index, like a JS `Map` for entries added during the walk; an entry deleted before the cursor shifts the rest one place (JS does not skip). No binder path deletes during a walk.
- `print_expression` returns `String`: a string literal cut at 32 UTF-16 units that splits a surrogate pair prints U+FFFD where pyright keeps the lone surrogate. Same for f-string middle tokens.
- `PythonVersion` parts are `i32`, so `sys.version_info >= (3.5, 0)` truncates `3.5` to `3` where pyright compares the float.
- `get_file_info_from_node` calls `get_file_info_if_available`: TS's `getFileInfo(...)!` returns undefined at runtime where the Rust `get_file_info` panics.
- Places where JS throws a TypeError (`parent!` missing in `get_execution_scope_node`, `get_built_in_scope` with no builtins scope) panic in Rust; only the message differs.
- Skipped test: `parseTreeUtils.test.ts` "printExpression forward declarations use parser annotations but not owner annotations" needs `program.analyze()` and the evaluator.
