//! Port of `packages/pyright-internal/src/analyzer/staticExpressions.ts`.
//!
//! Functions that operate on expressions (parse node trees)
//! whose values can be evaluated statically.

use jsrt::{JsBigInt, JsMap, JsString, NumberValue, P};

use crate::common::config_options::{DefineConstantValue, ExecutionEnvironment, PythonPlatform};
use crate::common::python_version::{PythonReleaseLevel, PythonVersion};
use crate::parser::parse_nodes::{
    ArgCategory, DictionaryEntryNode, ExpressionNode, NumberNode, ParseNode, ParseNodeDetails as D,
    ParseNodeType, StringListNode, TupleNode,
};
use crate::parser::tokenizer_types::{KeywordType, OperatorType};

// analyzer/staticExpressions.ts:27
// Returns undefined if the expression cannot be evaluated
// statically as a bool value or true/false if it can.
pub fn evaluate_static_bool_expression(
    node: ExpressionNode,
    exec_env: &ExecutionEnvironment,
    defined_constants: &JsMap<String, DefineConstantValue>,
    typing_import_aliases: Option<&[String]>,
    sys_import_aliases: Option<&[String]>,
) -> Option<bool> {
    _evaluate_static_bool_or_bool_like_expression(
        node,
        exec_env,
        defined_constants,
        typing_import_aliases,
        sys_import_aliases,
        _evaluate_bool_constant,
    )
}

// analyzer/staticExpressions.ts:47
// Similar to evaluateStaticBoolExpression except that it handles other non-bool
// values that are statically falsy or truthy (like "None", "...", and
// numeric/string/container literals).
pub fn evaluate_static_bool_like_expression(
    node: ExpressionNode,
    exec_env: &ExecutionEnvironment,
    defined_constants: &JsMap<String, DefineConstantValue>,
    typing_import_aliases: Option<&[String]>,
    sys_import_aliases: Option<&[String]>,
) -> Option<bool> {
    _evaluate_static_bool_or_bool_like_expression(
        node,
        exec_env,
        defined_constants,
        typing_import_aliases,
        sys_import_aliases,
        _evaluate_bool_like_literal,
    )
}

