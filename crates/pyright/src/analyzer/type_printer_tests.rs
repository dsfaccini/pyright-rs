//! Port of `packages/pyright-internal/src/tests/typePrinter.test.ts`.
//!
//! The TypeScript tests pass their own return-type callback:
//! `type.shared.declaredReturnType ?? UnknownType.create(true)`. The Rust
//! `FunctionReturnTypeCallback` has one form, the evaluator's
//! `getEffectiveReturnType`, so these tests build an evaluator for it. It
//! returns the declared return type where the test functions have one. Where
//! they have none, inference for a function without a declaration gives
//! `Unknown`, which prints as the TypeScript callback's result does.

use std::rc::Rc;

use crate::analyzer::analyzer_node_info::{AnalyzerNodeInfoReader, AnalyzerNodeInfoStore};
use crate::analyzer::type_evaluator::{EvaluatorOptions, create_type_evaluator};
use crate::analyzer::type_printer::{FunctionReturnTypeCallback, PrintTypeFlags, print_type};
use crate::analyzer::types::{
    AnyType, ClassType, ClassTypeFlags, FunctionParam, FunctionParamFlags, FunctionType,
    FunctionTypeFlags, ModuleType, NeverType, ParamSpecType, TypeVarKind, TypeVarType,
    UnboundType, UnknownType, combine_types,
};
use crate::common::uri::Uri;
use crate::parser::parse_nodes::ParamCategory;

fn return_type_callback() -> FunctionReturnTypeCallback {
    let node_info_reader = AnalyzerNodeInfoStore::new().to_dyn::<dyn AnalyzerNodeInfoReader>(|store| store);
    FunctionReturnTypeCallback {
        type_evaluator: create_type_evaluator(
            Rc::new(|_, _| None),
            EvaluatorOptions {
                print_type_flags: PrintTypeFlags::None,
                log_calls: false,
                minimum_logging_threshold: 0.0,
                evaluate_unknown_imports_as_any: false,
                verify_type_cache_evaluator_flags: false,
                node_info_reader,
            },
        ),
    }
}

fn create_class(name: &str, full_name: &str, module_name: &str, flags: ClassTypeFlags) -> ClassType {
    ClassType::create_instantiable(
        name,
        full_name,
        module_name,
        &Uri::empty(),
        flags,
        0,
        /* declaredMetaclass */ None,
        /* effectiveMetaclass */ None,
        None,
    )
}

fn simple_param(type_: ClassType, name: &str) -> FunctionParam {
    FunctionParam::create(ParamCategory::Simple, type_.0, FunctionParamFlags::TypeDeclared, Some(name), None, None)
}

// tests/typePrinter.test.ts:38
#[test]
fn simple_types() {
    let callback = return_type_callback();

    let any_type = AnyType::create(/* isEllipsis */ false);
    assert_eq!(print_type(any_type.0, PrintTypeFlags::None, &callback), "Any");

    let ellipsis_type = AnyType::create(/* isEllipsis */ true);
    assert_eq!(print_type(ellipsis_type.0, PrintTypeFlags::None, &callback), "...");

    let unknown_type = UnknownType::create(false);
    assert_eq!(print_type(unknown_type.0, PrintTypeFlags::None, &callback), "Unknown");
    assert_eq!(print_type(unknown_type.0, PrintTypeFlags::PrintUnknownWithAny, &callback), "Any");
    assert_eq!(print_type(unknown_type.0, PrintTypeFlags::PythonSyntax, &callback), "Any");

    let unbound_type = UnboundType::create();
    assert_eq!(print_type(unbound_type.0, PrintTypeFlags::None, &callback), "Unbound");
    assert_eq!(print_type(unbound_type.0, PrintTypeFlags::PythonSyntax, &callback), "Any");

    let module_type = ModuleType::create("Test", &Uri::empty(), None);
    assert_eq!(print_type(module_type.0, PrintTypeFlags::None, &callback), "Module(\"Test\")");
    assert_eq!(print_type(module_type.0, PrintTypeFlags::PythonSyntax, &callback), "Any");
}

