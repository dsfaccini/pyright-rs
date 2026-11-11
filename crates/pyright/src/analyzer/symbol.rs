//! Port of `packages/pyright-internal/src/analyzer/symbol.ts`.

use jsrt::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use jsrt::{Atom, AtomKey, JsMap, OptionRefCell, P};

use jsrt::flag_ops;
use super::declaration::{Declaration, DeclarationType};
use super::declaration_utils::{are_declarations_same, has_type_for_declaration};
use super::types::Type;
use crate::parser::parse_nodes::NameNode;


thread_local! {
    /// `get_declarations` of a symbol without declarations: TypeScript's `[]`. Always empty, so
    /// it holds no handle and needs no collector root.
    static NO_DECLARATIONS: Rc<Vec<P<Declaration>>> = Rc::new(Vec::new());
}

// analyzer/symbol.ts:17
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[derive(jsrt::Trace)]
pub struct SymbolFlags(pub u32);

impl SymbolFlags {
    pub const None: SymbolFlags = SymbolFlags(0);

    // Indicates that the symbol is unbound at the start of
    // execution. Some symbols are initialized by the module
    // loader, so they are bound even before the first statement
    // in the module is executed.
    pub const InitiallyUnbound: SymbolFlags = SymbolFlags(1 << 0);

    // Indicates that the symbol is not visible from other files.
    // Used for module-level symbols.
    pub const ExternallyHidden: SymbolFlags = SymbolFlags(1 << 1);

    // Indicates that the symbol is a class member of a class.
    pub const ClassMember: SymbolFlags = SymbolFlags(1 << 2);

    // Indicates that the symbol is an instance member of a class.
    pub const InstanceMember: SymbolFlags = SymbolFlags(1 << 3);

    // Indicates that the symbol is specified in the __slots__
    // declaration of a class. Such symbols act like instance members
    // in some respects but are actually implemented as class members
    // using descriptor objects.
    pub const SlotsMember: SymbolFlags = SymbolFlags(1 << 4);

    // Indicates that the symbol is considered "private" to the
    // class or module and should not be accessed outside or overridden.
    pub const PrivateMember: SymbolFlags = SymbolFlags(1 << 5);

    // Indicates that the symbol is not considered for protocol
    // matching. This applies to some built-in symbols like __module__.
    pub const IgnoredForProtocolMatch: SymbolFlags = SymbolFlags(1 << 6);

    // Indicates that the symbol is a ClassVar, so it cannot be
    // set when accessed through a class instance.
    pub const ClassVar: SymbolFlags = SymbolFlags(1 << 7);

    // Indicates that the symbol is in __all__.
    pub const InDunderAll: SymbolFlags = SymbolFlags(1 << 8);

    // Indicates that the symbol is a private import in a py.typed module.
    pub const PrivatePyTypedImport: SymbolFlags = SymbolFlags(1 << 9);

    // Indicates that the symbol is an InitVar as specified in PEP 557.
    pub const InitVar: SymbolFlags = SymbolFlags(1 << 10);

    // Indicates that the symbol is a field in a NamedTuple class, which
    // is modeled as an instance member but in some respects acts as a
    // class member.
    pub const NamedTupleMember: SymbolFlags = SymbolFlags(1 << 11);

    // Indicates that the symbol should be exempt from override type checks.
    pub const IgnoredForOverrideChecks: SymbolFlags = SymbolFlags(1 << 12);

    // Indicates that the symbol is marked Final and is assigned a value
    // in the class body. The typing spec indicates that these should be
    // considered ClassVars unless they are found in a dataclass.
    pub const FinalVarInClassBody: SymbolFlags = SymbolFlags(1 << 13);

    // Indicates that the symbol is a KW_ONLY separator in a dataclass.
    pub const DataClassKeywordOnly: SymbolFlags = SymbolFlags(1 << 14);
}

flag_ops!(SymbolFlags);

thread_local! {
    // analyzer/symbol.ts:80
    static NEXT_SYMBOL_ID: Cell<u32> = const { Cell::new(1) };
}

