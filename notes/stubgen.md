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

- Results (`TypeResult`, `EffectiveTypeResult`, `InferenceContext`, ...) are handles, as the default rule makes them: the evaluator writes cached results after storing them, and a copy changed pyright's output in a reproduction. Each evaluation allocates handles that the arena never frees; the memory cost is unmeasured.
- A JS collection is a shared reference; the stubs make it an owned value. The binder's code-flow expression set and the evaluator's type caches are mapped to shared handles. Other collections stored in two places and changed afterwards need the same `[sites]` entry.
- Value structs that inherit from another value struct are flattened; two evaluator call sites pass a derived struct where the base is expected (docs/evaluator-design.md, section 6).
- Number sites in the stubbed files are reviewed; sites ending in `?` belong to files only placeholders cover, and their owners should review them when those files are stubbed or ported.