// tests/typePrinter.test.ts:59
#[test]
fn type_var_types() {
    let callback = return_type_callback();

    let type_var_type = TypeVarType::create_instance("T", TypeVarKind::TypeVar);
    assert_eq!(print_type(type_var_type.0, PrintTypeFlags::None, &callback), "T");

    let param_spec_type = TypeVarType::create_instance("P", TypeVarKind::ParamSpec);
    assert_eq!(print_type(param_spec_type.0, PrintTypeFlags::None, &callback), "P");

    let type_var_tuple_type = TypeVarType::create_instance("Ts", TypeVarKind::TypeVarTuple);
    assert_eq!(print_type(type_var_tuple_type.0, PrintTypeFlags::None, &callback), "Ts");
}

// tests/typePrinter.test.ts:70
#[test]
fn class_types() {
    let callback = return_type_callback();

    let class_type_a = create_class("A", "", "", ClassTypeFlags::None);

    let type_var_s = TypeVarType::create_instance("S", TypeVarKind::TypeVar);
    let type_var_t = TypeVarType::create_instance("T", TypeVarKind::TypeVar);

    class_type_a.d().shared.get().type_params.borrow_mut().extend([type_var_s, type_var_t]);

    assert_eq!(print_type(class_type_a.0, PrintTypeFlags::None, &callback), "type[A[S, T]]");

    let instance_a = ClassType::clone_as_instance(class_type_a, true);
    assert_eq!(print_type(instance_a.0, PrintTypeFlags::None, &callback), "A[S, T]");

    let class_type_int = create_class("int", "", "", ClassTypeFlags::None);
    let instance_int = ClassType::clone_as_instance(class_type_int, true);

    let specialized_a = ClassType::specialize(instance_a, Some(&[instance_int.0, instance_int.0]), None, false, None, None);

    assert_eq!(print_type(specialized_a.0, PrintTypeFlags::None, &callback), "A[int, int]");

    let union_type = combine_types(&[instance_int.0, specialized_a.0, type_var_s.0], None);
    assert_eq!(print_type(union_type, PrintTypeFlags::None, &callback), "Union[int, A[int, int], S]");
    assert_eq!(print_type(union_type, PrintTypeFlags::PEP604, &callback), "int | A[int, int] | S");
}

// tests/typePrinter.test.ts:113
#[test]
fn class_types_python_syntax_omits_type_args_for_stub_only_generics() {
    let callback = return_type_callback();

    // Simulate operator.attrgetter — defined in a stub, not built-in, no __class_getitem__.
    let attrgetter = create_class("attrgetter", "operator", "operator.attrgetter", ClassTypeFlags::DefinedInStub);

    let type_var_t = TypeVarType::create_instance("_T", TypeVarKind::TypeVar);
    attrgetter.d().shared.get().type_params.borrow_mut().push(type_var_t);

    let class_type_str = create_class("str", "builtins", "builtins.str", ClassTypeFlags::BuiltIn);
    let instance_str = ClassType::clone_as_instance(class_type_str, true);

    let instance_attrgetter = ClassType::clone_as_instance(attrgetter, true);
    let specialized = ClassType::specialize(instance_attrgetter, Some(&[instance_str.0]), None, false, None, None);

    // Without PythonSyntax, type args are shown normally.
    assert_eq!(print_type(specialized.0, PrintTypeFlags::None, &callback), "attrgetter[str]");

    // With PythonSyntax, type args are omitted because the class is not runtime-subscriptable.
    assert_eq!(print_type(specialized.0, PrintTypeFlags::PythonSyntax, &callback), "attrgetter");

    // Built-in classes that are also defined in stubs should still show type args in PythonSyntax.
    let list_class = create_class(
        "list",
        "builtins",
        "builtins.list",
        ClassTypeFlags::BuiltIn | ClassTypeFlags::DefinedInStub,
    );
    let type_var_e = TypeVarType::create_instance("_E", TypeVarKind::TypeVar);
    list_class.d().shared.get().type_params.borrow_mut().push(type_var_e);

    let instance_list = ClassType::clone_as_instance(list_class, true);
    let specialized_list = ClassType::specialize(instance_list, Some(&[instance_str.0]), None, false, None, None);

    assert_eq!(print_type(specialized_list.0, PrintTypeFlags::PythonSyntax, &callback), "list[str]");
}

