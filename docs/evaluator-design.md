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
- A stored evaluator is `P<TypeEvaluator>`: struct fields, and closures stored in types (`ClassDetailsShared.synthesizeMethodsDeferred`) that must be `'static`. Get it with `self.evaluator_interface.get()`.
- `getCodeFlowEngine(evaluatorInterface, ...)` stores the evaluator, so it takes the `P<TypeEvaluator>` (section 4, stored parameters).

### other closure factories

The same rules apply to every `[factories]` entry. `getCodeFlowEngine` is `CodeFlowEngine` and its nested factory `createCodeFlowAnalyzer` is `CodeFlowAnalyzer`.

- The returned object is either a `const` named in `self_names` (`evaluatorInterface`) or the object literal of a top-level `return { .. }` (`getCodeFlowEngine`). Its members are the `pub` methods.
- A nested factory's struct gets a back-pointer field to its parent, `code_flow_engine: P<CodeFlowEngine>`, because its closures read the parent's locals. Its own nested functions are its methods.
- `&self` cannot become a `P<Self>`. A factory with a nested factory and no `self_names` gets `handle: OnceCell<P<Self>>`, set once the struct is in the arena, so its methods can hand the back-pointer to the nested struct.

### for the step 5 agents

- `type_evaluator.rs` holds the struct, `create_type_evaluator` and one `impl TypeEvaluator` block, all in TypeScript order. An agent that owns a line range fills the `todo!()` bodies whose markers fall in that range and leaves the rest alone. Git merges disjoint ranges of one file.
- Splitting the file into several modules later is mechanical: Rust allows many `impl TypeEvaluator` blocks, and the methods are already `pub(crate)` or `pub`.

## 2. Names and markers

- Names follow porting.md: types and enum members keep their TypeScript names; functions, methods, fields and parameters become snake_case with acronym runs kept together (`getJSXType` -> `get_jsx_type`), a leading `_` kept, and a trailing `_` on a Rust keyword (`type_`, `priv_`).
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
- A union whose members map to one Rust type is that type (`T | Promise<T>` is `T`).
- A union named in `[types]` by its sorted members maps as the table says: `"string | string[]"` is `Vec<String>`, param `&[String]`.
- Any other union renders as its first member followed by `/*? <TypeScript> */`. The run report lists every such site.

### collections

| TypeScript | owned | param | param the callee changes |
| --- | --- | --- | --- |
| `T[]`, `readonly T[]`, `Array<T>`, iterables | `Vec<T>` | `&[T]` | `&mut Vec<T>` |
| `Map<K, V>`, `ReadonlyMap`, `WeakMap`, `Record` | `JsMap<K, V>` | `&JsMap<K, V>` | `&mut JsMap<K, V>` |
| `Set<T>`, `ReadonlySet`, `WeakSet` | `JsSet<T>` | `&JsSet<T>` | `&mut JsSet<T>` |
| `[A, B]` | `(A, B)` | `(A, B)` | |
| `{ [key: string]: T }` | `JsMap<String, T>` | | |

A collection is a value in Rust and a shared reference in TypeScript. Where two holders share one collection and one of them changes it, map every site that carries it to a shared handle in `[sites]`:

- the binder's code-flow expression set, `P<RefCell<JsSet<String>>>`: the binder, its deferred tasks and the node-info store share it;
- the evaluator's type caches, `P<RefCell<JsMap<u32, ..>>>`: the speculative type tracker deletes from the map it was handed, and the return-inference caches are swapped per call.

A getter that returns live state its callers write to returns a reference into it, also through `[sites]`: `ClassType::get_symbol_table` returns `&'static RefCell<JsMap<..>>`, `TypeBase::add_props` returns `RefMut<'static, TypeBaseProps>`.

### callbacks

| position | Rust |
| --- | --- |
| param | `&mut dyn FnMut(A) -> R` |
| optional param | `Option<&mut dyn FnMut(A) -> R>` |
| field, element, type alias | `Rc<dyn Fn(A) -> R>` |
| return | `Box<dyn FnMut(A) -> R>` |

