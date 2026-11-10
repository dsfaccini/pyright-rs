# Evaluator design and stub mapping rules

How `tools/stubgen` turns pyright's TypeScript into Rust signatures. `docs/porting.md` is the contract; this file states how the generator applies it, rule by rule.
Change a rule here first, then in `tools/stubgen/typemap.toml` (per-type and per-site choices) or in the generator (the rule itself).

## 1. The evaluator

`createTypeEvaluator` in `analyzer/typeEvaluator.ts` builds the evaluator out of closures: 393 nested functions share 2 parameters and 25 top-level locals and call each other directly, and helper files call them through the `TypeEvaluator` interface (112 members, `analyzer/typeEvaluatorTypes.ts`).
The generator maps it, and every function configured under `[factories]`, as follows.

### one struct

- The factory becomes one struct, named after the interface it returns: `TypeEvaluator`, in `analyzer/type_evaluator.rs`.
- The TypeScript interface `TypeEvaluator` gets no Rust item of its own. Every reference to it names the struct.
- `createTypeEvaluator` becomes `create_type_evaluator(..) -> P<TypeEvaluator>`. The evaluator lives in the leak arena because other objects store it (below). Its parameters are taken by value: the struct stores them.
- `wrapWithLogger` is dropped (`drop_params`). It only adds timing and call logging, which the check path does not print.

### fields

- Each factory parameter is a field: `import_lookup`, `evaluator_options`.
- Each top-level `const` or `let` of the factory body is a field, in source order, with a `// <file>:<line>` marker.
- A local that is reassigned, or whose array, `Map` or `Set` is changed in place, is a `Cell` (Copy types) or a `RefCell` (others). The write analysis (section 3, "mutability") decides. A `let` is always interior-mutable.
- `evaluatorInterface`, the object the factory returns, is the struct itself (`self_names`). It becomes `evaluator_interface: OnceCell<P<TypeEvaluator>>`, set by `create_type_evaluator` once the struct is in the arena.
- A local initialized after `evaluatorInterface` from an expression that uses it is a `OnceCell` set during construction: `code_flow_engine: OnceCell<P<CodeFlowEngine>>`.
- `const x = wrapWithLogger(f)` is not a field. `x` becomes a method with `f`'s signature (`get_inferred_return_type_result`).

### methods

- Each function declared directly in the factory body is a `&self` method, in source order, with its own marker.
- A method is `pub` when the returned object lists it, so it is part of the `TypeEvaluator` interface, and `pub(crate)` otherwise.
- A member of the returned object that is not a nested function (`getAnalyzerNodeInfoReader: () => nodeInfo`) is a method with the interface's signature, marked at the property. A member that renames a function (`a: b`) is a method `a` with `b`'s signature.
- The signature comes from the nested function, not from the interface. The nested function may take more parameters (`signatureTracker`); callers through the interface pass `None` for them. TypeScript default parameters are plain parameters: every caller passes the value (section 4).
- Functions nested two levels deep (13 in the evaluator) stay inside their parent's body. The stub lists them in a `/// Nested in the TS body:` line, with lines. Port them as local functions or closures there.

### why `&self`, `Cell` and `RefCell`

- The nested functions re-enter each other through deep recursion, and callbacks such as `mapSubtypesExpandTypeVars(type, options, callback)` call back into the evaluator while it is running. `&self` allows all of it; `&mut self` would forbid a callback that captures `self` while a method holds it.
- Shared state is interior-mutable instead. Never hold a `RefCell` borrow across a call that can reach back into the evaluator (porting.md, "object references"): copy the value out first.
- Callback parameters are `&mut dyn FnMut(..)`. A closure that captures `&self` is fine, since every holder only reads `self`.

### where the evaluator is passed and stored

- A helper parameter of type `TypeEvaluator` is `&TypeEvaluator`. Inside a method, pass `self` where the TypeScript passes `evaluatorInterface`.
- A stored evaluator is `P<TypeEvaluator>`: struct fields, and the stored callbacks of section 3 (`SynthesizeMethodsDeferred.type_evaluator`). Get it with `self.evaluator_interface.get()`.
- `getCodeFlowEngine(evaluatorInterface, ...)` stores the evaluator, so it takes the `P<TypeEvaluator>` (section 4, stored parameters).

### other closure factories

The same rules apply to every `[factories]` entry. `getCodeFlowEngine` is `CodeFlowEngine` and its nested factory `createCodeFlowAnalyzer` is `CodeFlowAnalyzer`.

