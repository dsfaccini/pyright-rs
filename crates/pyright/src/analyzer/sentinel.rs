//! Port of `packages/pyright-internal/src/analyzer/sentinel.ts`.
//! Evaluator code: allocate with `P::new_eval` (docs/porting.md, "memory").

use crate::analyzer::analyzer_node_info::AnalyzerNodeInfoAccessor;
use crate::analyzer::parse_tree_utils::{get_class_full_name, get_type_source_id};
use crate::analyzer::type_evaluator::TypeEvaluator;
use crate::analyzer::type_evaluator_types::Arg;
use crate::analyzer::type_utils::compute_mro_linearization;
use crate::analyzer::types::{
    ClassType, ClassTypeFlags, LiteralValue, SentinelLiteral, Type, TypeBase,
};
use crate::common::diagnostic_rules::DiagnosticRule;
use crate::localization::localize::LocMessage;
use crate::parser::parse_nodes::{
    ArgCategory, AssignmentNode, ExpressionNode, NameNode, ParseNodeDetails, StringListNode,
};
use jsrt::{JsString, P};

// analyzer/sentinel.ts:19
pub fn create_sentinel_type(
    evaluator: &TypeEvaluator,
    error_node: ExpressionNode,
    arg_list: &[Arg],
    sentinel_base_class: ClassType,
    node_info: &AnalyzerNodeInfoAccessor,
) -> Option<P<Type>> {
    let mut class_name = String::new();

    if arg_list.len() != 1 {
        evaluator.add_diagnostic(
            DiagnosticRule::reportCallIssue,
            &LocMessage::sentinel_param_count(),
            error_node,
            None,
        );
        return None;
    }

    let (arg_category, value_expression, arg_node) = match &arg_list[0] {
        Arg::ArgWithType(arg) => (arg.arg_category, arg.value_expression, arg.node),
        Arg::ArgWithExpression(arg) => (arg.arg_category, Some(arg.value_expression), arg.node),
    };
    if arg_category == ArgCategory::Simple
        && let Some(string_list) = value_expression.and_then(StringListNode::cast)
    {
        let mut joined = JsString::from("");
        for s in string_list.d().strings.borrow().iter() {
            let value = match &s.get().d {
                ParseNodeDetails::String(d) => &d.value,
                ParseNodeDetails::FormatString(d) => &d.value,
                _ => unreachable!("a string list holds string nodes"),
            };
            joined = joined.concat(value);
        }
        class_name = joined.to_string_lossy();
    }

    if class_name.is_empty() {
        evaluator.add_diagnostic(
            DiagnosticRule::reportArgumentType,
            &LocMessage::sentinel_bad_name(),
            arg_node.map_or(error_node, |node| node.0),
            None,
        );
        return None;
    }

    if let Some(assignment) = error_node.parent.get().and_then(AssignmentNode::cast) {
        let left_expr = assignment.d().left_expr.get();
        if let Some(left_name) = NameNode::cast(left_expr)
            && left_name.d().value != class_name
        {
            evaluator.add_diagnostic(
                DiagnosticRule::reportGeneralTypeIssues,
                &LocMessage::sentinel_name_mismatch(),
                left_expr,
                None,
            );
            return None;
        }
    }

    let file_info = node_info.get_file_info(error_node);
    let full_class_name = get_class_full_name(error_node, &file_info.module_name, &class_name);
    let mut class_type = ClassType::create_instantiable(
        &class_name,
        &full_class_name,
        &file_info.module_name,
        &file_info.file_uri,
        ClassTypeFlags::Final | ClassTypeFlags::ValidTypeAliasClass,
        get_type_source_id(error_node),
        /* declaredMetaclass */ None,
        sentinel_base_class.d().shared.get().effective_metaclass.get(),
        None,
    );

    class_type
        .d()
        .shared
        .get()
        .base_classes
        .borrow_mut()
        .push(sentinel_base_class.0);
    compute_mro_linearization(class_type);
    class_type = ClassType::clone_with_literal(
        class_type,
        Some(LiteralValue::SentinelLiteral(SentinelLiteral::new(full_class_name, class_name))),
    );

    let mut instance_type = ClassType::clone_as_instance(class_type, true).0;

    instance_type = TypeBase::clone_with_type_form(instance_type, Some(instance_type));

    Some(instance_type)
}
