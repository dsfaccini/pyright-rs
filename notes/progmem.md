# progmem notes

Shrink the program heap: parse trees, tokens and binder objects, which are never collected.

## Signature changes

- `ParseNode` is the shared header; a node is allocated as `NodeAlloc<D>`, the header followed by the details of its type. `node.d()` returns `ParseNodeDetails`, an enum of `&'static <Name>NodeDetails`: replace `&node.d` and `&node.get().d` with `node.d()`. Typed handles keep `d()`.
- `ParseNode::shallow_copy` ports `Object.assign({}, node)` (`_parseStatement`'s augmented assignment). `create_placeholder_node` makes the id-0 node behind `undefined_declaration_node`.
- `jsrt::P::header` turns a `P` to a `#[repr(C)]` object into a `P` to its first field.
- `Token` is the `TokenBase` header; a token is allocated as `TokenAlloc<D>`, the header followed by its interface fields. `token.d()` returns `TokenDetails`, an enum of `&'static <Name>TokenDetails`; typed handles keep `d()`. `token.comments` is `token.comments()`; `Token::create_with_shared_comments` takes `token.shared_comments()`.
- `AnalyzerNodeInfo` keeps `flow_node` inline; the other fields live in `AnalyzerNodeInfoCold`. Read `info.cold().and_then(|c| c.scope.get())`, write `info.cold_mut().scope.set(..)`.

## Shared-file edits

- `crates/jsrt/src/arena.rs`: `P::header`.
- `token.comments()` and `token.d()` in `comment_utils.rs`, `analyzer/parse_tree_utils.rs`, `source_file.rs`, `oracle/tokens.rs`, `parser.rs`, `string_token_utils.rs`, `tokenizer_tests.rs`.
- `oracle/bind.rs`, `oracle/flow.rs`: read the cold node-info fields.
- `.d` to `.d()` at parse-node match sites in `binder.rs`, `checker.rs`, `code_flow_types.rs`, `declaration.rs`, `decorators.rs`, `enums.rs`, `named_tuples.rs`, `analyzer/parse_tree_utils.rs`, `sentinel.rs`, `static_expressions.rs`, `typed_dicts.rs`, `oracle/tree.rs`, `parser/parse_tree_utils.rs`, `parser.rs`.

## Needs from others

## Doubts
