# frontend notes

Tokenizer and parser: milestone 1, first half (`PLAN.md` step 4).

## parse node API

`crates/pyright/src/parser/parse_nodes.rs`, from `parser/parseNodes.ts`.

- `ParseNode` struct: `node_type: ParseNodeType`, `start`/`length: Cell<i32>`, `id: Cell<u32>`, `parent: Cell<Option<P<ParseNode>>>`, `a: ParseTreeKey`, `d: ParseNodeDetails`. A node handle is `P<ParseNode>`; `==` is identity.
- `ParseNodeDetails`: one variant per `ParseNodeType` member, same name, holding `<Name>NodeDetails`. Match on `node.d` or use a typed handle.
- Typed handle per node type, for example `FunctionNode(pub P<ParseNode>)`: `FunctionNode::cast(p) -> Option<FunctionNode>` (checks `node_type`), `.d() -> &'static FunctionNodeDetails`, `.node()`/`.0`/`P::from`, `Deref<Target = ParseNode>`, `==` against `P<ParseNode>` in both directions, and the TypeScript `create` functions as associated functions (`ClassNode::create_dummy_for_decorators`).
- `d` fields, snake_case of the TypeScript names:
  - a field typed as one node type holds that handle (`FunctionNodeDetails.name: Cell<NameNode>`); a union (`ExpressionNode`, `SuiteNode | IfNode`, `PatternAtomNode`, ...) holds `P<ParseNode>`;
  - an optional or `| undefined` field is `Cell<Option<_>>`, including optional booleans (`ForNodeDetails.is_async: Cell<Option<bool>>`): the oracle prints a set `false` and omits `undefined`;
  - arrays are `RefCell<Vec<_>>`; copy values out before calling anything that can reach the same node;
  - non-Copy values that TypeScript never writes after creation are plain fields: `NameNodeDetails.value: String`, `StringNodeDetails.value: JsString`, `FormatStringNodeDetails.value: JsString` (empty), `NumberNodeDetails.value: NumberValue`, `ErrorNodeDetails.decorators: Option<Vec<DecoratorNode>>`.
- Union aliases: `ExpressionNode`, `StatementNode`, `SmallStatementNode`, `ComprehensionForIfNode`, `DictionaryEntryNode`, `PatternAtomNode`, `PatternMappingEntryNode`, `EvaluationScopeNode`, `ExecutionScopeNode`, `TypeParameterScopeNode` are all `P<ParseNode>`. `ParseNodeArray = Vec<Option<P<ParseNode>>>`.
- `ParseTreeKey = P<ParseTreeKeyObject>`, compared by identity. A file parse's key is created by `ModuleNode::create`; `get_parse_tree_root(node)` returns that module. `create_parse_tree_key()` is the `{}` of a detached expression parse. The parser's string annotations live on the key: `get_parser_string_annotation(node)`, `get_parser_string_annotation_info(node)`, `set_parser_string_annotation_info(key, info)`.
- `get_next_node_id()` is a thread-local counter starting at 1, as `_nextNodeId`.
- `extend_range(node: &ParseNode, range: &impl HasTextRange)`; `is_expression_node(&ParseNode)`.
- Enums keep TypeScript names and values: `ParseNodeType`, `ErrorExpressionCategory`, `ParamCategory`, `TypeParamKind`, `ArgCategory`.

- Every `<Name>NodeDetails` and `ParseNodeDetails` derive `Clone`, for the parser's copy of an augmented assignment's `destExpr` (see doubts).

## parser API

`crates/pyright/src/parser/parser.rs`, from `parser/parser.ts`.

- `ParseOptions::new()` has the TypeScript defaults (`python_version: LATEST_STABLE_PYTHON_VERSION`, every flag false); set fields directly.
- `Parser::new().parse_source_file(file_contents: JsString, parse_options, diag_sink: P<DiagnosticSink>, content_hash: Option<StringFingerprint>) -> ParseFileResults`. `None` computes `get_string_fingerprint(text)`, as the TypeScript does.
- `parse_text_expression(file_contents, text_offset, text_length, parse_options, parse_text_mode, initial_paren_depth: i32, typing_symbol_aliases: Option<&JsMap<String, String>>, owner_key: Option<ParseTreeKey>) -> ParseExpressionTextResults`: the four TypeScript overloads in one; pass `0` for the omitted `initialParenDepth`. `parse_tree` is a `FunctionAnnotationNode` for `ParseTextMode::FunctionAnnotation`.
- `ParseFileResults { text, content_hash, parser_output, tokenizer_output: P<TokenizerOutput> }`; `ParserOutput.lines` and `ParseExpressionTextResults.lines` are `&'static TextRangeCollection<TextRange>` borrowed from the arena tokenizer output.
- `ParserOutput.future_imports: JsSet<String>`, `typing_symbol_aliases: JsMap<String, String>`, `imported_modules: Vec<ModuleImport>` with `imported_symbols: Option<JsSet<String>>`.
- The parser recurses once per nesting level, as the TypeScript does; run it on a thread with a large stack (`examples/oracle_dump.rs` uses 1 GiB).