- The returned object is either a `const` named in `self_names` (`evaluatorInterface`) or the object literal of a top-level `return { .. }` (`getCodeFlowEngine`). Its members are the `pub` methods.
- A nested factory's struct gets a back-pointer field to its parent, `code_flow_engine: P<CodeFlowEngine>`, because its closures read the parent's locals. Its own nested functions are its methods.
- `&self` cannot become a `P<Self>`. A factory with a nested factory and no `self_names` gets `handle: OnceCell<P<Self>>`, set once the struct is in the arena, so its methods can hand the back-pointer to the nested struct.

### for the step 5 agents

- `type_evaluator.rs` holds the struct, `create_type_evaluator` and one `impl TypeEvaluator` block, all in TypeScript order. An agent that owns a line range fills the `todo!()` bodies whose markers fall in that range and leaves the rest alone. Git merges disjoint ranges of one file. `docs/step5-shares.md` holds the split.
- Splitting the file into several modules later is mechanical: Rust allows many `impl TypeEvaluator` blocks, and the methods are already `pub(crate)` or `pub`.

## 2. Names and markers

- Names follow porting.md: types and enum members keep their TypeScript names; functions, methods, fields and parameters become snake_case with acronym runs kept together (`getJSXType` -> `get_jsx_type`), a leading `_` kept, and a trailing `_` on a Rust keyword (`type_`, `priv_`). A method named `clone` is `clone_` (porting.md "names"); a namespace function `clone` (`FunctionType.clone`) keeps its name, since it takes no receiver.
- Every function, method, struct, enum, trait, alias, constant and factory field gets `// <path>.ts:<line>` on the line above. The run report checks that every TypeScript function, method and type declaration of a stubbed file has a marker.
- Two Rust items with one name in one `impl` or module get `_2`, `_3`; the run report lists each rename.

## 3. Type mapping

One TypeScript type renders differently by position. "Param" is a parameter, "owned" is a field, return value or container element.

### primitives and numbers

| TypeScript | owned | param |
| --- | --- | --- |
| `string`, string literal unions | `String` | `&str` |
| `boolean` | `bool` | `bool` |
| `void`, `undefined` (return) | no return type | |
| `never` (return) | `!` | |
| `bigint` | `jsrt::JsBigInt` | |
| `number \| bigint` | `jsrt::NumberValue` | |
| `number` | per site, from `numbers.toml` | |

A `number` cannot be classified from its type, so `numbers.toml` records one Rust type per site: `["<file>"] "<Container>.<member>[.<param>\|.return][#n]" = "<type>"`.
The generator adds a site it has not seen with a guess and a trailing `?`; a reviewer checks it and removes the `?`. The guess follows porting.md "numbers" by the site's last name: ids and the keys of `Map<number, _>` and `Set<number>` are `u32`; counters, depths and recursion counts are `u32`; offsets, lengths, lines, columns and indexes are `i32`; ratios and complexities are `f64`; anything else is `f64`, the faithful default.
A type alias of `number` (`TypeSourceId`) is one site, so one entry covers every use.

### optional values and unions

- `T | undefined`, `T | null`, `x?: T` and `x?: T` parameters are `Option<T>`. Inside `Option`, parameters keep their borrowed form: `Option<&str>`, `Option<&[T]>`, `Option<&T>`.
- A union of members of one family is the family root, `P<Root>` (below).
- A union of members of one enum is that enum.
- A union of string literals and string-literal aliases (`StructElementType | 'pad'`) is `String`; a union of number literals (`-1 | 0 | 1`) is one number site.
- A union whose members map to one Rust type is that type (`T | Promise<T>` is `T`).
- A union named in `[types]` by its sorted members maps as the table says: `"string | string[]"` is `Vec<String>`, param `&[String]`.
- Any other union renders as its first member followed by `/*? <TypeScript> */`. The run report lists every such site.

### collections

| TypeScript | owned | param | param the callee changes |
| --- | --- | --- | --- |
| `T[]`, `readonly T[]`, `Array<T>`, iterables | `Vec<T>` | `&[T]` | `&mut Vec<T>` |
| `Generator<T>`, `IterableIterator<T>` | `Box<dyn Iterator<Item = T>>` | `&mut dyn Iterator<Item = T>` | |
| `Map<K, V>`, `ReadonlyMap`, `WeakMap`, `Record` | `JsMap<K, V>` | `&JsMap<K, V>` | `&mut JsMap<K, V>` |
| `Set<T>`, `ReadonlySet`, `WeakSet` | `JsSet<T>` | `&JsSet<T>` | `&mut JsSet<T>` |
| `[A, B]` | `(A, B)` | `(A, B)` | |
| `{ [key: string]: T }` | `JsMap<String, T>` | | |

