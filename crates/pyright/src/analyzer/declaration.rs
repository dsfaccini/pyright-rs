//! Port of `packages/pyright-internal/src/analyzer/declaration.ts`.
//!
//! `Declaration` is one struct: the `DeclarationBase` fields plus `d`, an enum
//! with the fields of the specific declaration interface. A declaration is
//! shared (symbols and node info hold it, the binder mutates it), so it lives
//! in the arena as `P<Declaration>`. Optional fields are `Option`: the oracle
//! prints a field only when the object literal set it.

use std::cell::{Cell, RefCell};

use jsrt::{JsMap, P};

use crate::common::text_range::Range;
use crate::common::uri::Uri;
use crate::parser::parse_nodes::{ExpressionNode, NameNode, ParseNode, RaiseNode, ReturnNode};

thread_local! {
    // analyzer/declaration.ts:36
    static UNRESOLVED_MODULE_MARKER: Uri = Uri::constant("*** unresolved module ***");
}

/// `UnresolvedModuleMarker`: one constant Uri, compared by identity.
pub fn unresolved_module_marker() -> Uri {
    UNRESOLVED_MODULE_MARKER.with(|uri| *uri)
}

// analyzer/declaration.ts:38
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclarationType {
    Intrinsic,
    Variable,
    Param,
    TypeParam,
    TypeAlias,
    Function,
    Class,
    SpecialBuiltInClass,
    Alias,
}

// analyzer/declaration.ts:50
/// One of `'Any'`, `'str'`, `'str | None'`, `'int'`, `'MutableSequence[str]'`,
/// `'__class__'`, `'dict[str, Any]'`.
pub type IntrinsicType = &'static str;

// analyzer/declaration.ts:59
pub struct Declaration {
    // Category of this symbol (function, variable, etc.).
    // Used by hover provider to display helpful text.
    pub type_: DeclarationType,

    // Parse node associated with the declaration. Does not necessarily match
    // the path and range.
    pub node: Cell<P<ParseNode>>,

    // The file and range within that file that
    // contains the declaration. Unless this is an alias, then uri refers to the
    // file the alias is referring to.
    pub uri: Cell<Uri>,
    pub range: Cell<Range>,

    // The dot-separated import name for the file that
    // contains the declaration (may not be definitive
    // because a source file can be accessed via different
    // import names in some cases).
    pub module_name: RefCell<String>,

    // The declaration is within an except clause of a try
    // statement. We may want to ignore such declarations.
    pub is_in_except_suite: Cell<bool>,

    // This declaration is within an inlined TypedDict definition.
    pub is_in_inlined_typed_dict: Cell<Option<bool>>,

    pub d: DeclarationDetails,
}

/// The fields of the specific declaration interface, one variant per
/// `DeclarationType`.
pub enum DeclarationDetails {
    Intrinsic(IntrinsicDeclarationDetails),
    Variable(VariableDeclarationDetails),
    Param(ParamDeclarationDetails),
    TypeParam,
    TypeAlias(TypeAliasDeclarationDetails),
    Function(FunctionDeclarationDetails),
    Class,
    SpecialBuiltInClass,
    Alias(AliasDeclarationDetails),
}

impl Declaration {
    /// The `DeclarationBase` part of an object literal. `d` must match `type_`.
    pub fn create(
        type_: DeclarationType,
        node: P<ParseNode>,
        uri: Uri,
        range: Range,
        module_name: String,
        is_in_except_suite: bool,
        d: DeclarationDetails,
    ) -> P<Declaration> {
        P::new(Declaration {
            type_,
            node: Cell::new(node),
            uri: Cell::new(uri),
            range: Cell::new(range),
            module_name: RefCell::new(module_name),
            is_in_except_suite: Cell::new(is_in_except_suite),
            is_in_inlined_typed_dict: Cell::new(None),
            d,
        })
    }

    pub fn intrinsic(&self) -> Option<&IntrinsicDeclarationDetails> {
        match &self.d {
            DeclarationDetails::Intrinsic(d) => Some(d),
            _ => None,
        }
    }

    pub fn variable(&self) -> Option<&VariableDeclarationDetails> {
        match &self.d {
            DeclarationDetails::Variable(d) => Some(d),
            _ => None,
        }
    }

    pub fn param(&self) -> Option<&ParamDeclarationDetails> {
        match &self.d {
            DeclarationDetails::Param(d) => Some(d),
            _ => None,
        }
    }

    pub fn type_alias(&self) -> Option<&TypeAliasDeclarationDetails> {
        match &self.d {
            DeclarationDetails::TypeAlias(d) => Some(d),
            _ => None,
        }
    }

    pub fn function(&self) -> Option<&FunctionDeclarationDetails> {
        match &self.d {
            DeclarationDetails::Function(d) => Some(d),
            _ => None,
        }
    }

    pub fn alias(&self) -> Option<&AliasDeclarationDetails> {
        match &self.d {
            DeclarationDetails::Alias(d) => Some(d),
            _ => None,
        }
    }
}