Tokens (`parser/tokenizer_types.rs`) follow the same pattern: `Token { type_, start, length, comments: Option<&'static [Comment]>, d: TokenDetails }` in the arena as `P<Token>`, typed handles `KeywordToken`, `StringToken`, `IdentifierToken`, ... with `cast`, `d()`, `Deref<Target = Token>`. `TokenDetails::None` is a plain `Token`.

`common/text_range.rs`: trait `HasTextRange` (`text_range() -> TextRange`) stands in for TypeScript's structural `TextRange`; it is implemented for `TextRange`, `Token`, `Comment`, `ParseNode`, every typed handle, `P<T>` and `&T`. `TextRange::extend`, `contains`, ... take `&impl HasTextRange`.

## Signature changes

- `Char` (`common/char_codes.rs`) is a module of `u16` constants, not an enum, so `ch == Char::Hash` compares with a char code as the TypeScript does.
- `Tokenizer<'a>` borrows the source text: `tokenize(&mut self, text: &'a [u16], start: Option<i32>, length: Option<i32>, initial_paren_depth: i32, use_notebook_mode: bool)`.
- `Tokenizer::get_operator_info` returns `Option<OperatorFlags>`; `None` is the `undefined` that `Walrus` gets.
- `getIndexContaining` takes `in_range: Option<&dyn Fn(&T, i32) -> bool>`; `None` is the `TextRange.contains` default.
- `combineRange(ranges)` takes `&mut [Range]` because it mutates `ranges[0]`, as the TypeScript does.
- `get_line_end_position`/`get_line_end_offset` take the text as `&[u16]`.
- `get_unescaped_string(token, elide_crlf: bool)`: pass `true` for the TypeScript default.
- `get_child_nodes(node, get_string_annotation: Option<&dyn Fn(StringListNode) -> Option<ExpressionNode>>)`: `None` is the `getParserStringAnnotation` default.

## Shared-file edits

- `crates/pyright/src/jsrt_pending.rs` held number stand-ins until jsrt numbers reached main (8a4b41e); removed, the tokenizer and parse nodes now use `jsrt::{NumberValue, JsBigInt}` and `jsrt::number::{parse_int, parse_float, parse_bigint, math_round}`.
- `crates/pyright/src/oracle/format.rs` (new): the FORMAT.md value printer (string escaping, numbers, sorted fields) shared by the tokens and tree dumpers.
- `crates/pyright/examples/oracle_dump.rs` (new): `cargo run --release --example oracle_dump -- [--python-version <x.y>] [--stage tokens|tree]... <reference dump dir> <out dir>` writes Rust dumps for every file of a reference dump, for `oracle/compare.py`. `--python-version` is the corpus's configured version: 3.11 for pydantic-ai, 3.13 (the default) for the others.
- `tools/unicode_tables.mjs` (new): generates `parser/unicode.rs` from the evaluated `unicode.ts`; the test `parser::unicode::tests::tables_match_typescript` reruns it and compares every table.

## Needs from others

None.

## Doubts

- An augmented assignment's `destExpr` is `Object.assign({}, leftExpr)` with a new id: its `d` is the same object as `leftExpr.d`. The Rust copy clones `d`, so it holds the same child handles, but a later write to one node's `d` would not reach the other. Nothing writes to either after the copy: the parser does not, and `analyzer/` has no `.d.<field> =` assignment.

- `is_python_keyword("and")` returns false: TypeScript tests `!keyword`, and `KeywordType.And` is 0. Ported as is.
- `IdentifierToken` values and comment values are `String`. The only text that can hold a lone surrogate is the dummy file the evaluator builds from an unescaped string annotation; an identifier never holds one (the tables reject it), a comment there would print U+FFFD. String-token `escaped_value` is a `JsString` and keeps it.
