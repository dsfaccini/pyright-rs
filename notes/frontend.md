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

Tokens (`parser/tokenizer_types.rs`) follow the same pattern: `Token { type_, start, length, comments: Option<&'static [Comment]>, d: TokenDetails }` in the arena as `P<Token>`, typed handles `KeywordToken`, `StringToken`, `IdentifierToken`, ... with `cast`, `d()`, `Deref<Target = Token>`. `TokenDetails::None` is a plain `Token`.

`common/text_range.rs`: trait `HasTextRange` (`text_range() -> TextRange`) stands in for TypeScript's structural `TextRange`; it is implemented for `TextRange`, `Token`, `Comment`, `ParseNode`, every typed handle, `P<T>` and `&T`. `TextRange::extend`, `contains`, ... take `&impl HasTextRange`.

## Signature changes

- `Char` (`common/char_codes.rs`) is a module of `u16` constants, not an enum, so `ch == Char::Hash` compares with a char code as the TypeScript does.
- `Tokenizer<'a>` borrows the source text: `tokenize(&mut self, text: &'a [u16], start: Option<i32>, length: Option<i32>, initial_paren_depth: i32, use_notebook_mode: bool)`.
- `Tokenizer::get_operator_info` returns `Option<OperatorFlags>`; `None` is the `undefined` that `Walrus` gets.
- `getIndexContaining` takes `in_range: Option<&dyn Fn(&T, i32) -> bool>`; `None` is the `TextRange.contains` default.
- `combineRange(ranges)` takes `&mut [Range]` because it mutates `ranges[0]`, as the TypeScript does.
- `get_line_end_position`/`get_line_end_offset` take the text as `&[u16]`.

## Shared-file edits

- `crates/pyright/src/lib.rs`: `pub mod jsrt_pending;`.
- `crates/pyright/src/jsrt_pending.rs` (new, temporary): `NumberValue`, `JsBigInt` and `jsrt::number::{parse_int, parse_float, parse_bigint}` stand-ins with the names jsrt announced. Delete when jsrt numbers land on main.
- `tools/unicode_tables.mjs` (new): generates `parser/unicode.rs` from the evaluated `unicode.ts`; the test `parser::unicode::tests::tables_match_typescript` reruns it and compares every table.

## Needs from others

- jsrt: `NumberValue`, `JsBigInt` (`PartialOrd<f64>`), `number::{parse_int, parse_float, parse_bigint, number_to_string}`. Announced; the frontend uses the stand-ins until then.

## Doubts

- `is_python_keyword("and")` returns false: TypeScript tests `!keyword`, and `KeywordType.And` is 0. Ported as is.
- `IdentifierToken` values and comment values are `String`. The only text that can hold a lone surrogate is the dummy file the evaluator builds from an unescaped string annotation; an identifier never holds one (the tables reject it), a comment there would print U+FFFD. String-token `escaped_value` is a `JsString` and keeps it.