A TS generator (`function*`) runs lazily, interleaved with its caller, so it is an iterator, never a `Vec` (`getClassMemberIterator`, `getClassIterator`).

A collection is a value in Rust and a shared reference in TypeScript. Where two holders share one collection and one of them changes it, map every site that carries it to a shared handle in `[sites]`:

- the binder's code-flow expression set, `P<RefCell<JsSet<String>>>`: the binder, its deferred tasks and the node-info store share it;
- the evaluator's type caches and the code-flow node type cache (`CodeFlowTypeCache.cache`), `P<RefCell<JsMap<u32, ..>>>`: the speculative type tracker deletes from the map it was handed, and the return-inference caches are swapped per call. `SpeculativeEntry.cache` holds either kind as a `SpeculativeCache` enum, defined in `[items]`.
- a tuple's type arguments, `ClassDetailsPriv.tuple_type_args`, `Option<P<RefCell<Vec<TupleTypeArg>>>>`: `TypeBase.cloneType` spreads `priv` (types.ts:272), so every clone shares the array, and `getSequencePatternInfo` splices it in place (patternMatching.ts:1558-1684), which reaches every clone. `ClassType.specialize` copies the array (types.ts:963) and `transformTypeVarsInClassType`'s pair transform stores a fresh one (typeUtils.ts:3675), so both allocate a new handle. A reader that keeps the args across other calls copies them out (`.borrow().clone()`); one that only looks borrows within the statement (`.get().borrow()`).

A getter that returns live state its callers write to returns a reference into it, also through `[sites]`: `ClassType::get_symbol_table` returns `&'static RefCell<JsMap<..>>`, `TypeBase::add_props` returns `RefMut<'static, TypeBaseProps>`.

### callbacks

| position | Rust |
| --- | --- |
| param | `&mut dyn FnMut(A) -> R` |
| optional param | `Option<&mut dyn FnMut(A) -> R>` |
| field, element, factory local | a stored callback (below) |
| type alias a field names | a stored callback (below) |
| other type alias | `Rc<dyn Fn(A) -> R>` |
| return | `Box<dyn FnMut(A) -> R>` |

- `dyn`, never a generic `impl FnMut`: a generic callback passed down a recursive call instantiates without end.
- In a class whose methods take `&mut self` (an "owned" class, below), every callback parameter of an inherent method takes the receiver first: `_bindLoopStatement(.., callback: &mut dyn FnMut(&mut Binder))`. The closure cannot capture `self` while the method holds it, so it receives it. tsrs's checker does the same. In a trait class with a `&mut self` receiver, the trait's methods and their implementations pass `&mut Self`, so the closure reaches the implementor's overrides: `TypeVarTransformer::do_for_each_constraint_set(.., callback: &mut dyn FnMut(&mut Self) -> FunctionType)`.

### stored callbacks

porting.md "memory": a closure that a struct stores becomes a struct with named fields plus a method, so a collector sees every `P` it holds (`tools/stubgen/src/callbacks.ts`).

- A slot is a field, closure-factory local or container element of function type, or a function type alias that a field names. Its creation sites are the closures assigned to it anywhere on the check path, and named functions passed to it.
- Each site gets a struct with one field per variable it captures, in first-use order, and `pub fn call(&self, ..)` with a `todo!()` body under the site's marker. The struct lives in the module that declares the slot; its marker names the file of the closure.
- One site: the struct takes the slot's name (`ClassDetailsShared.synthesizeMethodsDeferred` is `SynthesizeMethodsDeferred`). Several: one struct per site named after its enclosing function, and an enum named after the slot whose `call` dispatches.
- Captures: a local or parameter of an enclosing function is a field of its type; a closure factory's locals and nested functions are its struct (`type_evaluator: P<TypeEvaluator>`); `this` is the class; a local function or `const` closure the closure calls adds its own captures and a doc line. A captured `let` that the TypeScript reassigns gets a comment: the struct holds a copy.
- A slot whose every site captures nothing is a `fn` pointer (`typeFormSpecialFormDiagnosticFactories` holds `fn() -> String`).
- Aliases only passed or returned (`TypeNarrowingCallback`) stay `Rc<dyn Fn>`: no struct keeps them.
- A function type alias that `[types]` maps is not a slot. The table gives its Rust type, `[items]` defines it, and the generator emits nothing at the alias. `FunctionReturnTypeCallback` (typePrinter.ts:104) is a trait held as `P<dyn FunctionReturnTypeCallback>` in every position, a Copy handle like the JS function reference. Pyright passes three different functions to it: the evaluator's own nested `getEffectiveReturnType`, so `TypeEvaluator` implements the trait (typeEvaluator.ts:30313) and hands the printer `evaluator_interface.to_dyn(..)` without allocating; the oracle dumper's, which never runs inference (`oracle/types.rs`); and the typePrinter tests' (2026-11-10).