// analyzer/staticExpressions.ts:66
// Shared implementation of the two functions above.
// The `evaluateLeafAsBool` callback evaluates leaf expressions.
fn _evaluate_static_bool_or_bool_like_expression(
    node: ExpressionNode,
    exec_env: &ExecutionEnvironment,
    defined_constants: &JsMap<String, DefineConstantValue>,
    typing_import_aliases: Option<&[String]>,
    sys_import_aliases: Option<&[String]>,
    evaluate_leaf_as_bool: fn(ExpressionNode) -> Option<bool>,
) -> Option<bool> {
    if let D::AssignmentExpression(d) = &node.get().d {
        return _evaluate_static_bool_or_bool_like_expression(
            d.right_expr.get(),
            exec_env,
            defined_constants,
            typing_import_aliases,
            sys_import_aliases,
            evaluate_leaf_as_bool,
        );
    }

    if let D::UnaryOperation(d) = &node.get().d {
        if d.operator.get() == OperatorType::Not {
            let value = _evaluate_static_bool_or_bool_like_expression(
                d.expr.get(),
                exec_env,
                defined_constants,
                typing_import_aliases,
                sys_import_aliases,
                // `not x` always forces a truthiness context on its operand and yields real `bool`.
                // So the operand is folded with the bool-like leaf even for strict callers.
                _evaluate_bool_like_literal,
            );
            if let Some(value) = value {
                return Some(!value);
            }
        }
    } else if let D::BinaryOperation(d) = &node.get().d {
        let operator = d.operator.get();
        let left_expr = d.left_expr.get();
        let right_expr = d.right_expr.get();

        // Is it an OR or AND expression?
        if operator == OperatorType::Or || operator == OperatorType::And {
            let left_value = _evaluate_static_bool_or_bool_like_expression(
                left_expr,
                exec_env,
                defined_constants,
                typing_import_aliases,
                sys_import_aliases,
                evaluate_leaf_as_bool,
            );
            let right_value = _evaluate_static_bool_or_bool_like_expression(
                right_expr,
                exec_env,
                defined_constants,
                typing_import_aliases,
                sys_import_aliases,
                evaluate_leaf_as_bool,
            );

            let (Some(left_value), Some(right_value)) = (left_value, right_value) else {
                return None;
            };

            if operator == OperatorType::Or {
                return Some(left_value || right_value);
            } else {
                return Some(left_value && right_value);
            }
        }

        if _is_sys_version_info_expression(left_expr, sys_import_aliases)
            && let Some(right_tuple) = TupleNode::cast(right_expr)
        {
            // Handle the special case of "sys.version_info >= (3, x)"
            let comparison_version = _convert_tuple_to_version(right_tuple);
            return _evaluate_version_binary_operation(
                operator,
                Some(exec_env.python_version),
                comparison_version,
            );
        }

        if let D::Index(left_index) = &left_expr.get().d
            && _is_sys_version_info_expression(left_index.left_expr.get(), sys_import_aliases)
            && left_index.items.borrow().len() == 1
            && !left_index.trailing_comma.get()
            && left_index.items.borrow()[0].d().name.get().is_none()
            && left_index.items.borrow()[0].d().arg_category.get() == ArgCategory::Simple
            && let D::Number(index_number) =
                &left_index.items.borrow()[0].d().value_expr.get().get().d
            && !index_number.is_imaginary.get()
            && index_number.value == NumberValue::Number(0.0)
            && let D::Number(right_number) = &right_expr.get().d
            && right_number.is_integer.get()
            && let NumberValue::Number(right_value) = &right_number.value
        {
            // Handle the special case of "sys.version_info[0] >= X"
            return _evaluate_version_binary_operation(
                operator,
                Some(PythonVersion::create(
                    exec_env.python_version.major,
                    0,
                    None,
                    None,
                    None,
                )),
                Some(PythonVersion::create(
                    number_to_version_part(*right_value),
                    0,
                    None,
                    None,
                    None,
                )),
            );
        }

        if _is_sys_platform_info_expression(left_expr, sys_import_aliases)
            && let Some(right_strings) = StringListNode::cast(right_expr)
        {
            // Handle the special case of "sys.platform != 'X'"
            let comparison_platform = join_string_list_values(right_strings);
            let expected_platform_name = _get_expected_platform_name_from_platform(exec_env);
            return _evaluate_string_binary_operation(
                operator,
                expected_platform_name,
                Some(&comparison_platform),
            );
        }

        if _is_os_name_info_expression(left_expr)
            && let Some(right_strings) = StringListNode::cast(right_expr)
        {
            // Handle the special case of "os.name == 'X'"
            let comparison_os_name = join_string_list_values(right_strings);
            let expected_os_name = _get_expected_os_name_from_platform(exec_env);
            if expected_os_name.is_some() {
                return _evaluate_string_binary_operation(
                    operator,
                    expected_os_name,
                    Some(&comparison_os_name),
                );
            }
        } else {
            // Handle the special case of <definedConstant> == 'X' or <definedConstant> != 'X'.
            if let Some(right_strings) = StringListNode::cast(right_expr) {
                let mut constant_value: Option<&DefineConstantValue> = None;

                if let D::Name(left_name) = &left_expr.get().d {
                    constant_value = defined_constants.get(&left_name.value);
                } else if let D::MemberAccess(left_member_access) = &left_expr.get().d {
                    constant_value =
                        defined_constants.get(&left_member_access.member.get().d().value);
                }

                if let Some(DefineConstantValue::String(constant_value)) = constant_value {
                    let comparison_string_name = join_string_list_values(right_strings);
                    return _evaluate_string_binary_operation(
                        operator,
                        Some(constant_value),
                        Some(&comparison_string_name),
                    );
                }
            }
        }
    } else if let D::Name(d) = &node.get().d {
        if d.value == "TYPE_CHECKING" {
            return Some(true);
        }

        let constant = defined_constants.get(&d.value);
        if let Some(constant) = constant {
            return Some(is_truthy(constant));
        }
    } else if let D::MemberAccess(d) = &node.get().d {
        if let Some(typing_import_aliases) = typing_import_aliases
            && d.member.get().d().value == "TYPE_CHECKING"
            && let D::Name(left_name) = &d.left_expr.get().get().d
            && typing_import_aliases.contains(&left_name.value)
        {
            return Some(true);
        }

        let constant = defined_constants.get(&d.member.get().d().value);
        if let Some(constant) = constant {
            return Some(is_truthy(constant));
        }
    }

    evaluate_leaf_as_bool(node)
}