- `dyn`, never a generic `impl FnMut`: a generic callback passed down a recursive call instantiates without end.
- In a class whose methods take `&mut self` (an "owned" class, below), every callback parameter of an inherent method takes the receiver first: `_bindLoopStatement(.., callback: &mut dyn FnMut(&mut Binder))`. The closure cannot capture `self` while the method holds it, so it receives it. tsrs's checker does the same.

### objects

A class or interface is held one of four ways ("mode"). `[modes]` overrides the default.

| mode | Rust | param | default for |
| --- | --- | --- | --- |
| `handle` | `P<T>`, identity, shared | `P<T>` | classes; interfaces with a field written after construction, or compared with `===` |
| `value` | owned struct, `#[derive(Clone)]` | `&T`, or `&mut T` when the callee writes it | other interfaces |
| `owned` | owned struct, methods take `&mut self` | `&T` | classes that implement a trait class (walkers) |
| `opaque` | placeholder struct, ported by hand | `&T` | none; `Uri` |

Value types, value enums and inline-object structs derive `Clone`: the TypeScript copies them with spreads and reads them out of containers. Handle, owned and family structs derive nothing.

The default follows porting.md: an object written after construction, or compared with `===`, needs identity. Writes cannot show whether an object is shared, so the default errs toward the handle. The run report lists every interface the default made a handle, with the write that decided it, and every `[modes]` value that some code compares by identity.
`typemap.toml` makes an interface a value only where the TypeScript shows that copying it is safe:

- records copied with a spread before each write, never compared with `===` (`TypedDictEntry`, `ValidateArgTypeParams`, `MatchArgsToParamsResult`);
- the per-instance `priv`, `props` and `cached` records of a type, which `TypeBase.cloneType` copies with a spread;
- stack entries that live only in their stack (`SymbolResolutionStackEntry`, `SuppressedNodeStackEntry`).

Results stay handles: the evaluator caches a `TypeResult` and then writes it (typeEvaluator.ts:1348–1353), and returns a cached `EffectiveTypeResult` that its caller writes (24668, 25483). A copy would change pyright's output.

### mutability

The write analysis reads the whole check path:

- a field assigned outside its own constructor, or deleted, is written;
- an array, `Map` or `Set` changed in place (`push`, `splice`, `set`, `add`, `delete`, `clear`, `sort`, element assignment, `length = n`) is mutated; so is one passed to a parameter its callee mutates, through any number of calls (the analysis repeats until nothing changes);
- a call through a closure factory's interface (`evaluator.getTypeOfExpression(..)`) reaches the nested function behind it;
- `x.f.g = v` writes through `f`;
- a local initialized from a field (`const a = x.f`) stands for the field;
- `a === b` on two objects of one interface or class marks that type as compared by identity.

Then:

- In a handle struct, a field that is written, mutated, or written through while holding a value struct is `Cell<T>` for Copy types and `RefCell<T>` otherwise. `[mutable]` forces it on or off.
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
- Two options record the shape a hand port chose. `typed_handles = false` drops the typed handles, so a member is `P<Root>`; `empty_variants = "unit"` makes a member without fields of its own a unit variant. `Declaration` uses both, as `crates/pyright/src/analyzer/declaration.rs` does. Tokens name their root variant `None`, as `tokenizer_types.rs` does.

### traits

`[traits]` lists the classes and interfaces that become traits: those called through an interface or a base class with several implementations.

- A trait interface (`AnalyzerNodeInfoReader`) becomes a trait with one required method per method, and one accessor per data member. Params of its type are `&dyn T`; stored ones, fields and parameters the function stores, are `&'static dyn T`.
- A trait class (`ParseTreeWalker`) becomes a trait whose methods have default `todo!()` bodies. Its fields become required accessors; each implementing struct stores them and returns them. Params are `&mut dyn T`, stored ones `Box<dyn T>`.
- An implementing class gets one `impl Trait for Class` per trait in its ancestry, holding the methods that trait declares, with the trait's signature and the base's type arguments substituted (`_default` is `bool` in `impl ParseTreeVisitor<bool> for Binder`). The marker points at the implementing method. Its other methods go in an inherent `impl`.

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
- Other type parameters stay Rust generics: a value struct cannot stand for its subtypes (`addContextualTypeCacheEntry<T extends ContextualTypeCacheEntry>` keeps `T`). A struct type parameter no field uses gets a `PhantomData` field.
- A type parameter of a function type (`<T>(func: T) => ..`) has no Rust generic to bind to and takes its constraint.