### objects

A class or interface is held one of four ways ("mode"). `[modes]` overrides the default.

| mode | Rust | param | default for |
| --- | --- | --- | --- |
| `handle` | `P<T>`, identity, shared | `P<T>` | classes and interfaces compared by identity, or written after being stored in a second place |
| `value` | owned struct, `#[derive(Clone)]` | `&T`, or `&mut T` when the callee writes it | other interfaces |
| `owned` | owned struct; methods take `&mut self` when some code writes a field | `&T`, or `&mut T` when the callee changes it | other classes; classes that implement a trait class (walkers) |
| `rc` | `Rc<T>`, shared and immutable | `&T` | classes stored in a second place and never written |
| `opaque` | placeholder struct, ported by hand | `&T` | none; `Uri` |

Value types, value enums and inline-object structs derive `Clone`: the TypeScript copies them with spreads and reads them out of containers. One whose fields are all optional also derives `Default`, so an options literal `{ ignoreTypeFlags: true }` is `TypeSameOptions { ignore_type_flags: Some(true), ..Default::default() }`. Handle, owned and family structs derive nothing.

The default follows porting.md "memory": an object is a `P` only when pyright compares it with `===`, or writes to it after storing it elsewhere. The write analysis decides both:

- compared: `a === b` on two objects of the type, or an object of the type used as a `Map` key or `Set` element, which hashes by identity;
- written: a field assigned outside the constructor, a field's collection changed in place, or a method that does either called on it;
- stored: an existing object of the type put in a second place: assigned to a field or element, pushed or set into a container, placed in an object or array literal, copied with its container (`[...a]`, `slice`, `Array.from`), or passed to a parameter the callee stores. A fresh object (`new`, a literal) placed straight into one place is not stored.

The analysis is per type, not per object, so it errs toward the handle. The run report lists, for each class and interface of the stubbed files, its mode with the identity, write and store sites that decided it. It also lists every value-mode type whose fields some code writes through a parameter, directly or by passing the parameter on (the write analysis follows calls): a callee that writes a copy loses the write at its caller. The main session decides each listed type. `[modes]` overrides it; each override in `typemap.toml` says why.

Decisions on the types `docs/research/memory-advice.md` names (2026-11-10):