/// JS `!!constant` on a `boolean | string`.
fn is_truthy(constant: &DefineConstantValue) -> bool {
    match constant {
        DefineConstantValue::Bool(value) => *value,
        DefineConstantValue::String(value) => !value.is_empty(),
    }
}

/// `s.d.value` of a `StringNode | FormatStringNode` in a `StringListNode`.
fn string_node_value(node: P<ParseNode>) -> &'static JsString {
    match &node.get().d {
        D::String(d) => &d.value,
        D::FormatString(d) => &d.value,
        _ => unreachable!("a string list holds only string nodes"),
    }
}

/// `node.d.strings.map((s) => s.d.value).join('')`.
fn join_string_list_values(node: StringListNode) -> JsString {
    node.d()
        .strings
        .borrow()
        .iter()
        .flat_map(|s| string_node_value(*s).units().iter().copied())
        .collect()
}

/// A JS number stored in a `PythonVersion` part, which is `i32`: a fractional
/// value truncates and an out-of-range value saturates.
fn number_to_version_part(value: f64) -> i32 {
    value as i32
}

// analyzer/staticExpressions.ts:225
fn _evaluate_bool_constant(node: ExpressionNode) -> Option<bool> {
    if let D::Constant(d) = &node.get().d {
        if d.const_type.get() == KeywordType::True {
            return Some(true);
        }
        if d.const_type.get() == KeywordType::False {
            return Some(false);
        }
    }

    None
}

// analyzer/staticExpressions.ts:238
fn _evaluate_bool_like_literal(node: ExpressionNode) -> Option<bool> {
    match &node.get().d {
        D::Constant(d) => {
            if d.const_type.get() == KeywordType::True {
                return Some(true);
            }
            if d.const_type.get() == KeywordType::False || d.const_type.get() == KeywordType::None {
                return Some(false);
            }
            None
        }

        D::Ellipsis(_) => Some(true),

        D::Number(_) => Some(_evaluate_number_truthiness(NumberNode(node))),

        D::StringList(_) => _evaluate_string_list_truthiness(StringListNode(node)),

        D::List(d) => _evaluate_sequence_truthiness(&d.items.borrow()),
        D::Set(d) => _evaluate_sequence_truthiness(&d.items.borrow()),
        D::Tuple(d) => _evaluate_sequence_truthiness(&d.items.borrow()),

        D::Dictionary(d) => _evaluate_dict_truthiness(&d.items.borrow()),

        _ => None,
    }
}

// analyzer/staticExpressions.ts:271
fn _evaluate_number_truthiness(node: NumberNode) -> bool {
    // bool(v) is False iff v == 0:
    // - zero (0, 0.0, 0j) is falsy;
    // - everything else is truthy, including infinity and NaN.
    match &node.d().value {
        NumberValue::BigInt(value) => *value != JsBigInt::from(0),
        NumberValue::Number(value) => *value != 0.0,
    }
}

