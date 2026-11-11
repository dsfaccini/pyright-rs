//! Port of `packages/pyright-internal/src/analyzer/scope.ts`.

use jsrt::cell::{Cell, RefCell};
use std::rc::Rc;

use jsrt::{JsMap, P};

use super::declaration::DeclarationType;
use super::symbol::{Symbol, SymbolFlags, SymbolTable};
use crate::common::debug::fail;

// analyzer/scope.ts:16
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum ScopeType {
    // Used for PEP 695-style type parameters.
    TypeParameter,

    // Used for comprehension nodes.
    Comprehension,

    // Function scopes are used for lambdas and functions.
    Function,

    // Class scopes are used for classes.
    Class,

    // Module scopes are used for modules.
    Module,

    // Built-in scopes are used for all ambient symbols provided
    // by the Python environment.
    Builtin,
}

// analyzer/scope.ts:37
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub enum NameBindingType {
    // With "nonlocal" keyword
    Nonlocal,

    // With "global" keyword
    Global,
}

// analyzer/scope.ts:46
// Provides information for recursive scope lookups.
#[derive(Clone, Copy, Debug)]
#[derive(jsrt::Trace)]
pub struct SymbolWithScope {
    // Found symbol
    pub symbol: P<Symbol>,

    // Scope in which symbol was found
    pub scope: P<Scope>,

    // Indicates that the recursion needed to proceed
    // outside of the module's scope into the builtins
    // scope.
    pub is_outside_caller_module: bool,

    // Indicates that the recursion needed to proceed
    // to a scope that is beyond the current execution
    // scope. An execution scope is defined as a function,
    // module, or lambda. Classes are not considered execution
    // scopes because they are "executed" immediately as
    // part of the scope in which they are contained.
    pub is_beyond_execution_scope: bool,

    // The symbol was accessed through a nonlocal or global binding.
    pub uses_nonlocal_binding: bool,
    pub uses_global_binding: bool,
}

// analyzer/scope.ts:71
#[derive(Clone, Copy, Debug)]
pub struct GlobalScopeResult {
    pub scope: P<Scope>,
    pub is_beyond_execution_scope: bool,
}

// analyzer/scope.ts:76
#[derive(Clone, Copy, Debug, Default)]
pub struct LookupSymbolOptions {
    pub is_outside_caller_module: Option<bool>,
    pub is_beyond_execution_scope: Option<bool>,
    pub use_proxy_scope: Option<bool>,
    pub use_chained_module_level_scopes: Option<bool>,
    pub uses_nonlocal_binding: Option<bool>,
    pub uses_global_binding: Option<bool>,
}

// analyzer/scope.ts:85
#[derive(Clone, Copy, Debug, Default)]
#[derive(jsrt::Trace)]
pub struct ChainedModuleLevelLookupContext {
    pub is_outside_caller_module: Option<bool>,
    pub is_beyond_execution_scope: Option<bool>,
    pub uses_nonlocal_binding: Option<bool>,
    pub uses_global_binding: Option<bool>,
}

// analyzer/scope.ts:92
pub type ScopeChainedModuleLevelLookup =
    Rc<dyn Fn(&str, Option<ChainedModuleLevelLookupContext>) -> Option<SymbolWithScope>>;

// analyzer/scope.ts:97
#[derive(jsrt::Trace)]
pub struct Scope {
    // The scope type, as defined in the enumeration.
    pub type_: ScopeType,

    // The next scope in the hierarchy or undefined if it's the
    // top-most scope.
    pub parent: Option<P<Scope>>,

    // An alternate parent scope that can be used to resolve symbols
    // in certain contexts. Used for TypeParam scopes.
    pub proxy: Option<P<Scope>>,

    // An optional lookup for chained module-level scopes (notebook cells)
    // consulted after the current scope is checked.
    /// Set only for notebook cells, which the CLI never checks.
    #[trace(skip)]
    pub chained_module_level_scope_lookup: Option<ScopeChainedModuleLevelLookup>,

    // Association between names and symbols.
    pub symbol_table: SymbolTable,