- `ConstraintSet` stays a handle. `ConstraintTracker.cloneWithSignature` gives the clone the original's set objects (constraintTracker.ts:204) and both trackers later write them.
- `ConstraintTracker` stays a handle. `validateArgTypeParams` returns the caller's own tracker inside `constraintTrackers` (typeEvaluator.ts:13820, 13871, 13875), and an overload match stores its tracker in `MatchedOverloadInfo` (10089); the tracker is written through `copyFromClone` and `addConstraintSets`. The identity hit at protocols.ts:366 is a false positive: that `===` only runs when one side is `undefined`. An owned tracker needs a proof per call site that no write follows a store.
- `DiagnosticAddendum` (common/diagnostic.ts) is written after it is stored: `createAddendum` pushes the new child into its parent and returns it, and callers write the child. It stays a handle, as ported.
- `SpeculativeContext` and `SpeculativeEntry` are values: each lives only in the speculative stack, which `disableSpeculativeMode` moves out and `enableSpeculativeMode` moves back. `SpeculativeEntry.cache` is a `SpeculativeCache` (below).
- `TypeResult` stays a handle: `getTypeOfExpression` writes `typeResult.type` after `writeTypeCache` stored it.
- `MatchArgsToParamsResult` is a handle, with `argument_match_score` a `Cell` and `arg_params` a `RefCell` (2026-11-10). `validateArgTypes` writes `argumentMatchScore` (typeEvaluator.ts:13575) into the result that `validateOverloadedArgTypes` still holds in `filteredMatchResults`, and `evaluateUsingBestMatchingOverload` reads it back from there (10900). The other write, `argParams` (10053), goes to a spread copy before it is stored. Rule: a value whose fields pyright writes after another holder keeps it (an array, a struct field, a variable that outlives the call) is a handle; `&mut` only when the parameter chain from its creator is the only path to it.
- `ParamListDetails` is a value, passed as `&mut` where it is written (`[modes]` pins it). Its one write site, `adjustSourceParamDetailsForDestVariadic` (typeEvaluator.ts:28091), writes `srcDetails`; the one caller (28256) passes one of two locals fresh from `getParamListDetails` (28248, 28251) and reads them afterward. Every other `getParamListDetails` result lives in a local of its caller (checker.ts:434, 5175; constructorTransform.ts:224; typeEvaluator.ts:12092, 16271, 20000, 23523, 29345, 29384), and none is put in an array, a struct field or a variable that outlives the call.
- `ProtocolCompatibilityCheckState` stays a value passed as `&mut`, by the rule above: `setProtocolCompatibility` creates it (protocols.ts:324), passes it only to `assignToProtocolInternal`, which writes `isOverloadedTypeBindingFailure` (572, 637), and reads it back (341).
- `InferenceContext` is a handle, `P<InferenceContext>` (2026-11-10, `[modes]`), allocated with `P::new_eval`. `getTypeOfExpression` writes `inferenceContext.expectedType` in place after its caller handed the context on (typeEvaluator.ts:1251, 15248, 15855; tuples.ts:159), and every holder sees the write. A `&mut` parameter matches that only while nobody stores the context or passes it twice; a handle matches it everywhere. `expected_type` is a `Cell`. A spread `{ ...ctx, expectedType: t }` is a new `P` with the fields copied.
- `TypeAliasSharedInfo.computedVariance` is a shared array, `Cell<Option<P<RefCell<Vec<Variance>>>>>` (2026-11-10, `[sites]`), and so are `inferVarianceForTypeAlias`'s return value and `updateUsageVariancesRecursive`'s `usageVariances`. Pyright caches the array before inferring it and fills it in place (typeEvaluator.ts:8355-8484), so a re-entrant read during the traversal sees the partial array.
- Other types the rule turned from handles into values: `FlowNodeTypeResult`, `ExpectedTypeCacheEntry`, `CallResult`, `ExpectedTypeOptions`, `PrefetchedTypes`, `ClassMember`, `ParamListDetails`, `ParamAssignmentInfo`, `LiteralTypes`, `SignatureWithOffsets`, `ProtocolCompatibilityCheckState`, `TypeVarUsageInfo`. `ConstraintSolution` is an `Rc`. `SpeculativeTypeTracker`, `UniqueNameMap`, `ParamAssignmentTracker`, the `TypeVarTransformer` classes and `NumberReference` are owned; a struct that holds one and is shared wraps it in a `RefCell`. The evaluator and its code flow engine share one tracker (typeEvaluator.ts:30591), so both hold it as `P<RefCell<SpeculativeTypeTracker>>` (`[sites]`).

`typemap.toml` makes an interface a value only where the TypeScript shows that copying it is safe:

- records copied with a spread before each write, never compared with `===` (`TypedDictEntry`, `ValidateArgTypeParams`);
- the per-instance `priv`, `props` and `cached` records of a type, which `TypeBase.cloneType` copies with a spread;
- stack entries that live only in their stack (`SymbolResolutionStackEntry`, `SuppressedNodeStackEntry`).

Results stay handles: the evaluator caches a `TypeResult` and then writes it (typeEvaluator.ts:1348–1353), and returns a cached `EffectiveTypeResult` that its caller writes (24668, 25483). A copy would change pyright's output.

Evaluator and checker code allocates with `P::new_eval`; every stub of a run with `heap = "eval"` (`runs.toml`) says so in its header.

### mutability

The write analysis reads the whole check path:

- a field assigned outside its own constructor, or deleted, is written;
- an array, `Map` or `Set` changed in place (`push`, `splice`, `set`, `add`, `delete`, `clear`, `sort`, element assignment, `length = n`) is mutated; so is one passed to a parameter its callee mutates, through any number of calls (the analysis repeats until nothing changes);
- a method that writes a field of `this`, changes one in place, or calls such a method on `this` changes `this`; calling it changes the receiver, as `push` does;
- a call through a closure factory's interface (`evaluator.getTypeOfExpression(..)`) reaches the nested function behind it;
- `x.f.g = v` writes through `f`;
- a local initialized from a field (`const a = x.f`) stands for the field;
- `a === b` on two objects of one interface or class marks that type as compared by identity.

