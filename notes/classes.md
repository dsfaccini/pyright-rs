# classes notes

Share H of step 5 (`docs/step5-shares.md`): `dataClasses.ts`, `typedDicts.ts`, `protocols.ts`, `enums.ts`, `decorators.ts`, `properties.ts`, `tuples.ts`, `namedTuples.ts`, `constructorTransform.ts`, `typeWalker.ts`, `typeComplexity.ts`, `sentinel.ts`, `symbolUtils.ts`. Every marker is filled.

## Signature changes

- `analyzer/typeWalker.ts:TypeWalker` is a trait (`[traits]` in `typemap.toml`, receiver `&mut self`). `typeUtils.ts` subclasses it (`ContainsLiteralTypeWalker`, `AnyWalker`, `AnyOrUnknownWalker`) and overrides its visit methods; an implementing struct stores `_recursion_count`, `_is_walk_canceled` and `_hit_recursion_limit` and returns them through the accessors.
- Generator: a trait-class field that the write analysis marks written or mutated gets a second accessor, `<name>_mut(&mut self) -> &mut T` (`tools/stubgen/src/emit.ts`, rule in `docs/evaluator-design.md` section 3, "traits"). Only `TypeWalker` has such fields.
- `ProtocolCompatibilityCheckState` stays a value passed as `&mut` (`[modes]`; rule and reasons in `docs/evaluator-design.md` section 3, "objects"): its creator's parameter chain is the only path to it (protocols.ts:324, 572, 637, 341).
- Generator: an `rc` object in parameter position renders as `&T` and no longer imports `std::rc::Rc` (`tools/stubgen/src/ir.ts`). Nine stubs lose an unused import; a crate file that has the import keeps it through the merge.

## Shared-file edits

- `tools/stubgen/typemap.toml`, `tools/stubgen/src/emit.ts`, `tools/stubgen/src/ir.ts`, `docs/evaluator-design.md`: the changes above.
- New `crates/pyright/src/analyzer/doc_string_conversion.rs`: `convertDocStringToPlainText` and `_isUndefinedOrWhitespace` of `docStringConversion.ts`, which `getDeprecatedMessageFromCall` (decorators.ts:648) applies to a `@deprecated("...")` message. No share owns that file; the Markdown conversion is LSP-only and stays unported. `crates/pyright/src/analyzer/mod.rs`: its `pub mod` line.

## Needs from others

- `Declaration::create` (`declaration.rs`), `Symbol::new` and `Symbol::create_with_type` (`symbol.rs`) allocate with `P::new`. Declarations and symbols this share synthesizes (namedTuples.ts:52, typedDicts.ts:79, 985, dataClasses.ts:188, properties.ts) land on the program heap. They need the evaluator heap, as `DiagnosticAddendum` got.
- Share A (`types.rs`): a spread copy for `ClassDetailsShared` (`{ ...shared }`, namedTuples.ts:426, which inlines a 31-field literal; `cloneWithNewFlags` at types.ts:1062 needs the same), `DataClassEntry` (`{ ...entry, mroClass }`, dataClasses.ts:1287) and `DataClassBehaviors` (`{ ...defaultBehaviors }`, dataClasses.ts:1678). The hand copies miss any field added later.
- `EvaluatorUsage` has no constructor for the TS default `{ method: 'get' }`; constructorTransform.ts:66 writes the literal.
- When `InferenceContext` becomes a handle, these sites pass a fresh context as `make_inference_context(..).as_mut()` and change with it: tuples.ts:65, 152; enums.ts:316; constructorTransform.ts:214 (four calls); dataClasses.ts:188.

## Doubts

- No pyright unit test covers this share's files. They run only through the sample tests, which need the whole evaluator.
- A string list's values are joined as `JsString` and then decoded lossily (sentinel.ts:19, enums.ts:84, namedTuples.ts:52, typedDicts.ts:79, 985, decorators.ts:648): the known lone-surrogate gap of `docs/porting.md`, "strings".
- tuples.ts:612: a `bigint` slice index converts to `f64`. JavaScript throws when it adds a negative `bigint` to the tuple length; the port clamps to 0 instead.
- namedTuples.ts:426: `{ ...shared }` shares `typeParams`, `dataClassEntries`, `namedTupleEntries`, `localSlotsNames`, `protocolCompatibility` and `inheritedSlotsNamesCached` with the original in TypeScript; the Rust copy clones them. A later in-place write to the original's arrays would diverge.
- TypeScript shares one `FunctionParam` object across several synthesized overloads (dataClasses.ts:188, typedDicts.ts:313); `FunctionType::add_param` takes a copy. This matters only if something writes a parameter in place.
- `SymbolTable::for_each` walks by index (enums.ts:58, dataClasses.ts:188, typedDicts.ts): entries added during a walk are visited, as in JavaScript, but a delete during a walk would shift the index.
- protocols.ts:83 (`try/catch` pop) and enums.ts:316 (`try/finally` pop) pop their stacks on the normal path only; a Rust panic is a port bug anyway (`docs/porting.md`, "exceptions").
- Unchecked `as ClassType` casts stay unchecked (`ClassType(..)`): protocols.ts:83, 372; typedDicts.ts:1367. If the value is not a class, `.d()` panics where TypeScript carries on.
- `literalValue as string` and `as number` are matches with `unreachable!()` for any other variant (dataClasses.ts:188, typedDicts.ts:1367, 1513, namedTuples.ts:52).
- `tools/stubgen --install` drops `//` comment lines placed above a marker, since they sit outside the function it keeps. This share puts each leading TypeScript comment between its marker and the `fn` line.
- An install gives a fully ported file the generated "Stubs for … Every body is `todo!()`" header back. The files keep that header so a rerun changes nothing.