    // Names within this scope that are bound to other scopes
    // (either nonlocal or global).
    pub not_local_bindings: RefCell<JsMap<String, NameBindingType>>,

    // Names defined by __slots__ within this scope (used only
    // for class scopes).
    pub slots_names: RefCell<Option<Vec<String>>>,

    // Indicates that __slots__ definitely contains at least one name,
    // even if the complete set of names is unknown.
    pub has_non_empty_slots: Cell<bool>,

    // Indicates that the class body contains an assignment target that
    // cannot be represented as a statically-known class symbol or exposes
    // its namespace to runtime mutation.
    pub has_potentially_dynamic_symbol_table: Cell<bool>,

    /// This scope's own handle, for methods that return `this`.
    _self: Cell<Option<P<Scope>>>,
}

impl Scope {
    // analyzer/scope.ts:133
    pub fn new(
        type_: ScopeType,
        parent: Option<P<Scope>>,
        proxy: Option<P<Scope>>,
        chained_module_level_scope_lookup: Option<ScopeChainedModuleLevelLookup>,
    ) -> P<Scope> {
        let scope = P::new(Scope {
            type_,
            parent,
            proxy,
            chained_module_level_scope_lookup,
            symbol_table: SymbolTable::new(),
            not_local_bindings: RefCell::new(JsMap::default()),
            slots_names: RefCell::new(None),
            has_non_empty_slots: Cell::new(false),
            has_potentially_dynamic_symbol_table: Cell::new(false),
            _self: Cell::new(None),
        });
        scope._self.set(Some(scope));
        scope
    }

    /// `this` as a handle.
    pub fn handle(&self) -> P<Scope> {
        self._self.get().unwrap()
    }

    // analyzer/scope.ts:145
    pub fn get_global_scope(&self) -> GlobalScopeResult {
        let mut cur_scope = Some(self.handle());
        let mut is_beyond_execution_scope = false;

        while let Some(scope) = cur_scope {
            if scope.type_ == ScopeType::Module || scope.type_ == ScopeType::Builtin {
                return GlobalScopeResult {
                    scope,
                    is_beyond_execution_scope,
                };
            }

            if scope.type_ == ScopeType::Function {
                is_beyond_execution_scope = true;
            }

            cur_scope = scope.parent;
        }

        fail(Some("failed to find scope"));
    }

    // analyzer/scope.ts:168
    // Independently-executable scopes are those that are executed independently
    // of their parent scopes. Classes are executed in the context of their parent
    // scope, so they don't fit this category.
    pub fn is_independently_executable(&self) -> bool {
        self.type_ == ScopeType::Module || self.type_ == ScopeType::Function
    }

    // analyzer/scope.ts:172
    pub fn look_up_symbol(&self, name: &str) -> Option<P<Symbol>> {
        self.symbol_table.get(name)
    }