Then:

- In a handle struct, a field that is written, mutated, or written through while holding a value struct is `Cell<T>` for Copy types and `RefCell<T>` otherwise. `[mutable]` forces it on or off; a factory local is keyed like its `[sites]` entry (`createTypeEvaluator.speculativeTypeTracker`).
- Value and owned structs have plain fields; their holders change them through `&mut`.
- A parameter that the function mutates, or writes through, renders in its `&mut` form.

### families: unions discriminated by a kind field

porting.md "types, classes and unions" and "parse nodes". `[families]` lists them: `ParseNode`, `Token`, `Type`, `Declaration`, `FlowNode`.

- The base interface's fields go in one struct named after the family (`ParseNode`, `Type`), plus `d: <Family>Details`, an enum with one variant per kind. The variant takes the kind's enum member name (`TypeDetails::Class`); a family without a kind field (`FlowNode`) takes the interface name.
- Each member interface gets `<Member>Details` with its own fields and a typed handle `<Member>(pub P<Root>)` with `cast`, `d()` and `Deref` to the root.
- A base field typed `object` is a per-kind slot: each member redeclares it (`shared`, `priv` on types). It lives in the member's details. `flatten = "d"` spreads the slot's object type into the details struct, as parse nodes do.
- A base field that a member only narrows (`node: ClassNode` on `ClassDeclaration`) stays on the root with the base type.
- A member that extends another member with the same kind (`ParamSpecType` extends `TypeVarType`) shares its parent's variant: it gets a typed handle without `cast`, because only `shared.kind` tells them apart. `[merge]` folds `ParamSpecDetailsPriv` and `TypeVarTupleDetailsPriv` into `TypeVarDetailsPriv`, which gains their extra fields as `Option`s.
- In a family without a kind field, a member's details embed its parent member's details as `base` (`FlowBranchLabelDetails.base: FlowLabelDetails`), so `FlowLabel::d()` works on every label variant.
- References: the union and the base are `P<Root>`; a member is its typed handle; an alias of a union of members (`ExpressionNode`) is `pub type ExpressionNode = P<ParseNode>`.
- `all_fields_mutable` makes every field `Cell`/`RefCell` except the kind field and `id`, as porting.md requires for parse nodes.
- `private_details` makes the details field and enum private to the family's module (`Type`). Other code reaches details only through typed handles and `d()`, and creates objects only through the module's create functions (porting.md "memory": the layout will change).
- Two options record the shape a hand port chose. `typed_handles = false` drops the typed handles, so a member is `P<Root>`; `empty_variants = "unit"` makes a member without fields of its own a unit variant. `Declaration` uses both, as `crates/pyright/src/analyzer/declaration.rs` does. Tokens name their root variant `None`, as `tokenizer_types.rs` does.

### traits

`[traits]` lists the classes and interfaces that become traits: those called through an interface or a base class with several implementations.

- A trait interface (`AnalyzerNodeInfoReader`) becomes a trait with one required method per method, and one accessor per data member. Params of its type are `&dyn T`; stored ones, fields, returns and parameters the function stores, are `P<dyn T>` (porting.md "memory": never a `&'static`). `P::to_dyn` makes one from the `P` of the implementing struct.
- A trait class (`ParseTreeWalker`, `TypeWalker`, `TypeVarTransformer`) becomes a trait whose methods have default `todo!()` bodies. Its fields become required accessors; each implementing struct stores them and returns them. A field the write analysis marks written or mutated also gets `<name>_mut(&mut self) -> &mut T`. Params are `&mut dyn T`, stored ones `Box<dyn T>`.
- An implementing class gets one `impl Trait for Class` per trait in its ancestry, holding the methods that trait declares, with the trait's signature and the base's type arguments substituted (`_default` is `bool` in `impl ParseTreeVisitor<bool> for Binder`). The marker points at the implementing method. Its other methods go in an inherent `impl`.
- `accessors` in a `[traits]` entry records how a hand-ported trait reads a trait-class field: a new name (`_default` is `default_visit_result`), by value, or no accessor and no field (`drop`: `_getStringAnnotation` is the method `get_string_annotation`).

### type aliases

| aliased type | Rust |
| --- | --- |
| union of family members | `pub type X = P<Root>` |
| union of string literals | `enum X` with `as_str()` |
| union of unrelated types | `enum X { A(A), B(B) }` |
| function type | `pub type X = Rc<dyn Fn(..)>` |
| object type | `struct X` |
| anything else | inlined at each use; `pub type X = ..` kept for reference |