// analyzer/staticExpressions.ts:281
fn _evaluate_string_list_truthiness(node: StringListNode) -> Option<bool> {
    let strings = node.d().strings.borrow();

    // If any segment is an f-string, the concatenated value is runtime-dependent
    // (e.g. f"{x}" may be empty or not).
    if strings
        .iter()
        .any(|str| str.node_type == ParseNodeType::FormatString)
    {
        return None;
    }

    // Truthy iff any segment is non-empty.
    Some(strings.iter().any(|str| string_node_value(*str).len() > 0))
}

// analyzer/staticExpressions.ts:292
fn _evaluate_sequence_truthiness(items: &[ExpressionNode]) -> Option<bool> {
    if items.len() == 0 {
        return Some(false);
    }

    // A concrete element (not an unpack "*x" or a comprehension) guarantees the sequence is non-empty.
    if items.iter().any(|item| {
        item.node_type != ParseNodeType::Unpack && item.node_type != ParseNodeType::Comprehension
    }) {
        return Some(true);
    }

    // Only unpacks/comprehensions remain (e.g. "[*x]", "[i for i in y]").
    None
}

// analyzer/staticExpressions.ts:306
fn _evaluate_dict_truthiness(items: &[DictionaryEntryNode]) -> Option<bool> {
    if items.len() == 0 {
        return Some(false);
    }

    // A concrete key/value entry guarantees the dict is non-empty.
    if items
        .iter()
        .any(|item| item.node_type == ParseNodeType::DictionaryKeyEntry)
    {
        return Some(true);
    }

    // Only "**" expansions or comprehensions remain.
    None
}

// analyzer/staticExpressions.ts:320
fn _convert_tuple_to_version(node: TupleNode) -> Option<PythonVersion> {
    let items = node.d().items.borrow();
    if items.len() >= 2 {
        if let D::Number(major_node) = &items[0].get().d
            && !major_node.is_imaginary.get()
            && let D::Number(minor_node) = &items[1].get().d
            && !minor_node.is_imaginary.get()
        {
            let (NumberValue::Number(major), NumberValue::Number(minor)) =
                (&major_node.value, &minor_node.value)
            else {
                return None;
            };

            let mut micro: Option<f64> = None;
            if items.len() >= 3
                && let D::Number(micro_node) = &items[2].get().d
                && !micro_node.is_imaginary.get()
                && let NumberValue::Number(value) = &micro_node.value
            {
                micro = Some(*value);
            }

            let mut release_level: Option<PythonReleaseLevel> = None;
            if items.len() >= 4
                && let D::StringList(release_level_node) = &items[3].get().d
                && release_level_node.strings.borrow().len() == 1
                && let D::String(release_level_string) =
                    &release_level_node.strings.borrow()[0].get().d
            {
                release_level =
                    PythonReleaseLevel::from_name(&release_level_string.value.to_string_lossy());
            }

            let mut serial: Option<f64> = None;
            if items.len() >= 5
                && let D::Number(serial_node) = &items[4].get().d
                && !serial_node.is_imaginary.get()
                && let NumberValue::Number(value) = &serial_node.value
            {
                serial = Some(*value);
            }

            return Some(PythonVersion::create(
                number_to_version_part(*major),
                number_to_version_part(*minor),
                micro.map(number_to_version_part),
                release_level,
                serial.map(number_to_version_part),
            ));
        }
    } else if items.len() == 1 {
        // `node.d.items[0] as NumberNode`: only a number node has a numeric `value`.
        if let D::Number(major) = &items[0].get().d
            && let NumberValue::Number(value) = &major.value
        {
            return Some(PythonVersion::create(
                number_to_version_part(*value),
                0,
                None,
                None,
                None,
            ));
        }
    }

    None
}

