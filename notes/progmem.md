# progmem notes

Shrink the program heap: parse trees, tokens and binder objects, which are never collected.

## Signature changes

- `ParseNode` is the shared header; a node is allocated as `NodeAlloc<D>`, the header followed by the details of its type. `node.d()` returns `ParseNodeDetails`, an enum of `&'static <Name>NodeDetails`: replace `&node.d` and `&node.get().d` with `node.d()`. Typed handles keep `d()`.
- `ParseNode::shallow_copy` ports `Object.assign({}, node)` (`_parseStatement`'s augmented assignment). `create_placeholder_node` makes the id-0 node behind `undefined_declaration_node`.
- `jsrt::P::header` turns a `P` to a `#[repr(C)]` object into a `P` to its first field.
- `Token` is the `TokenBase` header; a token is allocated as `TokenAlloc<D>`, the header followed by its interface fields. `token.d()` returns `TokenDetails`, an enum of `&'static <Name>TokenDetails`; typed handles keep `d()`. `token.comments` is `token.comments()`; `Token::create_with_shared_comments` takes `token.shared_comments()`.
- `NameNode.value` is `value()`, read from the token: TypeScript sets it to `token.value` and never writes either. It returns `&'static str`; take `.to_string()` where a `String` is needed.
- `TokenizerOutput` is owned, no longer a `P`: the parser owns it while parsing, `ParseFileResults` and then `SourceFile.parse` (an `Rc`, cached only for an open file) drop it. Its `lines` are a `P`, kept by the source file. Its `scratch` holds the tokens; dropping it drops those no node kept.
- Parse-node token fields are `TokenCell<T>` (`get`, `set`, `new`), which keep the token (`Token::keep`). `FormatStringNode.middle_tokens` keeps its tokens at creation.
- `jsrt::region::Region`: bump allocation into mmap chunks, released whole to a per-thread pool, or protected under check mode. `jsrt::P::from_raw` makes a handle into it.
- `FlowNode` is the header; a flow node is allocated as `FlowAlloc<D>`. `FlowNode::create(flags, id, details)` takes the details struct (`FlowPlain` for a bare node); `node.d()` returns `FlowNodeDetails`, an enum of `&'static Flow<Name>Details`.
- `AnalyzerNodeInfo` keeps `flow_node` inline; the other fields live in `AnalyzerNodeInfoCold`. Read `info.cold().and_then(|c| c.scope.get())`, write `info.cold_mut().scope.set(..)`.

## Shared-file edits

- `crates/jsrt/src/arena.rs`: `P::header`.
- `token.comments()` and `token.d()` in `comment_utils.rs`, `analyzer/parse_tree_utils.rs`, `source_file.rs`, `oracle/tokens.rs`, `parser.rs`, `string_token_utils.rs`, `tokenizer_tests.rs`.
- `NameNode` `.value` to `.value()` across the analyzer, parser and oracle.
- `crates/jsrt/src/region.rs` (new), `crates/jsrt/src/lib.rs`, `crates/jsrt/src/arena.rs` (`P::from_raw`).
- `source_file.rs`, `parser.rs`: own the tokenizer output.
- `binder.rs`, `code_flow_utils.rs` (tests), `oracle/flow.rs`: flow node creation and `d()`.
- `oracle/bind.rs`, `oracle/flow.rs`: read the cold node-info fields.
- `.d` to `.d()` at parse-node match sites in `binder.rs`, `checker.rs`, `code_flow_types.rs`, `declaration.rs`, `decorators.rs`, `enums.rs`, `named_tuples.rs`, `analyzer/parse_tree_utils.rs`, `sentinel.rs`, `static_expressions.rs`, `typed_dicts.rs`, `oracle/tree.rs`, `parser/parse_tree_utils.rs`, `parser.rs`.

## Needs from others

## Doubts