// analyzer/declaration.ts:88
pub struct IntrinsicDeclarationDetails {
    pub name: String,
    /// `ModuleNode | FunctionNode | LambdaNode | ClassNode`, in `node`.
    pub intrinsic_type: IntrinsicType,
}

// analyzer/declaration.ts:107
pub struct FunctionDeclarationDetails {
    pub is_method: Cell<bool>,
    pub is_generator: Cell<bool>,
    pub return_statements: RefCell<Option<Vec<ReturnNode>>>,
    /// `(YieldNode | YieldFromNode)[]`.
    pub yield_statements: RefCell<Option<Vec<P<ParseNode>>>>,
    pub raise_statements: RefCell<Option<Vec<RaiseNode>>>,
}

// analyzer/declaration.ts:117
pub struct ParamDeclarationDetails {
    // Inferred parameters can be inferred from pieces of an actual NameNode, so this
    // value represents the actual 'name' as the user thinks of it.
    pub inferred_name: RefCell<Option<String>>,

    // Nodes that potentially makeup the type of an inferred parameter.
    pub inferred_type_nodes: RefCell<Option<Vec<ExpressionNode>>>,
}

// analyzer/declaration.ts:134
pub struct TypeAliasDeclarationDetails {
    // If a docstring (based on PEP 258) is present...
    pub doc_string: RefCell<Option<String>>,
}

// analyzer/declaration.ts:142
/// `node` is a `NameNode | StringListNode`.
#[derive(Default)]
pub struct VariableDeclarationDetails {
    // An explicit type annotation, if provided
    pub type_annotation_node: Cell<Option<ExpressionNode>>,

    // A source of the inferred type
    pub inferred_type_source: Cell<Option<P<ParseNode>>>,

    // Is the declaration considered "constant" (i.e.
    // reassignment is not permitted)?
    pub is_constant: Cell<Option<bool>>,

    // Is the declaration considered "final" (similar to
    // constant in that reassignment is not permitted)?
    pub is_final: Cell<Option<bool>>,

    // Is the declaration an entry in __slots__?
    pub is_defined_by_slots: Cell<Option<bool>>,

    // For most symbols in a "py.typed" file, type inference is not
    // allowed. But in certain cases (as with __match_args__ or __slots__),
    // inference is permitted.
    pub is_inference_allowed_in_py_typed: Cell<Option<bool>>,

    // Is the declaration using a runtime-evaluated type expression
    // rather than an annotation? This is used for TypedDicts, NamedTuples,
    // and other complex (more dynamic) class definitions with typed variables.
    pub is_runtime_type_expression: Cell<Option<bool>>,

    // If the declaration is a type alias, points to the alias name.
    pub type_alias_name: Cell<Option<NameNode>>,

    // Is the declaration a class or instance variable defined
    // by a member access, or is it a direct variable declaration
    // within the class?
    pub is_defined_by_member_access: Cell<Option<bool>>,

    // If an "attribute docstring" (as defined in PEP 258) is present...
    pub doc_string: RefCell<Option<String>>,

    // If set, indicates an alternative node to use to determine the type of the variable.
    pub alternative_type_node: Cell<Option<ExpressionNode>>,

    // Is the declaration an assignment through an explicit nonlocal or global binding?
    pub is_explicit_binding: Cell<Option<bool>>,
}

/// A shared `Map<string, ModuleLoaderActions>`.
pub type ImplicitImportMap = P<RefCell<JsMap<String, P<ModuleLoaderActions>>>>;

/// A new, empty `Map<string, ModuleLoaderActions>`.
pub fn create_implicit_import_map() -> ImplicitImportMap {
    P::new(RefCell::new(JsMap::default()))
}

// analyzer/declaration.ts:197
// Alias declarations are used for imports. They are resolved
// after the binding phase.
/// `node` is an `ImportAsNode | ImportFromAsNode | ImportFromNode`.
pub struct AliasDeclarationDetails {
    // Does this declaration use a local name or use the
    // imported symbol directly? This is used to find and
    // rename references.
    pub uses_local_name: Cell<bool>,

    // Indicate whether symbols can be loaded from the path.
    pub load_symbols_from_path: Cell<bool>,

    // The name of the symbol being imported (used for "from X import Y"
    // statements, not applicable to "import X" statements).
    pub symbol_name: RefCell<Option<String>>,

    // If there is a symbol name that can't be resolved within
    // the target module (defined by "path"), the symbol might
    // refer to a submodule with the same name.
    pub submodule_fallback: Cell<Option<P<Declaration>>>,

    // The first part of the multi-part name used in the import
    // statement (e.g. for "import a.b.c", firstNamePart would
    // be "a").
    pub first_name_part: RefCell<Option<String>>,

    // If the alias is targeting a module, multiple other modules
    // may also need to be resolved and inserted implicitly into
    // the module's namespace to emulate the behavior of the python
    // module loader. This can be recursive (e.g. in the case of
    // an "import a.b.c.d" statement).
    pub implicit_imports: Cell<Option<ImplicitImportMap>>,

