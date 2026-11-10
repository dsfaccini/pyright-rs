# stubgen notes

## Signature changes

None to ported code: the generator writes only to `tools/stubgen/out/`.

## Shared-file edits

None. New files only: `tools/stubgen/`, `docs/evaluator-design.md`, this file.

## Needs from others

- jsrt owner: `js_flags!` (bit operators for a flags newtype `pub struct X(pub u32)`), `JsBigInt`, `NumberValue` (porting.md "numbers"), `JsRegExp`. The compile check shims them from `tools/stubgen/jsrt_shim.rs`; the real ones may differ in body, not in name or shape.
- frontend owner: the stubs name parse nodes and tokens by the porting.md pattern: `P<ParseNode>`, typed handles `FunctionNode(P<ParseNode>)` with `cast` and `d()`, `ParseNodeDetails::<Kind>`, `<Name>NodeDetails`, aliases such as `pub type ExpressionNode = P<ParseNode>`, and `Token` the same way with variant `Plain` for tokens created as `Token`. `ParseTreeKey` maps to `P<ParseNode>`, the module node. The check compiles the stubs against `parser/parse_nodes.rs` as soon as it holds code.
- common/uri owner: `Uri` is one opaque value type at `crate::common::uri::uri::Uri`; `uriInterface.ts`'s `Uri` is merged into it.

## Doubts

- `TypeResult`, `CallResult`, `InferenceContext` and other result and option records are values, not handles. That is faithful only while no code writes one after storing it somewhere else. Body porters must check each write that follows a store, and switch the type to `handle` in `typemap.toml` if one does.
- A JS collection is a shared reference; the stubs make it an owned value. Only the binder's code-flow expression set is mapped to a shared handle so far. Other collections stored in two places and changed afterwards need the same `[sites]` entry.
- Number sites in the stubbed files are reviewed; sites ending in `?` belong to files only placeholders cover, and their owners should review them when those files are stubbed or ported.