    // analyzer/scope.ts:176
    pub fn look_up_symbol_recursive(
        &self,
        name: &str,
        options: Option<LookupSymbolOptions>,
    ) -> Option<SymbolWithScope> {
        let options = options.unwrap_or_default();
        let mut effective_scope = self.handle();
        let mut symbol = self.symbol_table.get(name);

        if symbol.is_none() && options.use_proxy_scope == Some(true) {
            if let Some(proxy) = self.proxy {
                symbol = proxy.symbol_table.get(name);
                effective_scope = proxy;
            }
        }

        if let Some(symbol) = symbol {
            // If we're searching outside of the original caller's module (global) scope,
            // hide any names that are not meant to be visible to importers.
            if options.is_outside_caller_module == Some(true) && symbol.is_externally_hidden() {
                return None;
            }

            // If the symbol is a class variable that is defined only in terms of
            // member accesses, it is not accessible directly by name, so hide it.
            let decls = symbol.get_declarations();
            if decls.is_empty()
                || decls.iter().any(|decl| {
                    decl.type_ != DeclarationType::Variable
                        || decl
                            .variable()
                            .is_none_or(|v| v.is_defined_by_member_access.get() != Some(true))
                })
            {
                return Some(SymbolWithScope {
                    symbol,
                    is_outside_caller_module: options.is_outside_caller_module == Some(true),
                    is_beyond_execution_scope: options.is_beyond_execution_scope == Some(true),
                    scope: effective_scope,
                    uses_nonlocal_binding: options.uses_nonlocal_binding == Some(true),
                    uses_global_binding: options.uses_global_binding == Some(true),
                });
            }
        }

        let mut parent_scope: Option<P<Scope>> = None;
        let mut is_next_scope_beyond_execution_scope =
            options.is_beyond_execution_scope == Some(true) || self.is_independently_executable();

        let not_local_binding = self.not_local_bindings.borrow().get(name).copied();
        if not_local_binding == Some(NameBindingType::Global) {
            let global_scope_result = self.get_global_scope();
            if global_scope_result.scope != self.handle() {
                parent_scope = Some(global_scope_result.scope);
                if global_scope_result.is_beyond_execution_scope {
                    is_next_scope_beyond_execution_scope = true;
                }
            }
        } else {
            parent_scope = self.parent;
        }

        if options.use_chained_module_level_scopes == Some(true) {
            if let Some(lookup) = &self.chained_module_level_scope_lookup {
                let fallback_result = lookup(
                    name,
                    Some(ChainedModuleLevelLookupContext {
                        is_outside_caller_module: Some(options.is_outside_caller_module == Some(true)),
                        is_beyond_execution_scope: Some(is_next_scope_beyond_execution_scope),
                        uses_nonlocal_binding: Some(
                            not_local_binding == Some(NameBindingType::Nonlocal)
                                || options.uses_nonlocal_binding == Some(true),
                        ),
                        uses_global_binding: Some(
                            not_local_binding == Some(NameBindingType::Global)
                                || options.uses_global_binding == Some(true),
                        ),
                    }),
                );

                if fallback_result.is_some() {
                    return fallback_result;
                }
            }
        }

        if let Some(parent_scope) = parent_scope {
            // If our recursion is about to take us outside the scope of the current
            // module (i.e. into a built-in scope), indicate as such with the second
            // parameter.
            return parent_scope.look_up_symbol_recursive(
                name,
                Some(LookupSymbolOptions {
                    is_outside_caller_module: Some(
                        options.is_outside_caller_module == Some(true)
                            || self.type_ == ScopeType::Module,
                    ),
                    is_beyond_execution_scope: Some(is_next_scope_beyond_execution_scope),
                    use_proxy_scope: None,
                    use_chained_module_level_scopes: Some(
                        options.use_chained_module_level_scopes == Some(true),
                    ),
                    uses_nonlocal_binding: Some(
                        not_local_binding == Some(NameBindingType::Nonlocal)
                            || options.uses_nonlocal_binding == Some(true),
                    ),
                    uses_global_binding: Some(
                        not_local_binding == Some(NameBindingType::Global)
                            || options.uses_global_binding == Some(true),
                    ),
                }),
            );
        }

        None
    }

    // analyzer/scope.ts:255
    pub fn add_symbol(&self, name: &str, flags: SymbolFlags) -> P<Symbol> {
        let symbol = Symbol::new(flags);
        self.symbol_table.set(name, symbol);
        symbol
    }

    // analyzer/scope.ts:261
    pub fn get_binding_type(&self, name: &str) -> Option<NameBindingType> {
        self.not_local_bindings.borrow().get(name).copied()
    }

    // analyzer/scope.ts:265
    pub fn set_binding_type(&self, name: &str, binding_type: NameBindingType) {
        let mut bindings = self.not_local_bindings.borrow_mut();
        if let Some(existing) = bindings.get_mut(name) {
            *existing = binding_type;
        } else {
            bindings.insert(name.to_string(), binding_type);
        }
    }

    // analyzer/scope.ts:269
    pub fn set_slots_names(&self, names: Vec<String>) {
        *self.slots_names.borrow_mut() = Some(names);
    }

    // analyzer/scope.ts:273
    pub fn get_slots_names(&self) -> Option<Vec<String>> {
        self.slots_names.borrow().clone()
    }

    // analyzer/scope.ts:277
    pub fn set_has_non_empty_slots(&self, value: bool) {
        self.has_non_empty_slots.set(value);
    }
}