// analyzer/symbol.ts:81
fn get_unique_symbol_id() -> u32 {
    NEXT_SYMBOL_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

// analyzer/symbol.ts:86
// Symbol ID that indicates that there is no specific symbol.
pub const INDETERMINATE_SYMBOL_ID: u32 = 0;

// analyzer/symbol.ts:113
#[derive(Clone, Copy)]
#[derive(jsrt::Trace)]
pub struct SynthesizedTypeInfo {
    pub type_: P<Type>,

    // An optional node that is not used by the type evaluator
    // but can be used by language services to provide additional
    // functionality (such as go-to-definition).
    pub node: Option<NameNode>,
}

// analyzer/symbol.ts:115
/// The private fields keep their leading underscore; the oracle prints them.
#[derive(jsrt::Trace)]
pub struct Symbol {
    // Information about the node that declared the value -
    // i.e. where the editor will take the user if "show definition"
    // is selected. Multiple declarations can exist for variables,
    // properties, and functions (in the case of @overload).
    /// Shared with the callers of `get_declarations`, which TypeScript hands the live array.
    pub _declarations: RefCell<Option<Rc<Vec<P<Declaration>>>>>,

    // Flags that provide information about the symbol.
    pub _flags: Cell<SymbolFlags>,

    // Unique numeric ID for each symbol allocated.
    pub id: u32,

    // Symbols that are completely synthesized (i.e. have no
    // corresponding declarations in the program) can have
    // a specified type.
    /// Allocated on the first write: few symbols are synthesized.
    pub _synthesized_type_info: OptionRefCell<SynthesizedTypeInfo>,

    // Is this symbol an alias for a symbol originally imported from
    // the typing or typing_extensions module (e.g. "Final")?
    /// Allocated on the first write: few symbols alias a typing name.
    pub _typing_symbol_alias: OptionRefCell<String>,
}
const _: () = assert!(size_of::<Symbol>() <= 40);

impl Symbol {
    // analyzer/symbol.ts:119
    pub fn new(flags: SymbolFlags) -> P<Symbol> {
        P::new(Symbol {
            _declarations: RefCell::new(None),
            _flags: Cell::new(flags),
            id: get_unique_symbol_id(),
            _synthesized_type_info: OptionRefCell::new(),
            _typing_symbol_alias: OptionRefCell::new(),
        })
    }

    // analyzer/symbol.ts:124
    pub fn create_with_type(flags: SymbolFlags, type_: P<Type>, node: Option<NameNode>) -> P<Symbol> {
        let new_symbol = Symbol::new(flags);
        *new_symbol._synthesized_type_info.borrow_mut() = Some(SynthesizedTypeInfo { type_, node });
        new_symbol
    }

    fn has_flag(&self, flag: SymbolFlags) -> bool {
        self._flags.get().any(flag)
    }

    fn add_flags(&self, flags: SymbolFlags) {
        self._flags.set(self._flags.get() | flags);
    }

    // analyzer/symbol.ts:130
    pub fn is_initially_unbound(&self) -> bool {
        self.has_flag(SymbolFlags::InitiallyUnbound)
    }

    // analyzer/symbol.ts:134
    pub fn set_is_externally_hidden(&self) {
        self.add_flags(SymbolFlags::ExternallyHidden);
    }

    // analyzer/symbol.ts:134
    pub fn is_externally_hidden(&self) -> bool {
        self.has_flag(SymbolFlags::ExternallyHidden)
    }

    // analyzer/symbol.ts:142
    pub fn set_is_ignored_for_protocol_match(&self) {
        self.add_flags(SymbolFlags::IgnoredForProtocolMatch);
    }

    // analyzer/symbol.ts:142
    pub fn is_ignored_for_protocol_match(&self) -> bool {
        self.has_flag(SymbolFlags::IgnoredForProtocolMatch)
    }

    // analyzer/symbol.ts:150
    pub fn set_is_class_member(&self) {
        self.add_flags(SymbolFlags::ClassMember);
    }

    // analyzer/symbol.ts:150
    pub fn is_class_member(&self) -> bool {
        self.has_flag(SymbolFlags::ClassMember)
    }

    // analyzer/symbol.ts:158
    pub fn set_is_instance_member(&self) {
        self.add_flags(SymbolFlags::InstanceMember);
    }

    // analyzer/symbol.ts:158
    pub fn is_instance_member(&self) -> bool {
        self.has_flag(SymbolFlags::InstanceMember)
    }

    // analyzer/symbol.ts:166
    pub fn set_is_slots_member(&self) {
        self.add_flags(
            SymbolFlags::ClassMember | SymbolFlags::InstanceMember | SymbolFlags::SlotsMember,
        );
    }

    // analyzer/symbol.ts:166
    pub fn is_slots_member(&self) -> bool {
        self.has_flag(SymbolFlags::SlotsMember)
    }

    // analyzer/symbol.ts:174
    pub fn set_is_class_var(&self) {
        self.add_flags(SymbolFlags::ClassVar);
    }

    // analyzer/symbol.ts:174
    pub fn is_class_var(&self) -> bool {
        self.has_flag(SymbolFlags::ClassVar)
    }

    // analyzer/symbol.ts:182
    pub fn set_is_final_var_in_class_body(&self) {
        self.add_flags(SymbolFlags::FinalVarInClassBody);
    }

    // analyzer/symbol.ts:182
    pub fn is_final_var_in_class_body(&self) -> bool {
        self.has_flag(SymbolFlags::FinalVarInClassBody)
    }

    // analyzer/symbol.ts:190
    pub fn set_is_init_var(&self) {
        self.add_flags(SymbolFlags::InitVar);
    }

    // analyzer/symbol.ts:190
    pub fn is_init_var(&self) -> bool {
        self.has_flag(SymbolFlags::InitVar)
    }

    // analyzer/symbol.ts:198
    pub fn set_is_data_class_keyword_only(&self) {
        self.add_flags(SymbolFlags::DataClassKeywordOnly);
    }

    // analyzer/symbol.ts:198
    pub fn is_data_class_keyword_only(&self) -> bool {
        self.has_flag(SymbolFlags::DataClassKeywordOnly)
    }

    // analyzer/symbol.ts:206
    pub fn set_is_in_dunder_all(&self) {
        self.add_flags(SymbolFlags::InDunderAll);
    }

    // analyzer/symbol.ts:206
    pub fn is_in_dunder_all(&self) -> bool {
        self.has_flag(SymbolFlags::InDunderAll)
    }

    // analyzer/symbol.ts:214
    pub fn set_is_private_member(&self) {
        self.add_flags(SymbolFlags::PrivateMember);
    }

    // analyzer/symbol.ts:214
    pub fn is_private_member(&self) -> bool {
        self.has_flag(SymbolFlags::PrivateMember)
    }

    // analyzer/symbol.ts:222
    pub fn set_private_py_typed_import(&self) {
        self.add_flags(SymbolFlags::PrivatePyTypedImport);
    }

    // analyzer/symbol.ts:226
    pub fn is_private_py_typed_import(&self) -> bool {
        self.has_flag(SymbolFlags::PrivatePyTypedImport)
    }

    // analyzer/symbol.ts:230
    pub fn is_named_tuple_member_member(&self) -> bool {
        self.has_flag(SymbolFlags::NamedTupleMember)
    }

    // analyzer/symbol.ts:234
    pub fn is_ignored_for_override_checks(&self) -> bool {
        self.has_flag(SymbolFlags::IgnoredForOverrideChecks)
    }

    // analyzer/symbol.ts:238
    pub fn set_typing_symbol_alias(&self, aliased_name: &str) {
        *self._typing_symbol_alias.borrow_mut() = Some(aliased_name.to_string());
    }

    // analyzer/symbol.ts:242
    pub fn get_typing_symbol_alias(&self) -> Option<String> {
        self._typing_symbol_alias.borrow().clone()
    }

    // analyzer/symbol.ts:246
    pub fn add_declaration(&self, declaration: P<Declaration>) {
        let declarations = self._declarations.borrow().clone();
        if let Some(declarations) = declarations {
            // See if this node was already identified as a declaration. If so,
            // replace it. Otherwise, add it as a new declaration to the end of
            // the list.
            let decl_index = declarations
                .iter()
                .position(|decl| are_declarations_same(decl, &declaration, false, false));
            match decl_index {
                None => {
                    let mut declarations = declarations.to_vec();
                    declarations.push(declaration);

                    // If there is more than one declaration for a symbol, we will
                    // assume it is not a type alias.
                    for decl in &declarations {
                        if let Some(var) = decl.variable() {
                            if var.type_alias_name.get().is_some() {
                                var.type_alias_name.set(None);
                            }
                        }
                    }
                    *self._declarations.borrow_mut() = Some(Rc::new(declarations));
                }
                Some(decl_index) => {
                    // If the new declaration has a defined type, it should replace
                    // the existing one.
                    let cur_decl = declarations[decl_index];
                    if has_type_for_declaration(&declaration) {
                        Rc::make_mut(self._declarations.borrow_mut().as_mut().unwrap())[decl_index] = declaration;
                        if cur_decl.type_ == DeclarationType::Variable
                            && declaration.type_ == DeclarationType::Variable
                        {
                            let new_var = declaration.variable().unwrap();
                            let cur_var = cur_decl.variable().unwrap();
                            if new_var.inferred_type_source.get().is_none()
                                && cur_var.inferred_type_source.get().is_some()
                            {
                                new_var
                                    .inferred_type_source
                                    .set(cur_var.inferred_type_source.get());
                            }
                        }
                    } else if declaration.type_ == DeclarationType::Variable {
                        // If it's marked "final" or "type alias", this should be reflected
                        // in the existing declaration. Likewise, if the existing declaration
                        // doesn't have a type source, add it.
                        if let Some(cur_var) = cur_decl.variable() {
                            let new_var = declaration.variable().unwrap();
                            if new_var.is_final.get() == Some(true) {
                                cur_var.is_final.set(Some(true));
                            }

                            cur_var.type_alias_name.set(new_var.type_alias_name.get());

                            if cur_var.inferred_type_source.get().is_none()
                                && new_var.inferred_type_source.get().is_some()
                            {
                                cur_var
                                    .inferred_type_source
                                    .set(new_var.inferred_type_source.get());
                            }
                        }
                    }
                }
            }
        } else {
            *self._declarations.borrow_mut() = Some(Rc::new(vec![declaration]));
        }
    }

    // analyzer/symbol.ts:295
    pub fn has_declarations(&self) -> bool {
        self._declarations
            .borrow()
            .as_ref()
            .is_some_and(|decls| !decls.is_empty())
    }

    // analyzer/symbol.ts:299
    pub fn get_declarations(&self) -> Rc<Vec<P<Declaration>>> {
        match &*self._declarations.borrow() {
            Some(declarations) => declarations.clone(),
            None => NO_DECLARATIONS.with(Rc::clone),
        }
    }

    // analyzer/symbol.ts:303
    pub fn has_typed_declarations(&self) -> bool {
        // We'll treat an synthesized type as an implicit declaration.
        if self._synthesized_type_info.borrow().is_some() {
            return true;
        }

        self.get_declarations()
            .iter()
            .any(|decl| has_type_for_declaration(decl))
    }

    // analyzer/symbol.ts:312
    pub fn get_typed_declarations(&self) -> Vec<P<Declaration>> {
        self.get_declarations()
            .iter()
            .copied()
            .filter(|decl| has_type_for_declaration(decl))
            .collect()
    }

    // analyzer/symbol.ts:316
    pub fn get_synthesized_type(&self) -> Option<SynthesizedTypeInfo> {
        *self._synthesized_type_info.borrow()
    }
}

// analyzer/symbol.ts:322
// Maps names to symbols information.
/// A shared `Map<string, Symbol>`: copies of the handle are the same map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub struct SymbolTable(P<RefCell<JsMap<Atom, P<Symbol>>>>);

/// Symbol names are atoms (`docs/porting.md`, "strings"): a lookup by `&str` finds the
/// text's atom first, and text no atom has is in no table.
impl SymbolTable {
    /// `new Map<string, Symbol>()`.
    pub fn new() -> SymbolTable {
        SymbolTable(P::new(RefCell::new(JsMap::default())))
    }

    pub fn get(&self, name: impl AtomKey) -> Option<P<Symbol>> {
        let name = name.lookup()?;
        self.0.borrow().get(&name).copied()
    }

    pub fn has(&self, name: impl AtomKey) -> bool {
        name.lookup().is_some_and(|name| self.0.borrow().contains_key(&name))
    }

    pub fn set(&self, name: impl AtomKey, symbol: P<Symbol>) {
        self.0.borrow_mut().insert(name.intern(), symbol);
    }

    pub fn delete(&self, name: impl AtomKey) -> bool {
        name.lookup().is_some_and(|name| self.0.borrow_mut().shift_remove(&name).is_some())
    }

    pub fn size(&self) -> usize {
        self.0.borrow().len()
    }

    /// Read access for code that does not call back into the table.
    pub fn borrow(&self) -> Ref<'_, JsMap<Atom, P<Symbol>>> {
        self.0.borrow()
    }

    /// `forEach`, as JavaScript iterates a `Map`: entries added during the
    /// walk are visited too, and the callback may write to the table.
    pub fn for_each(&self, mut callback: impl FnMut(P<Symbol>, Atom)) {
        let mut index = 0;
        loop {
            let Some((name, symbol)) = self.0.borrow().get_index(index).map(|(name, symbol)| (*name, *symbol)) else {
                break;
            };
            callback(symbol, name);
            index += 1;
        }
    }

    /// The entries in insertion order, copied out.
    pub fn entries(&self) -> Vec<(Atom, P<Symbol>)> {
        self.0
            .borrow()
            .iter()
            .map(|(name, symbol)| (*name, *symbol))
            .collect()
    }
}
