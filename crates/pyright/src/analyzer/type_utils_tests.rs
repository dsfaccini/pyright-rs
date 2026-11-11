//! Port of `packages/pyright-internal/src/tests/typeUtils.test.ts`.

use crate::analyzer::type_utils::transform_type_pair;
use std::rc::Rc;
use crate::analyzer::types::{
    AnyType, ClassType, ClassTypeFlags, FunctionParam, FunctionParamFlags, FunctionType,
    FunctionTypeFlags, TupleTypeArg, Type, TypeVarKind, TypeVarScopeType, TypeVarType, UnionType,
    UnknownType, Variance, is_class, is_function, is_type_var,
};
use crate::common::uri::Uri;
use crate::parser::parse_nodes::ParamCategory;
use jsrt::P;

#[test]
fn transform_aligned_function_types() {
    let source_leaf = UnknownType::create(false).0;
    let target_leaf = AnyType::create(false).0;
    let source_function = create_function(source_leaf, source_leaf, source_leaf);
    let target_function = create_function(target_leaf, target_leaf, target_leaf);

    let result = transform_type_pair(
        source_function.0,
        target_function.0,
        &mut replace_pair(source_leaf, target_leaf),
    );

    assert!(is_function(result));
    assert_ne!(result, source_function.0);
    let result = FunctionType(result);
    assert_eq!(FunctionType::get_param_type(result, 0), target_leaf);
    assert_eq!(
        FunctionType::get_param_default_type(result, 0),
        Some(target_leaf)
    );
    assert_eq!(
        FunctionType::get_effective_return_type(result, true),
        Some(target_leaf)
    );
    assert_eq!(
        FunctionType::get_param_type(source_function, 0),
        source_leaf
    );
    assert_eq!(
        FunctionType::get_param_default_type(source_function, 0),
        Some(source_leaf)
    );
    assert_eq!(
        FunctionType::get_effective_return_type(source_function, true),
        Some(source_leaf)
    );
}

#[test]
fn transform_type_variable_metadata_without_changing_its_identity() {
    let source_leaf = UnknownType::create(false).0;
    let target_leaf = AnyType::create(false).0;
    let source_type_var = TypeVarType::clone_for_scope_id(
        TypeVarType::create_instance("T", TypeVarKind::TypeVar),
        "source-scope".to_string(),
        Some("source".to_string()),
        Some(TypeVarScopeType::Class),
    );
    let source_shared = source_type_var.d().shared.get();
    *source_shared.constraints.borrow_mut() = vec![source_leaf];
    source_shared.bound_type.set(Some(source_leaf));
    source_shared.default_type.set(source_leaf);
    source_shared.declared_variance.set(Variance::Covariant);

    let target_type_var = TypeVarType::create_instance("U", TypeVarKind::TypeVar);
    let target_shared = target_type_var.d().shared.get();
    *target_shared.constraints.borrow_mut() = vec![target_leaf];
    target_shared.bound_type.set(Some(target_leaf));
    target_shared.default_type.set(target_leaf);

    let result = transform_type_pair(
        source_type_var.0,
        target_type_var.0,
        &mut replace_pair(source_leaf, target_leaf),
    );

    assert!(is_type_var(result));
    assert_ne!(result, source_type_var.0);
    let result = TypeVarType(result);
    let result_shared = result.d().shared.get();
    assert_eq!(*result_shared.name.borrow(), "T");
    assert_eq!(result_shared.declared_variance.get(), Variance::Covariant);
    assert_eq!(
        result.d().priv_.borrow().scope_id,
        source_type_var.d().priv_.borrow().scope_id
    );
    assert_eq!(*result_shared.constraints.borrow(), vec![target_leaf]);
    assert_eq!(result_shared.bound_type.get(), Some(target_leaf));
    assert_eq!(result_shared.default_type.get(), target_leaf);

    let recursive_source = TypeVarType::create_instance("RecursiveSource", TypeVarKind::TypeVar);
    let recursive_target = TypeVarType::create_instance("RecursiveTarget", TypeVarKind::TypeVar);
    recursive_source
        .d()
        .shared
        .get()
        .bound_type
        .set(Some(recursive_source.0));
    recursive_target
        .d()
        .shared
        .get()
        .bound_type
        .set(Some(recursive_target.0));
    assert_eq!(
        transform_type_pair(recursive_source.0, recursive_target.0, &mut |_, _| None),
        recursive_source.0
    );
}