// analyzer/staticExpressions.ts:378
fn _evaluate_version_binary_operation(
    operator_type: OperatorType,
    left_value: Option<PythonVersion>,
    right_value: Option<PythonVersion>,
) -> Option<bool> {
    if let (Some(left_value), Some(right_value)) = (left_value, right_value) {
        if operator_type == OperatorType::LessThan {
            return Some(PythonVersion::is_less_than(&left_value, &right_value));
        }

        if operator_type == OperatorType::LessThanOrEqual {
            return Some(PythonVersion::is_less_or_equal_to(
                &left_value,
                &right_value,
            ));
        }

        if operator_type == OperatorType::GreaterThan {
            return Some(PythonVersion::is_greater_than(&left_value, &right_value));
        }

        if operator_type == OperatorType::GreaterThanOrEqual {
            return Some(PythonVersion::is_greater_or_equal_to(
                &left_value,
                &right_value,
            ));
        }

        if operator_type == OperatorType::Equals {
            return Some(PythonVersion::is_equal_to(&left_value, &right_value));
        }

        if operator_type == OperatorType::NotEquals {
            return Some(!PythonVersion::is_equal_to(&left_value, &right_value));
        }
    }

    None
}

// analyzer/staticExpressions.ts:412
fn _evaluate_string_binary_operation(
    operator_type: OperatorType,
    left_value: Option<&str>,
    right_value: Option<&JsString>,
) -> Option<bool> {
    if let (Some(left_value), Some(right_value)) = (left_value, right_value) {
        if operator_type == OperatorType::Equals {
            return Some(left_value == right_value);
        } else if operator_type == OperatorType::NotEquals {
            return Some(left_value != right_value);
        }
    }

    None
}

// analyzer/staticExpressions.ts:428
fn _is_sys_version_info_expression(
    node: ExpressionNode,
    sys_import_aliases: Option<&[String]>,
) -> bool {
    let default_sys_import_aliases;
    let sys_import_aliases = match sys_import_aliases {
        Some(sys_import_aliases) => sys_import_aliases,
        None => {
            default_sys_import_aliases = [String::from("sys")];
            &default_sys_import_aliases[..]
        }
    };

    if let D::MemberAccess(d) = &node.get().d {
        if let D::Name(left_name) = &d.left_expr.get().get().d
            && d.member.get().d().value == "version_info"
        {
            if sys_import_aliases.contains(&left_name.value) {
                return true;
            }
        }
    }

    false
}

// analyzer/staticExpressions.ts:440
fn _is_sys_platform_info_expression(
    node: ExpressionNode,
    sys_import_aliases: Option<&[String]>,
) -> bool {
    let default_sys_import_aliases;
    let sys_import_aliases = match sys_import_aliases {
        Some(sys_import_aliases) => sys_import_aliases,
        None => {
            default_sys_import_aliases = [String::from("sys")];
            &default_sys_import_aliases[..]
        }
    };

    if let D::MemberAccess(d) = &node.get().d {
        if let D::Name(left_name) = &d.left_expr.get().get().d
            && d.member.get().d().value == "platform"
        {
            if sys_import_aliases.contains(&left_name.value) {
                return true;
            }
        }
    }

    false
}

// analyzer/staticExpressions.ts:452
fn _is_os_name_info_expression(node: ExpressionNode) -> bool {
    if let D::MemberAccess(d) = &node.get().d {
        if let D::Name(left_name) = &d.left_expr.get().get().d
            && left_name.value == "os"
            && d.member.get().d().value == "name"
        {
            return true;
        }
    }

    false
}

// analyzer/staticExpressions.ts:466
fn _get_expected_platform_name_from_platform(
    exec_env: &ExecutionEnvironment,
) -> Option<&'static str> {
    let python_platform = exec_env.python_platform.as_deref();
    if python_platform == Some(PythonPlatform::Darwin.as_str()) {
        return Some("darwin");
    } else if python_platform == Some(PythonPlatform::Windows.as_str()) {
        return Some("win32");
    } else if python_platform == Some(PythonPlatform::Linux.as_str()) {
        return Some("linux");
    } else if python_platform == Some(PythonPlatform::iOS.as_str()) {
        return Some("ios");
    } else if python_platform == Some(PythonPlatform::Android.as_str()) {
        // Python >= 3.13 reports Android as 'android', earlier used to report it as 'linux'
        if exec_env.python_version.major == 3 && exec_env.python_version.minor >= 13 {
            return Some("android");
        } else {
            return Some("linux");
        }
    }

    None
}