    // Is this a dummy entry for an unresolved import?
    pub is_unresolved: Cell<Option<bool>>,

    // Is this a dummy entry for an import that cannot be resolved
    // directly because it targets a native library?
    pub is_native_lib: Cell<Option<bool>>,

    // Was this import declared with the "lazy" keyword (PEP 810)?
    pub is_lazy: Cell<Option<bool>>,
}

impl AliasDeclarationDetails {
    /// The required fields; every optional field unset.
    pub fn new(uses_local_name: bool, load_symbols_from_path: bool) -> AliasDeclarationDetails {
        AliasDeclarationDetails {
            uses_local_name: Cell::new(uses_local_name),
            load_symbols_from_path: Cell::new(load_symbols_from_path),
            symbol_name: RefCell::new(None),
            submodule_fallback: Cell::new(None),
            first_name_part: RefCell::new(None),
            implicit_imports: Cell::new(None),
            is_unresolved: Cell::new(None),
            is_native_lib: Cell::new(None),
            is_lazy: Cell::new(None),
        }
    }
}

// analyzer/declaration.ts:247
// This interface represents a set of actions that the python loader
// performs when a module import is encountered.
pub struct ModuleLoaderActions {
    // The resolved uri of the implicit import. This can be empty
    // if the resolved uri doesn't reference a module (e.g. it's
    // a directory).
    pub uri: Cell<Uri>,

    // Is this a dummy entry for an unresolved import?
    pub is_unresolved: Cell<Option<bool>>,

    // Indicate whether symbols can be loaded from the path.
    pub load_symbols_from_path: Cell<bool>,

    // See comment for "implicitImports" field in AliasDeclaration.
    pub implicit_imports: Cell<Option<ImplicitImportMap>>,
}

impl ModuleLoaderActions {
    pub fn create(uri: Uri, load_symbols_from_path: bool) -> P<ModuleLoaderActions> {
        P::new(ModuleLoaderActions {
            uri: Cell::new(uri),
            is_unresolved: Cell::new(None),
            load_symbols_from_path: Cell::new(load_symbols_from_path),
            implicit_imports: Cell::new(None),
        })
    }
}

/// TypeScript passes an `AliasDeclaration` wherever a `ModuleLoaderActions`
/// is expected: both have `uri`, `isUnresolved`, `loadSymbolsFromPath` and
/// `implicitImports`. This trait is that structural overlap.
pub trait ModuleLoaderActionsLike {
    fn uri(&self) -> &Cell<Uri>;
    fn is_unresolved(&self) -> &Cell<Option<bool>>;
    fn load_symbols_from_path(&self) -> &Cell<bool>;
    fn implicit_imports(&self) -> &Cell<Option<ImplicitImportMap>>;
}

impl ModuleLoaderActionsLike for ModuleLoaderActions {
    fn uri(&self) -> &Cell<Uri> {
        &self.uri
    }
    fn is_unresolved(&self) -> &Cell<Option<bool>> {
        &self.is_unresolved
    }
    fn load_symbols_from_path(&self) -> &Cell<bool> {
        &self.load_symbols_from_path
    }
    fn implicit_imports(&self) -> &Cell<Option<ImplicitImportMap>> {
        &self.implicit_imports
    }
}

/// Only for an alias declaration; panics on any other kind.
impl ModuleLoaderActionsLike for Declaration {
    fn uri(&self) -> &Cell<Uri> {
        &self.uri
    }
    fn is_unresolved(&self) -> &Cell<Option<bool>> {
        &self.alias().expect("alias declaration").is_unresolved
    }
    fn load_symbols_from_path(&self) -> &Cell<bool> {
        &self.alias().expect("alias declaration").load_symbols_from_path
    }
    fn implicit_imports(&self) -> &Cell<Option<ImplicitImportMap>> {
        &self.alias().expect("alias declaration").implicit_imports
    }
}

// analyzer/declaration.ts:274
pub fn is_function_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Function
}

// analyzer/declaration.ts:278
pub fn is_class_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Class
}

// analyzer/declaration.ts:282
pub fn is_param_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Param
}

// analyzer/declaration.ts:286
pub fn is_type_param_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::TypeParam
}

// analyzer/declaration.ts:290
pub fn is_type_alias_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::TypeAlias
}

// analyzer/declaration.ts:294
pub fn is_variable_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Variable
}

// analyzer/declaration.ts:298
pub fn is_alias_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Alias
}

// analyzer/declaration.ts:302
pub fn is_special_built_in_class_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::SpecialBuiltInClass
}

// analyzer/declaration.ts:306
pub fn is_intrinsic_declaration(decl: &Declaration) -> bool {
    decl.type_ == DeclarationType::Intrinsic
}

// analyzer/declaration.ts:310
pub fn is_unresolved_alias_declaration(decl: &Declaration) -> bool {
    is_alias_declaration(decl) && decl.uri.get().equals(Some(unresolved_module_marker()))
}