An alias declared inside a function body is inlined.

### enums

- An enum with a `<<` or `|` initializer, or listed in `[enums] flags`, is a flags newtype `pub struct X(pub u32)` with one associated constant per member and `jsrt::flag_ops!(X)` for the bit operators and `any()`.
- Any other numeric enum is `#[repr(i32)]` with the same values. A member that repeats an earlier value is an associated constant.
- A string enum is a plain enum with `as_str()`.

### generics

- A type parameter whose constraint maps to a Copy handle (`P<T>`, a typed handle, an enum) is replaced by its constraint, in functions and in structs: `<T extends Type>(type: T): T` is `(type_: P<Type>) -> P<Type>`, and `TypeResult<T extends Type = Type>` is `struct TypeResult`. A caller that knows the member type casts the handle back.
- Other type parameters stay Rust generics (`ensureExpectedTypeCandidates<T>` keeps `T`): a value struct cannot stand for its subtypes. A struct type parameter no field uses gets a `PhantomData` field.
- A type parameter of a function type (`<T>(func: T) => ..`) has no Rust generic to bind to and takes its constraint.
- `[bounds]` keeps a function's type parameter as a Rust generic with the bounds it gives, instead of its constraint. A trait constraint needs it: `addContextualTypeCacheEntry<T extends ContextualTypeCacheEntry>` is `<T: Clone + ContextualTypeCacheEntry>`, called with `P<SpeculativeTypeEntry>` and `TypeFormTypeCacheEntry`.

### other TypeScript types

- `Partial<X>` is a generated struct `PartialX` beside `X` with every field optional.
- `ReturnType<..>`, `Parameters<..>` and similar utility types are resolved by the checker.
- `T extends X ? T : never` maps its true branch, with a `/*? .. */` marker.
- `Promise<T>` is `T`: nothing on the check path awaits.
- `CancellationToken` is `()`: nothing cancels the CLI check.
- `any`, `unknown`, `object` and unique symbols render as `()` with a `/*? .. */` marker, unless `[sites]` names the type.
- `RegExp` is `regex::Regex`. Pyright's patterns have no `u` flag: spell JS-only escapes with the `jsrt::js_regex` fragments.
- An inline object type is a struct named after its site.

## 4. Items

- A function with a body becomes `pub fn` (exported) or `pub(crate) fn`, with `todo!()`. An overload set gives the implementation's signature.
- A default parameter (`flags = EvalFlags.None`) is a plain parameter; a `/// TS defaults:` line records the default. A rest parameter is a slice. A destructured parameter is `argN`.
- A parameter the function stores takes the form of the field that keeps it: by value, `P<T>` for a factory struct, `&'static dyn T` for a trait. Stored means a constructor parameter property, a parameter the body assigns to a property as is (`this._x = x`), or any parameter of a closure factory.
- A missing return annotation takes the checker's inferred type.
- A class becomes a struct and an `impl`: `new(..)` returns `P<Self>` for handles and `Self` otherwise; getters are methods; setters are `set_<name>`; static methods are associated functions; static fields follow the module-variable rules below.
- A namespace merged with a type adds associated functions to it (`ClassType::create_instantiable`). Any other namespace becomes a unit struct of the same name (`TypeBase`) holding them; next to a trait of that name, the unit struct is `<Name>Fns`.
- A module `const` with a literal value is a Rust `const`. Every other module variable is a `thread_local!` static: `Cell`/`RefCell` when it is a `let` or some code writes it, with a `todo!()` initializer unless the value is a literal. Its marker sits inside the `thread_local!` block, above the `static`.
- Variables of a namespace and static fields of a class go in a module named after it: `UnboundType._instance` is `unbound_type::INSTANCE`.
- A closure factory follows section 1. Its builder function gets no `/// Nested in the TS body:` line: its nested functions are the struct's methods.
- A declaration in `skip` keeps only its marker (`LogWrapper`, the type of the dropped `wrapWithLogger`).
- `[items]` defines Rust items verbatim in a file's module: a type the mapping needs but the TypeScript does not declare (`SpeculativeCache`), or an `impl` that stands in for a TypeScript object literal or spread (`TypeResult::new` sets `type_` and leaves every other field unset; `Clone for TypeResult` copies a spread, which `P::new_eval` gives its own identity; `TypeResultWithNode::new` and `TypeResultWithNode::from_type_result` build `{ type, node }` and `{ ...typeResult, node }`).

## 5. Dependency placeholders and the compile check

