//! Tests for `analyzer/types.rs`: the part of `tests/types.test.ts` that runs without an
//! evaluator, and checks of the shared arrays, union literal maps and module identity.

use crate::analyzer::symbol::{Symbol, SymbolFlags, SymbolTable};
use crate::analyzer::types::*;
use crate::common::uri::uri::Uri;
use crate::parser::parse_nodes::ParamCategory;

fn create_class(name: &str, flags: ClassTypeFlags) -> ClassType {
    ClassType::create_instantiable(
        name,
        name,
        "",
        &Uri::empty(),
        flags,
        0,
        /* declared_metaclass */ None,
        /* effective_metaclass */ None,
        None,
    )
}

// tests/types.test.ts: DisjointBaseDoesNotSynthesizeDataClassMethods. The deferred slots
// hold callback structs that need a TypeEvaluator, so this part sets `hasNonEmptySlots`
// directly instead of through `synthesizeDataClassSlotsDeferred`.
#[test]
fn disjoint_base_from_non_empty_slots() {
    let class_type = create_class("SlottedDataClass", ClassTypeFlags::None);
    assert_eq!(ClassType::get_disjoint_base(class_type), None);

    class_type
        .d()
        .shared
        .get()
        .has_non_empty_slots
        .set(Some(true));
    assert_eq!(ClassType::get_disjoint_base(class_type), Some(class_type));
}

#[test]
fn function_clone_shares_parameter_array() {
    let func = FunctionType::create_instance("f", "f", "", FunctionTypeFlags::None, None);
    FunctionType::add_param(
        func,
        &FunctionParam::create(
            ParamCategory::Simple,
            AnyType::create(false).0,
            FunctionParamFlags::TypeDeclared,
            Some("x"),
            None,
            None,
        ),
    );

    // `{ ...type.shared }` keeps the parameters array, so a push through the clone
    // reaches the original, as in expandTypedKwargs.
    let clone = FunctionType::clone(func, false, None);
    FunctionType::add_param(
        clone,
        &FunctionParam::create(
            ParamCategory::Simple,
            UnknownType::create(false).0,
            FunctionParamFlags::None,
            Some("y"),
            None,
            None,
        ),
    );
    assert_eq!(func.d().shared.get().parameters.get().borrow().len(), 2);
    assert_ne!(func.d().shared.get(), clone.d().shared.get());

    // Stripping the first parameter makes a new array.
    let bound = FunctionType::clone(func, true, None);
    assert_eq!(bound.d().shared.get().parameters.get().borrow().len(), 1);
    assert_eq!(func.d().shared.get().parameters.get().borrow().len(), 2);
    assert_eq!(
        bound.d().priv_.borrow().stripped_first_param_type,
        Some(AnyType::create(false).0)
    );
    assert!(is_method_type(bound.0));
    assert!(!is_method_type(func.0));
}

#[test]
fn combine_types_dedups_and_flattens() {
    let any = AnyType::create(false).0;
    let unknown = UnknownType::create(false).0;

    assert_eq!(combine_types(&[], None), NeverType::create_never().0);
    assert_eq!(combine_types(&[any, any], None), any);
    assert_eq!(
        combine_types(
            &[NeverType::create_never().0, NeverType::create_no_return().0],
            None
        ),
        NeverType::create_no_return().0
    );

    let union = combine_types(&[any, unknown], None);
    assert!(is_union(union));
    let flattened = combine_types(&[union, any], None);
    assert_eq!(
        *UnionType(flattened).d().priv_.borrow().subtypes,
        vec![any, unknown]
    );
    assert!(is_any_or_unknown(flattened));
    assert!(is_type_same(
        union,
        flattened,
        &TypeSameOptions::default(),
        0
    ));
}

