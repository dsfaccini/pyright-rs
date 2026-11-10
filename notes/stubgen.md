# stubgen notes

## Signature changes

None to ported code: the generator writes only to `tools/stubgen/out/`.

## Shared-file edits

None. New files only: `tools/stubgen/`, `docs/evaluator-design.md`, this file.

## Needs from others

- jsrt owner: `flag_ops!`, exported from jsrt. The frontend defines it locally in `parser/tokenizer_types.rs`; porting.md puts Rust-only helpers in jsrt, and every generated flags enum calls `jsrt::flag_ops!`. The compile check shims it from `tools/stubgen/jsrt_shim.rs` with the frontend's body.
- frontend owner: none. The stubs compile against the ported `parse_nodes.rs` and `tokenizer_types.rs`. They name `ParseTreeKey` as `P<ParseNode>`, the module node.
- common/uri owner: `Uri` is one opaque value type at `crate::common::uri::uri::Uri`; `uriInterface.ts`'s `Uri` is merged into it.

## Doubts

- `TypeResult`, `CallResult`, `InferenceContext` and other result and option records are values, not handles. That is faithful only while no code writes one after storing it somewhere else. Body porters must check each write that follows a store, and switch the type to `handle` in `typemap.toml` if one does.
- A JS collection is a shared reference; the stubs make it an owned value. Only the binder's code-flow expression set is mapped to a shared handle so far. Other collections stored in two places and changed afterwards need the same `[sites]` entry.
- Number sites in the stubbed files are reviewed; sites ending in `?` belong to files only placeholders cover, and their owners should review them when those files are stubbed or ported.