// tests/typePrinter.test.ts:170
#[test]
fn class_types_python_syntax_formats_param_spec_specialized_generic_arguments() {
    let callback = return_type_callback();

    let class_type_a = create_class("A", "", "", ClassTypeFlags::None);

    let param_spec_p = TypeVarType::create_instance("P", TypeVarKind::ParamSpec);
    let type_var_r = TypeVarType::create_instance("R", TypeVarKind::TypeVar);
    class_type_a.d().shared.get().type_params.borrow_mut().extend([param_spec_p, type_var_r]);

    let instance_a = ClassType::clone_as_instance(class_type_a, true);

    let class_type_int = create_class("int", "", "", ClassTypeFlags::None);
    let instance_int = ClassType::clone_as_instance(class_type_int, true);

    let class_type_str = create_class("str", "", "", ClassTypeFlags::None);
    let instance_str = ClassType::clone_as_instance(class_type_str, true);

    let specialize_a = |param_spec_value: FunctionType| {
        ClassType::specialize(instance_a, Some(&[param_spec_value.0, instance_str.0]), None, false, None, None).0
    };

    let simple_param_spec_value = FunctionType::create_instance("", "", "", FunctionTypeFlags::ParamSpecValue, None);
    FunctionType::add_param(simple_param_spec_value, &simple_param(instance_int, "a"));

    let specialized_a = specialize_a(simple_param_spec_value);
    assert_eq!(print_type(specialized_a, PrintTypeFlags::None, &callback), "A[(a: int), str]");
    assert_eq!(print_type(specialized_a, PrintTypeFlags::PythonSyntax, &callback), "A[[int], str]");

    let multi_param_spec_value = FunctionType::create_instance("", "", "", FunctionTypeFlags::ParamSpecValue, None);
    FunctionType::add_param(multi_param_spec_value, &simple_param(instance_int, "a"));
    FunctionType::add_param(multi_param_spec_value, &simple_param(instance_str, "b"));

    assert_eq!(
        print_type(specialize_a(multi_param_spec_value), PrintTypeFlags::PythonSyntax, &callback),
        "A[[int, str], str]"
    );

    // Positional-only separator (`/`) is a nameless Simple param — it should be
    // skipped in the bracketed form, not cause a Callable fallback.
    let positional_only_param_spec_value =
        FunctionType::create_instance("", "", "", FunctionTypeFlags::ParamSpecValue, None);
    FunctionType::add_param(positional_only_param_spec_value, &simple_param(instance_int, "a"));
    FunctionType::add_position_only_param_separator(positional_only_param_spec_value);

    assert_eq!(
        print_type(specialize_a(positional_only_param_spec_value), PrintTypeFlags::None, &callback),
        "A[(a: int, /), str]"
    );

    assert_eq!(
        print_type(specialize_a(positional_only_param_spec_value), PrintTypeFlags::PythonSyntax, &callback),
        "A[[int], str]"
    );

    let complex_param_spec_value = FunctionType::create_instance("", "", "", FunctionTypeFlags::ParamSpecValue, None);
    FunctionType::add_param(complex_param_spec_value, &simple_param(instance_int, "a"));
    FunctionType::add_keyword_only_param_separator(complex_param_spec_value);
    FunctionType::add_param(complex_param_spec_value, &simple_param(instance_str, "b"));

    assert_eq!(
        print_type(specialize_a(complex_param_spec_value), PrintTypeFlags::PythonSyntax, &callback),
        "A[Callable[..., Any], str]"
    );
}