### other TypeScript types

- `Partial<X>` is a generated struct `PartialX` beside `X` with every field optional.
- `ReturnType<..>`, `Parameters<..>` and similar utility types are resolved by the checker.
- `T extends X ? T : never` maps its true branch, with a `/*? .. */` marker.
- `Promise<T>` is `T`: nothing on the check path awaits.
- `CancellationToken` is `()`: nothing cancels the CLI check.
- `any`, `unknown`, `object` and unique symbols render as `()` with a `/*? .. */` marker, unless `[sites]` names the type.
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

## 5. Dependency placeholders and the compile check

A run writes `out/<run>/stubs/`, one module per stubbed file, and `out/<run>/deps/`: type-only placeholders for every file the stubs name that is not in the run.

- A placeholder holds the types other emitted code names, to a fixpoint, and the whole family when it holds a family. Never copy a placeholder into `crates/`.
- `--check` copies `crates/` to `target/stubgen-check/<run>` and runs `cargo check -p pyright` with:
  - each stub placed, unless the crate's file is already ported (a function with a real body): the port supersedes the stub, and the report lists it;
  - each placeholder placed where the crate's file is still an empty skeleton;
  - for a partially ported file, the items the stubs name that it lacks, served from the placeholder through a side module the file re-exports. The report lists them: they are requests to the file's owner.
- So the stubs compile against every real file on `main`; a name mismatch fails the check.
- Where a ported file chose a shape the rules would not give, `typemap.toml` records it: `TextRangeCollection<T>` keeps its generic, `ConsoleInterface` is a trait.
- `jsrt` items the stubs use but `crates/jsrt` lacks come from `tools/stubgen/jsrt_shim.rs`. The check report lists each: they are requests to the `jsrt` owner.

## 6. Open

- Collections shared by reference between holders that change them need a `[sites]` entry each; the write analysis does not see aliasing. Mapped so far: the binder's code-flow expression set and the evaluator's type caches. When `typeCacheUtils.ts` is stubbed, `SpeculativeEntry.cache` must hold those same handles, and when `codeFlowEngine.ts` is stubbed, its flow-node type cache (codeFlowEngine.ts:314) needs the same treatment.
- Interface inheritance between value structs is flattened, so a function that takes the base cannot take a derived struct. In the evaluator, `is_type_cache_entry_valid(Option<&TypeCacheEntry>)` is called with a `TypeFormTypeCacheEntry` (typeEvaluator.ts:821), and `read_contextual_type_cache_entry_for_node` returns one where it is typed `TypeCacheEntry` (788–799). Decide per site: a generic, a conversion, or a `[sites]` type.
- Handles never free. Results such as `TypeResult` are allocated per evaluation, and stack-like records the default makes handles (`FlowNodeTypeResult`, `SpeculativeContext`, `SpeculativeEntry`, in files not stubbed yet) leak per push. Measure at the step 4 checkpoint. A value form needs proof, per type, that no write follows a store.
- `RegExp` has no mapping yet; jsrt's `js_regex` translates patterns for the `regex` crate. Neither run uses it.
- The remaining `/*? .. */` sites in each run's report need a decision: `definedConstants` values (`boolean | string`), `ImportLookup`'s `Uri | AbsoluteModuleDescriptor` parameter, the `any` results in `staticExpressions.ts`.
- Number sites ending in `?` in `numbers.toml` are unreviewed guesses. The binder group, `typeEvaluator.ts` and `types.ts` have none; the remaining ones sit in files only placeholders cover so far.
- The evaluator stubs compile against the hand-ported binder group, `parse_nodes.rs`, `tokenizer_types.rs`, `parser.rs` and `common/` files on `main` at the time of the run. A later change to their names fails the check until the table follows.
- The binder group was ported by hand while the binder run was being built, so the check reports all nine binder stubs as superseded. The run stays as a record of the mapping on those files.