// analyzer/staticExpressions.ts:487
fn _get_expected_os_name_from_platform(exec_env: &ExecutionEnvironment) -> Option<&'static str> {
    let python_platform = exec_env.python_platform.as_deref();
    if python_platform == Some(PythonPlatform::Darwin.as_str()) {
        return Some("posix");
    } else if python_platform == Some(PythonPlatform::Windows.as_str()) {
        return Some("nt");
    } else if python_platform == Some(PythonPlatform::Linux.as_str()) {
        return Some("posix");
    } else if python_platform == Some(PythonPlatform::iOS.as_str()) {
        return Some("posix");
    } else if python_platform == Some(PythonPlatform::Android.as_str()) {
        return Some("posix");
    }

    None
}

// Pyright has no unit tests for this file. These build the parse nodes by
// hand; the expected values come from node running `staticExpressions.ts` on
// the same nodes.
#[cfg(test)]
mod tests {
    use super::*;
    use jsrt::JsStr;

    use crate::common::config_options::get_off_diagnostic_rule_set;
    use crate::common::text_range::TextRange;
    use crate::parser::parse_nodes::{
        ArgumentNode, AssignmentExpressionNode, BinaryOperationNode, ConstantNode, IndexNode,
        ListNode, MemberAccessNode, NameNode, StringNode, UnaryOperationNode, UnpackNode,
        create_parse_tree_key,
    };
    use crate::parser::tokenizer_types::{
        IdentifierToken, KeywordToken, NumberToken, StringToken, StringTokenFlags, Token, TokenType,
    };

    fn token() -> P<Token> {
        Token::create(TokenType::Operator, 0, 0, None)
    }

    fn name(value: &str) -> NameNode {
        let token = IdentifierToken::create(0, value.js_len(), value.to_string(), None);
        NameNode::create(create_parse_tree_key(), token)
    }

    fn member(left: P<ParseNode>, member: &str) -> P<ParseNode> {
        MemberAccessNode::create(create_parse_tree_key(), left, name(member)).node()
    }

    fn number(value: NumberValue, is_integer: bool, is_imaginary: bool) -> P<ParseNode> {
        let token = NumberToken::create(0, 1, value, is_integer, is_imaginary, None);
        NumberNode::create(create_parse_tree_key(), token).node()
    }

    fn int(value: f64) -> P<ParseNode> {
        number(NumberValue::Number(value), true, false)
    }

    fn string(value: &str) -> P<ParseNode> {
        let key = create_parse_tree_key();
        let token = StringToken::create(
            0,
            value.js_len() + 2,
            StringTokenFlags::DoubleQuote,
            JsString::from(value),
            0,
            None,
        );
        let string_node = StringNode::create(key, token, JsString::from(value));
        StringListNode::create(key, vec![string_node.node()]).node()
    }

    fn tuple(items: Vec<P<ParseNode>>) -> P<ParseNode> {
        let node = TupleNode::create(create_parse_tree_key(), &TextRange::create(0, 0), true);
        node.d().items.borrow_mut().extend(items);
        node.node()
    }

    fn list(items: Vec<P<ParseNode>>) -> P<ParseNode> {
        let node = ListNode::create(create_parse_tree_key(), &TextRange::create(0, 0));
        node.d().items.borrow_mut().extend(items);
        node.node()
    }

    fn binary(left: P<ParseNode>, operator: OperatorType, right: P<ParseNode>) -> P<ParseNode> {
        BinaryOperationNode::create(create_parse_tree_key(), left, right, token(), operator).node()
    }