#[test]
fn transform_aligned_class_arguments_but_preserve_mismatched_wrappers() {
    let source_leaf = UnknownType::create(false).0;
    let target_leaf = AnyType::create(false).0;
    let source_other = TypeVarType::create_instance("T", TypeVarKind::TypeVar).0;
    let target_other = TypeVarType::create_instance("T", TypeVarKind::TypeVar).0;
    let source_union = UnionType::create();
    source_union.d().priv_.borrow_mut().subtypes = Rc::new(vec![source_leaf, source_other]);
    let target_union = UnionType::create();
    target_union.d().priv_.borrow_mut().subtypes = Rc::new(vec![target_leaf, target_other]);

    let wrapper = ClassType::clone_as_instance(create_class("Wrapper"), true);
    let source_class = ClassType::specialize(
        wrapper,
        Some(&[source_union.0]),
        /* isTypeArgExplicit */ Some(true),
        /* includeSubclasses */ false,
        Some(&[TupleTypeArg {
            type_: source_leaf,
            is_unbounded: true,
            is_optional: Some(true),
        }]),
        None,
    );
    let target_class = ClassType::specialize(
        wrapper,
        Some(&[target_union.0]),
        /* isTypeArgExplicit */ Some(true),
        /* includeSubclasses */ false,
        Some(&[TupleTypeArg {
            type_: target_leaf,
            is_unbounded: false,
            is_optional: Some(false),
        }]),
        None,
    );

    let result = transform_type_pair(
        source_class.0,
        target_class.0,
        &mut replace_pair(source_leaf, target_leaf),
    );

    assert!(is_class(result));
    assert_ne!(result, source_class.0);
    let result_priv = ClassType(result).d().priv_.borrow().clone();
    let result_type_arg = result_priv.type_args.as_ref().unwrap()[0];
    assert_eq!(result_type_arg.category, source_union.category);
    assert_eq!(
        UnionType(result_type_arg).d().priv_.borrow().subtypes[0],
        target_leaf
    );
    let result_tuple_type_args = result_priv.tuple_type_args.unwrap().get().borrow();
    let result_tuple_type_arg = &result_tuple_type_args[0];
    assert_eq!(result_tuple_type_arg.type_, target_leaf);
    assert!(result_tuple_type_arg.is_unbounded);
    assert_eq!(result_tuple_type_arg.is_optional, Some(true));

    let mismatched_target = ClassType::specialize(
        ClassType::clone_as_instance(create_class("Other"), true),
        Some(&[target_leaf]),
        None,
        false,
        None,
        None,
    );
    assert_eq!(
        transform_type_pair(
            source_class.0,
            mismatched_target.0,
            &mut replace_pair(source_leaf, target_leaf)
        ),
        source_class.0
    );

    let short_target_union = UnionType::create();
    short_target_union.d().priv_.borrow_mut().subtypes = Rc::new(vec![target_leaf]);
    assert_eq!(
        transform_type_pair(
            source_union.0,
            short_target_union.0,
            &mut replace_pair(source_leaf, target_leaf)
        ),
        source_union.0
    );
}

fn create_function(
    parameter_type: P<Type>,
    default_type: P<Type>,
    return_type: P<Type>,
) -> FunctionType {
    let function_type = FunctionType::create_instance("", "", "", FunctionTypeFlags::None, None);
    FunctionType::add_param(
        function_type,
        &FunctionParam::create(
            ParamCategory::Simple,
            parameter_type,
            FunctionParamFlags::None,
            Some("value"),
            Some(default_type),
            None,
        ),
    );
    function_type
        .d()
        .shared
        .get()
        .declared_return_type
        .set(Some(return_type));
    function_type
}

fn create_class(name: &str) -> ClassType {
    ClassType::create_instantiable(
        name,
        "test",
        &format!("test.{name}"),
        &Uri::empty(),
        ClassTypeFlags::None,
        0,
        /* declaredMetaclass */ None,
        /* effectiveMetaclass */ None,
        None,
    )
}

fn replace_pair(
    source: P<Type>,
    target: P<Type>,
) -> impl FnMut(P<Type>, P<Type>) -> Option<P<Type>> {
    move |source_node, target_node| {
        if source_node == source && target_node == target {
            Some(target_node)
        } else {
            None
        }
    }
}
