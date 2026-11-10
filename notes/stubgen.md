# stubgen notes

## Signature changes

None to ported code: the generator writes only to `tools/stubgen/out/`.

## Shared-file edits

None. New files only: `tools/stubgen/`, `docs/evaluator-design.md`, this file.

## Needs from others

- frontend and binder owners: none. The evaluator stubs compile against the ported `parse_nodes.rs`, `tokenizer_types.rs`, `parser.rs` and binder group. `typemap.toml` records their shapes where they differ from porting.md's pattern: `Declaration` has no typed handles and uses unit variants for kinds without fields.
- declaration_utils owner: `ResolvedAliasInfo`, which the evaluator stubs name; the compile check serves it from the placeholder until the port has it.
- common/uri owner: `Uri` is one opaque value type at `crate::common::uri::uri::Uri`; `uriInterface.ts`'s `Uri` is merged into it.

## Doubts

- Results (`TypeResult`, `EffectiveTypeResult`, `InferenceContext`, ...) are handles, as the default rule makes them: the evaluator writes cached results after storing them, and a copy changed pyright's output in a reproduction. Each evaluation allocates handles that the arena never frees; the memory cost is unmeasured.
- A JS collection is a shared reference; the stubs make it an owned value. The binder's code-flow expression set and the evaluator's type caches are mapped to shared handles. Other collections stored in two places and changed afterwards need the same `[sites]` entry.
- Value structs that inherit from another value struct are flattened; two evaluator call sites pass a derived struct where the base is expected (docs/evaluator-design.md, section 6).
- Number sites in the stubbed files are reviewed; sites ending in `?` belong to files only placeholders cover, and their owners should review them when those files are stubbed or ported.