    fn not(expr: P<ParseNode>) -> P<ParseNode> {
        UnaryOperationNode::create(create_parse_tree_key(), token(), expr, OperatorType::Not).node()
    }

    fn constant(keyword_type: KeywordType) -> P<ParseNode> {
        let token = KeywordToken::create(0, 0, keyword_type, None);
        ConstantNode::create(create_parse_tree_key(), token).node()
    }

    fn index(left: P<ParseNode>, item: P<ParseNode>) -> P<ParseNode> {
        let key = create_parse_tree_key();
        let argument = ArgumentNode::create(key, None, item, ArgCategory::Simple);
        IndexNode::create(key, left, vec![argument], false, token()).node()
    }

    fn exec_env(major: i32, minor: i32, platform: Option<&str>) -> ExecutionEnvironment {
        ExecutionEnvironment {
            root: None,
            name: String::new(),
            python_version: PythonVersion::create(major, minor, None, None, None),
            python_platform: platform.map(String::from),
            extra_paths: Vec::new(),
            diagnostic_rule_set: get_off_diagnostic_rule_set(),
            skip_native_libraries: false,
        }
    }

    fn constants() -> JsMap<String, DefineConstantValue> {
        let mut constants = JsMap::default();
        constants.insert("DEBUG".to_string(), DefineConstantValue::Bool(true));
        constants.insert(
            "MODE".to_string(),
            DefineConstantValue::String("prod".to_string()),
        );
        constants.insert(
            "EMPTY".to_string(),
            DefineConstantValue::String(String::new()),
        );
        constants
    }

    fn strict(node: P<ParseNode>, env: &ExecutionEnvironment) -> Option<bool> {
        evaluate_static_bool_expression(node, env, &constants(), None, None)
    }

    fn bool_like(node: P<ParseNode>, env: &ExecutionEnvironment) -> Option<bool> {
        evaluate_static_bool_like_expression(node, env, &constants(), None, None)
    }

    fn sys(attribute: &str) -> P<ParseNode> {
        member(name("sys").node(), attribute)
    }

    #[test]
    fn type_checking() {
        let env = exec_env(3, 12, Some("Linux"));
        assert_eq!(strict(name("TYPE_CHECKING").node(), &env), Some(true));

        let typing_type_checking = member(name("typing").node(), "TYPE_CHECKING");
        let typing = [String::from("typing")];
        assert_eq!(
            evaluate_static_bool_expression(
                typing_type_checking,
                &env,
                &constants(),
                Some(&typing),
                None
            ),
            Some(true)
        );
        assert_eq!(strict(typing_type_checking, &env), None);
    }

    #[test]
    fn sys_version_info() {
        let env = exec_env(3, 12, Some("Linux"));
        let at_least_3_8 = binary(
            sys("version_info"),
            OperatorType::GreaterThanOrEqual,
            tuple(vec![int(3.0), int(8.0)]),
        );
        assert_eq!(strict(at_least_3_8, &env), Some(true));
        let below_3_13 = binary(
            sys("version_info"),
            OperatorType::LessThan,
            tuple(vec![int(3.0), int(13.0)]),
        );
        assert_eq!(strict(below_3_13, &env), Some(true));
        let at_least_3 = binary(
            sys("version_info"),
            OperatorType::GreaterThanOrEqual,
            tuple(vec![int(3.0)]),
        );
        assert_eq!(strict(at_least_3, &env), Some(true));

        // An empty alias list is not the `['sys']` default.
        assert_eq!(
            evaluate_static_bool_expression(at_least_3_8, &env, &constants(), None, Some(&[])),
            None
        );

        let major_is_3 = binary(
            index(sys("version_info"), int(0.0)),
            OperatorType::Equals,
            int(3.0),
        );
        assert_eq!(strict(major_is_3, &env), Some(true));
        let major_is_2 = binary(
            index(sys("version_info"), int(0.0)),
            OperatorType::Equals,
            int(2.0),
        );
        assert_eq!(strict(major_is_2, &env), Some(false));
    }