// Not in types.test.ts: the union literal map keys a str literal by its UTF-16 value, so literals
// that differ only in a lone surrogate stay distinct members, as in pyright.
#[test]
fn union_keeps_literals_that_differ_in_a_lone_surrogate() {
    let str_class = create_class("str", ClassTypeFlags::BuiltIn);
    let str_instance = ClassType::clone_as_instance(str_class, true);
    let literal = |units: &[u16]| {
        ClassType::clone_with_literal(str_instance, Some(LiteralValue::String(units.into()))).0
    };

    let high = literal(&[0x61, 0xd800]);
    let low = literal(&[0x61, 0xdc00]);
    let union = combine_types(&[high, low, literal(&[0x61, 0xd800])], None);
    assert_eq!(*UnionType(union).d().priv_.borrow().subtypes, vec![high, low]);
    for member in [&[0x61, 0xd800], &[0x61, 0xdc00]] {
        assert!(UnionType::contains_type(
            UnionType(union),
            literal(member),
            &TypeSameOptions::default(),
            None,
            0
        ));
    }
    assert!(!UnionType::contains_type(
        UnionType(union),
        literal(&[0x61, 0xfffd]),
        &TypeSameOptions::default(),
        None,
        0
    ));
}

#[test]
fn union_of_str_literals_uses_literal_map() {
    let str_class = create_class("str", ClassTypeFlags::BuiltIn);
    let str_instance = ClassType::clone_as_instance(str_class, true);
    let literal = |value: &str| {
        ClassType::clone_with_literal(str_instance, Some(LiteralValue::String(value.into()))).0
    };

    let a = literal("a");
    let b = literal("b");
    let union = combine_types(&[a, b, literal("a")], None);
    assert_eq!(*UnionType(union).d().priv_.borrow().subtypes, vec![a, b]);
    assert!(UnionType::contains_type(
        UnionType(union),
        literal("b"),
        &TypeSameOptions::default(),
        None,
        0
    ));
    assert!(!UnionType::contains_type(
        UnionType(union),
        literal("c"),
        &TypeSameOptions::default(),
        None,
        0
    ));

    // A non-literal str absorbs its literals when it comes first.
    let widened = combine_types(&[str_instance.0, a, b], None);
    assert_eq!(widened, str_instance.0);
}

#[test]
fn module_types_compare_by_symbol_table_identity() {
    let table = SymbolTable::new();
    table.set("x", Symbol::new(SymbolFlags::None));
    let module1 = ModuleType::create("m", &Uri::empty(), Some(table));
    let module2 = ModuleType::create("m", &Uri::empty(), Some(table));
    let empty1 = ModuleType::create("m", &Uri::empty(), None);
    let empty2 = ModuleType::create("m", &Uri::empty(), None);
    let options = TypeSameOptions::default();

    assert!(is_type_same(module1.0, module2.0, &options, 0));
    assert!(!is_type_same(module1.0, empty1.0, &options, 0));
    assert!(is_type_same(empty1.0, empty2.0, &options, 0));
}

#[test]
fn type_var_scope_and_bound_forms() {
    let type_var = TypeVarType::clone_for_scope_id(
        TypeVarType::create_instance("T", TypeVarKind::TypeVar),
        "scope".to_string(),
        Some("f".to_string()),
        Some(TypeVarScopeType::Function),
    );
    assert_eq!(&*TypeVarType::get_name_with_scope(type_var), "T.scope.f");
    assert_eq!(TypeVarType::get_readable_name(type_var, true), "T@f");

    let bound = TypeVarType::clone_as_bound(type_var);
    assert!(TypeVarType::is_bound(bound));
    assert_eq!(bound.d().priv_.borrow().scope_id.as_deref(), Some("scope*"));
    assert_eq!(TypeVarType::clone_as_bound(bound), bound);
    assert!(!is_type_same(
        type_var.0,
        bound.0,
        &TypeSameOptions::default(),
        0
    ));

    let unification = TypeVarType::clone_as_unification_var(type_var, Some(7));
    assert_eq!(
        unification.d().priv_.borrow().name_with_scope.as_deref(),
        Some("T.scope.f-7")
    );
    assert_eq!(
        unification.d().priv_.borrow().scope_id.as_deref(),
        Some(UNIFICATION_SCOPE_ID)
    );
}