A run writes `out/<run>/stubs/`, one module per stubbed file, and `out/<run>/deps/`: type-only placeholders for every file the stubs name that is not in the run.

- A placeholder holds the types other emitted code names, to a fixpoint, and the whole family when it holds a family. Never copy a placeholder into `crates/`.
- `--check` copies `crates/` to `target/stubgen-check/<run>` and runs `cargo check -p pyright` with:
  - each stub placed, unless the crate's file is already ported (a function with a real body): the port supersedes the stub, and the report lists it;
  - each placeholder placed where the crate's file is still an empty skeleton;
  - for a partially ported file, the items the stubs name that it lacks, served from the placeholder through a side module the file re-exports. The report lists them: they are requests to the file's owner.
- So the stubs compile against every real file on `main`; a name mismatch fails the check.
- A placeholder for a file the crate already ports names no further placeholders, so the check never chases a ported file's own dependencies. A partially ported file is served only the items the placed files import from it.

### install

A run with `install = true` (`runs.toml`) is installed into `crates/pyright/src` by `stubgen --run <run> --install`, and its check places the same files:

- a crate file with no ported function at a marker (a skeleton, a placeholder, an unchanged stub) is replaced;
- a crate file with ported functions is the base of its merge (`tools/stubgen/src/merge.ts`), and only its unported items change: a `todo!()` function or static, or a generated struct or enum, takes the stub's text when it differs; a new stub item goes in after its predecessor; an unported item the stub no longer has is removed. Its header, imports, ported functions and every other line stay byte for byte. An unchanged file is not rewritten, and the install runs no rustfmt;
- an item is a marker plus the function, static, struct or enum under it, keyed by marker and kind. A marked item inside another (a local function in a ported body) belongs to the outer one;
- a ported function keeps its own signature. After a signature change, update each ported function's signature by hand;
- what the merge cannot place is reported and left alone: a ported function whose marker the stub lost, a new item with no placed predecessor, and stub lines outside items that the file lacks (impl headers, accessors, `[items]` blocks). The file's owner adds those by hand;
- an import that a changed or inserted item names and the file lacks is added after the file's last import.

A run without `install` is a record: a ported crate file supersedes its stub.
- Where a ported file chose a shape the rules would not give, `typemap.toml` records it: `TextRangeCollection<T>` keeps its generic, `ConsoleInterface` is a trait.
- `jsrt` items the stubs use but `crates/jsrt` lacks come from `tools/stubgen/jsrt_shim.rs`. The check report lists each: they are requests to the `jsrt` owner.

## 6. Open

- Collections shared by reference between holders that change them need a `[sites]` entry each; the write analysis does not see aliasing. Mapped so far: the binder's code-flow expression set, the evaluator's type caches, the code-flow node type cache and a tuple's type arguments.
- Interface inheritance between value structs is flattened, so a function that takes the base cannot take a derived struct. In the evaluator, `is_type_cache_entry_valid(Option<&TypeCacheEntry>)` is called with a `TypeFormTypeCacheEntry` (typeEvaluator.ts:821), and `read_contextual_type_cache_entry_for_node` returns one where it is typed `TypeCacheEntry` (788–799). Decide per site: a generic, a conversion, or a `[sites]` type. `ContextualTypeCacheEntry` is a trait (`[traits]`); `[items]` holds its impls for `SpeculativeTypeEntry` and `TypeFormTypeCacheEntry`, since the generator emits impls only for classes.
- The handle rule is per type. A type made a handle because some object of it is stored and some object of it is written may have objects that are never both; a per-site proof can move it to a value later, through `[modes]`.
- Captured `let`s that the TypeScript reassigns (`SynthesizeMethodsDeferred.is_named_tuple_subclass` and others) are copied into the callback struct. Where a write follows the closure, the port needs a shared cell.
- The remaining `/*? .. */` sites are in files outside the evaluator run: `definedConstants` values (`boolean | string`), `ImportLookup`'s `Uri | AbsoluteModuleDescriptor` parameter, the `any` results in `staticExpressions.ts`.
- Number sites ending in `?` in `numbers.toml` are unreviewed guesses. The binder group and every file of the evaluator run have none.
- The evaluator stubs compile against the hand-ported binder group, `parse_nodes.rs`, `tokenizer_types.rs`, `parser.rs` and `common/` files on `main` at the time of the run. A later change to their names fails the check until the table follows.
- The binder group was ported by hand while the binder run was being built, so the check reports all nine binder stubs as superseded. The run stays as a record of the mapping on those files.