    #[test]
    fn sys_platform_and_os_name() {
        let linux = exec_env(3, 12, Some("Linux"));
        let no_platform = exec_env(3, 12, None);
        let is_linux = binary(sys("platform"), OperatorType::Equals, string("linux"));
        assert_eq!(strict(is_linux, &linux), Some(true));
        assert_eq!(strict(is_linux, &no_platform), None);
        let not_win32 = binary(sys("platform"), OperatorType::NotEquals, string("win32"));
        assert_eq!(strict(not_win32, &linux), Some(true));

        let android_3_13 = exec_env(3, 13, Some("Android"));
        let android_3_12 = exec_env(3, 12, Some("Android"));
        let is_android = binary(sys("platform"), OperatorType::Equals, string("android"));
        assert_eq!(strict(is_android, &android_3_13), Some(true));
        assert_eq!(strict(is_android, &android_3_12), Some(false));

        let os_name = member(name("os").node(), "name");
        let is_nt = binary(os_name, OperatorType::Equals, string("nt"));
        assert_eq!(strict(is_nt, &linux), Some(false));
        assert_eq!(strict(is_nt, &no_platform), None);
    }

    #[test]
    fn defined_constants() {
        let env = exec_env(3, 12, Some("Linux"));
        assert_eq!(strict(name("DEBUG").node(), &env), Some(true));
        assert_eq!(strict(name("EMPTY").node(), &env), Some(false));
        assert_eq!(strict(member(name("x").node(), "DEBUG"), &env), Some(true));

        let mode_is_prod = binary(name("MODE").node(), OperatorType::Equals, string("prod"));
        assert_eq!(strict(mode_is_prod, &env), Some(true));
        let mode_is_not_prod = binary(
            member(name("x").node(), "MODE"),
            OperatorType::NotEquals,
            string("prod"),
        );
        assert_eq!(strict(mode_is_not_prod, &env), Some(false));
        // A bool constant compared with a string is not evaluated.
        let debug_is_x = binary(name("DEBUG").node(), OperatorType::Equals, string("x"));
        assert_eq!(strict(debug_is_x, &env), None);
    }

    #[test]
    fn operators_and_literals() {
        let env = exec_env(3, 12, Some("Linux"));
        let none = constant(KeywordType::None);
        assert_eq!(strict(none, &env), None);
        assert_eq!(bool_like(none, &env), Some(false));
        // `not` folds its operand as bool-like even for the strict variant.
        assert_eq!(strict(not(none), &env), Some(true));

        let walrus = AssignmentExpressionNode::create(
            create_parse_tree_key(),
            name("x"),
            token(),
            constant(KeywordType::True),
        )
        .node();
        assert_eq!(strict(walrus, &env), Some(true));

        let true_and_zero = binary(constant(KeywordType::True), OperatorType::And, int(0.0));
        assert_eq!(strict(true_and_zero, &env), None);
        assert_eq!(bool_like(true_and_zero, &env), Some(false));
        let false_or_one = binary(constant(KeywordType::False), OperatorType::Or, int(1.0));
        assert_eq!(bool_like(false_or_one, &env), Some(true));

        assert_eq!(bool_like(list(vec![]), &env), Some(false));
        assert_eq!(bool_like(list(vec![int(1.0)]), &env), Some(true));
        let star_x = UnpackNode::create(create_parse_tree_key(), token(), name("x").node()).node();
        assert_eq!(bool_like(list(vec![star_x]), &env), None);
        assert_eq!(bool_like(string(""), &env), Some(false));
        assert_eq!(bool_like(string("a"), &env), Some(true));
        assert_eq!(
            bool_like(number(NumberValue::Number(0.0), false, true), &env),
            Some(false)
        );
        assert_eq!(
            bool_like(number(NumberValue::Number(f64::NAN), false, false), &env),
            Some(true)
        );
    }
}