// tests/typePrinter.test.ts:289
#[test]
fn function_types() {
    let callback = return_type_callback();
    let any_param = |category: ParamCategory, name: &str| {
        FunctionParam::create(
            category,
            AnyType::create(false).0,
            FunctionParamFlags::TypeDeclared,
            Some(name),
            None,
            None,
        )
    };

    let func_type_a = FunctionType::create_instance("A", "", "", FunctionTypeFlags::None, None);

    FunctionType::add_param(func_type_a, &any_param(ParamCategory::Simple, "a"));

    FunctionType::add_position_only_param_separator(func_type_a);

    FunctionType::add_param(func_type_a, &any_param(ParamCategory::ArgsList, "args"));

    FunctionType::add_param(func_type_a, &any_param(ParamCategory::KwargsDict, "kwargs"));

    func_type_a.d().shared.get().declared_return_type.set(Some(NeverType::create_no_return().0));

    assert_eq!(
        print_type(func_type_a.0, PrintTypeFlags::None, &callback),
        "(a: Any, /, *args: Any, **kwargs: Any) -> NoReturn"
    );
    assert_eq!(print_type(func_type_a.0, PrintTypeFlags::PythonSyntax, &callback), "Callable[..., NoReturn]");

    let func_type_b = FunctionType::create_instance("B", "", "", FunctionTypeFlags::None, None);

    FunctionType::add_param(func_type_b, &any_param(ParamCategory::Simple, "a"));

    FunctionType::add_position_only_param_separator(func_type_b);

    let param_spec_p = TypeVarType::create_instance("P", TypeVarKind::ParamSpec);
    FunctionType::add_param_spec_variadics(func_type_b, ParamSpecType(param_spec_p.0));

    func_type_b.d().shared.get().declared_return_type.set(Some(NeverType::create_never().0));

    assert_eq!(print_type(func_type_b.0, PrintTypeFlags::None, &callback), "(a: Any, /, **P) -> Never");
    assert_eq!(
        print_type(func_type_b.0, PrintTypeFlags::PythonSyntax, &callback),
        "Callable[Concatenate[Any, P], Never]"
    );

    let func_type_c = FunctionType::create_instance("C", "", "", FunctionTypeFlags::None, None);

    let type_var_tuple_ts = TypeVarType::create_instance("Ts", TypeVarKind::TypeVarTuple);
    let unpacked_ts = TypeVarType::clone_for_unpacked(type_var_tuple_ts, false);

    FunctionType::add_param(
        func_type_c,
        &FunctionParam::create(
            ParamCategory::ArgsList,
            unpacked_ts.0,
            FunctionParamFlags::TypeDeclared,
            Some("args"),
            None,
            None,
        ),
    );

    assert_eq!(print_type(func_type_c.0, PrintTypeFlags::None, &callback), "(*args: *Ts) -> Unknown");
    assert_eq!(
        print_type(func_type_c.0, PrintTypeFlags::UseTypingUnpack, &callback),
        "(*args: Unpack[Ts]) -> Unknown"
    );
    assert_eq!(print_type(func_type_c.0, PrintTypeFlags::PythonSyntax, &callback), "Callable[..., Any]");

    let func_type_d = FunctionType::create_instance("D", "", "", FunctionTypeFlags::None, None);

    func_type_d.d().shared.get().declared_return_type.set(Some(AnyType::create(false).0));
    FunctionType::add_param_spec_variadics(func_type_d, ParamSpecType(param_spec_p.0));

    assert_eq!(print_type(func_type_d.0, PrintTypeFlags::None, &callback), "(**P) -> Any");
    assert_eq!(print_type(func_type_d.0, PrintTypeFlags::PythonSyntax, &callback), "Callable[P, Any]");
}
