//! Port of `packages/pyright-internal/src/analyzer/parseTreeUtils.ts`.
//!
//! Utility routines for traversing a parse tree.

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

use jsrt::sort::sort_by;
use jsrt::string::JsStr;
use jsrt::{JsString, P};

use super::analyzer_file_info::AnalyzerFileInfo;
use super::analyzer_node_info::{self, AnalyzerNodeInfoReader};
use super::parse_tree_walker::{ParseTreeVisitor, ParseTreeWalker, get_child_nodes};
use super::types::TypeVarScopeId;
use crate::common::core::contains_only_whitespace;
use crate::common::debug::{assert, assert_never, fail};
use crate::common::position_utils::{convert_position_to_offset, convert_text_range_to_range};
use crate::common::text_range::{HasTextRange, Position, Range, TextRange};
use crate::common::text_range_collection::{TextRangeCollection, get_index_containing};
use crate::parser::parse_node_utils::{operator_type_name_map, parse_node_type_name_map};
use crate::parser::parse_nodes::{
    ArgCategory, ArgumentNode, AssertNode, AssignmentExpressionNode, AssignmentNode,
    AugmentedAssignmentNode, AwaitNode, CallNode, ClassNode, ComprehensionForNode,
    ComprehensionIfNode, DecoratorNode, DictionaryKeyEntryNode, EvaluationScopeNode,
    ExecutionScopeNode, ExpressionNode, FunctionNode, ImportAsNode, ImportFromAsNode,
    ImportFromNode, IndexNode, LambdaNode, MemberAccessNode, ModuleNameNode, ModuleNode, NameNode,
    NumberNode, ParamCategory, ParameterNode, ParseNode, ParseNodeDetails, ParseNodeType,
    StatementListNode, StatementNode, StringListNode, StringNode, SuiteNode, TypeAnnotationNode,
    TypeParameterScopeNode, UnaryOperationNode, get_parser_string_annotation, is_expression_node,
};
use crate::parser::parse_tree_utils::StringAnnotationProvider;
use crate::parser::parser::ParseFileResults;
use crate::parser::tokenizer::{Tokenizer, TokenizerOutput};
use crate::parser::tokenizer_types::{
    Comment, FStringMiddleToken, KeywordType, OperatorType, StringToken, StringTokenFlags, Token,
    TokenType,
};

// analyzer/parseTreeUtils.ts:55
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct PrintExpressionFlags(pub u32);

impl PrintExpressionFlags {
    pub const None: PrintExpressionFlags = PrintExpressionFlags(0);

    // Don't use string literals for forward declarations.
    pub const ForwardDeclarations: PrintExpressionFlags = PrintExpressionFlags(1 << 0);

    // By default, strings are truncated. If this flag
    // is specified, the full original string is used.
    pub const DoNotLimitStringLength: PrintExpressionFlags = PrintExpressionFlags(1 << 1);
}

impl BitOr for PrintExpressionFlags {
    type Output = PrintExpressionFlags;
    fn bitor(self, rhs: PrintExpressionFlags) -> PrintExpressionFlags {
        PrintExpressionFlags(self.0 | rhs.0)
    }
}

impl BitOrAssign for PrintExpressionFlags {
    fn bitor_assign(&mut self, rhs: PrintExpressionFlags) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for PrintExpressionFlags {
    type Output = PrintExpressionFlags;
    fn bitand(self, rhs: PrintExpressionFlags) -> PrintExpressionFlags {
        PrintExpressionFlags(self.0 & rhs.0)
    }
}

impl BitAndAssign for PrintExpressionFlags {
    fn bitand_assign(&mut self, rhs: PrintExpressionFlags) {
        self.0 &= rhs.0;
    }
}

impl Not for PrintExpressionFlags {
    type Output = PrintExpressionFlags;
    fn not(self) -> PrintExpressionFlags {
        PrintExpressionFlags(!self.0)
    }
}

// analyzer/parseTreeUtils.ts:66
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvaluationScopeInfo {
    pub node: EvaluationScopeNode,
    pub use_proxy_scope: Option<bool>,
    pub use_chained_module_level_scopes: Option<bool>,
}

/// The object `getCallNodeAndActiveParamIndex` returns. TypeScript declares
/// no name for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallNodeAndActiveParamIndex {
    pub call_node: CallNode,
    pub active_index: i32,
    pub active_or_fake: bool,
}

// Returns the depth of the node as measured from the root
// of the parse tree.
// analyzer/parseTreeUtils.ts:74
pub fn get_node_depth(node: P<ParseNode>) -> u32 {
    let mut depth = 0;
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        depth += 1;
        cur_node = cur.parent.get();
    }

    depth
}

// Returns the deepest node that contains the specified position.
// analyzer/parseTreeUtils.ts:87
pub fn find_node_by_position(
    node: P<ParseNode>,
    position: &Position,
    lines: &TextRangeCollection<TextRange>,
    reader: Option<&dyn AnalyzerNodeInfoReader>,
) -> Option<P<ParseNode>> {
    let offset = convert_position_to_offset(position, lines)?;

    find_node_by_offset(node, offset, reader)
}

// Returns the deepest node that contains the specified offset.
// analyzer/parseTreeUtils.ts:102
pub fn find_node_by_offset(
    node: P<ParseNode>,
    offset: i32,
    reader: Option<&dyn AnalyzerNodeInfoReader>,
) -> Option<P<ParseNode>> {
    if !TextRange::overlaps(&node, offset) {
        return None;
    }

    // When a reader is provided, descend into evaluator-discovered (tier-2) string
    // annotations too. Otherwise getChildNodes defaults to parser-tier annotations.
    let get_string_annotation = reader.map(|reader| {
        move |n: StringListNode| analyzer_node_info::get_string_annotation(n, reader)
    });

    // The range is found within this node. See if we can localize it
    // further by checking its children.
    let mut children = get_child_nodes(
        node,
        get_string_annotation
            .as_ref()
            .map(|f| -> StringAnnotationProvider<'_> { f }),
    );
    if is_compliant_with_node_range_rules(node) && children.len() > 20 {
        // Use binary search to find the child to visit. This should be helpful
        // when there are many siblings, such as statements in a module/suite
        // or expressions in a list, etc. Otherwise, we will have to traverse
        // every sibling before finding the correct one.
        let mut index = get_index_containing(
            &children,
            offset,
            Some(&|child: &P<ParseNode>, position| TextRange::overlaps(child, position)),
        );

        if index >= 0 {
            // Find first sibling that overlaps with the offset. This ensures that
            // our binary search result matches what we would have returned via a
            // linear search.
            let mut search_index = index - 1;
            while search_index >= 0 {
                let previous_child = children[search_index as usize];
                if let Some(previous_child) = previous_child {
                    if TextRange::overlaps(&previous_child, offset) {
                        index = search_index;
                    } else {
                        break;
                    }
                }

                search_index -= 1;
            }

            children = vec![children[index as usize]];
        }
    }

    for child in children {
        let Some(child) = child else {
            continue;
        };

        let containing_child = find_node_by_offset(child, offset, reader);
        if let Some(containing_child) = containing_child {
            // For augmented assignments, prefer the dest expression, which is a clone
            // of the left expression but is used to hold the type of the operation result.
            if let Some(augmented_assignment) = AugmentedAssignmentNode::cast(node) {
                if containing_child == augmented_assignment.d().left_expr.get() {
                    return Some(augmented_assignment.d().dest_expr.get());
                }
            }

            return Some(containing_child);
        }
    }

    Some(node)
}

// analyzer/parseTreeUtils.ts:169
pub fn is_compliant_with_node_range_rules(node: P<ParseNode>) -> bool {
    // ParseNode range rules are
    // 1. Children are all contained within the parent.
    // 2. Children have non-overlapping ranges.
    // 3. Children are listed in increasing order.
    node.node_type != ParseNodeType::Assignment && node.node_type != ParseNodeType::StringList
}

// analyzer/parseTreeUtils.ts:177
pub fn get_class_full_name(
    class_node: P<ParseNode>,
    module_name: &str,
    class_name: &str,
) -> String {
    let mut name_parts: Vec<&str> = vec![class_name];

    let mut cur_node: Option<P<ParseNode>> = Some(class_node);

    // Walk the parse tree looking for classes.
    while let Some(cur) = cur_node {
        let enclosing_class = get_enclosing_class(cur, false);
        cur_node = enclosing_class.map(ClassNode::node);
        if let Some(enclosing_class) = enclosing_class {
            name_parts.push(enclosing_class.d().name.get().d().value());
        }
    }

    name_parts.push(module_name);

    name_parts.reverse();
    name_parts.join(".")
}

// Create an ID that is based on the location within the file.
// This allows us to disambiguate between different types that
// don't have unique names (those that are not created with class
// declarations).
// analyzer/parseTreeUtils.ts:199
pub fn get_type_source_id(node: P<ParseNode>) -> i32 {
    node.start.get()
}

// analyzer/parseTreeUtils.ts:203
pub fn print_arg(node: ArgumentNode, flags: PrintExpressionFlags) -> String {
    let mut arg_str = String::new();
    if node.d().arg_category.get() == ArgCategory::UnpackedList {
        arg_str = "*".to_string();
    } else if node.d().arg_category.get() == ArgCategory::UnpackedDictionary {
        arg_str = "**".to_string();
    }
    if let Some(name) = node.d().name.get() {
        arg_str += name.d().value();
        arg_str += "=";
    }
    arg_str += &print_expression(node.d().value_expr.get(), flags);
    arg_str
}

/// `flags` is TypeScript's default `PrintExpressionFlags.None` when the caller
/// passes none.
// analyzer/parseTreeUtils.ts:217
pub fn print_expression(node: ExpressionNode, flags: PrintExpressionFlags) -> String {
    match node.d() {
        ParseNodeDetails::Name(d) => {
            return d.value().to_string();
        }

        ParseNodeDetails::MemberAccess(d) => {
            return print_expression(d.left_expr.get(), flags) + "." + d.member.get().d().value();
        }

        ParseNodeDetails::Call(d) => {
            let mut lhs = print_expression(d.left_expr.get(), flags);

            // Some left-hand expressions must be parenthesized.
            let left_expr = d.left_expr.get();
            if left_expr.node_type != ParseNodeType::MemberAccess
                && left_expr.node_type != ParseNodeType::Name
                && left_expr.node_type != ParseNodeType::Index
                && left_expr.node_type != ParseNodeType::Call
            {
                lhs = format!("({lhs})");
            }

            let args = d.args.borrow().clone();
            return lhs
                + "("
                + &args
                    .iter()
                    .map(|arg| print_arg(*arg, flags))
                    .collect::<Vec<_>>()
                    .join(", ")
                + ")";
        }

        ParseNodeDetails::Index(d) => {
            let items = d.items.borrow().clone();
            return print_expression(d.left_expr.get(), flags)
                + "["
                + &items
                    .iter()
                    .map(|item| print_arg(*item, flags))
                    .collect::<Vec<_>>()
                    .join(", ")
                + if d.trailing_comma.get() { "," } else { "" }
                + "]";
        }

        ParseNodeDetails::UnaryOperation(d) => {
            let expr_str = print_operator(d.operator.get()).to_string()
                + &print_expression(d.expr.get(), flags);
            return if d.has_parens.get() {
                format!("({expr_str})")
            } else {
                expr_str
            };
        }

        ParseNodeDetails::BinaryOperation(d) => {
            let expr_str = print_expression(d.left_expr.get(), flags)
                + " "
                + print_operator(d.operator.get())
                + " "
                + &print_expression(d.right_expr.get(), flags);

            return if d.has_parens.get() {
                format!("({expr_str})")
            } else {
                expr_str
            };
        }

        ParseNodeDetails::Number(d) => {
            let mut value = d.value.to_js_string();

            // If it's stored as a bigint, strip off the "n".
            if value.ends_with('n') {
                value = value.js_substring(0, value.js_len() - 1).into_owned();
            }

            if d.is_imaginary.get() {
                value += "j";
            }
            return value;
        }

        ParseNodeDetails::StringList(d) => {
            let annotation = get_parser_string_annotation(StringListNode(node));
            match annotation {
                Some(annotation) if (flags & PrintExpressionFlags::ForwardDeclarations).0 != 0 => {
                    return print_expression(annotation, flags);
                }
                _ => {
                    let strings = d.strings.borrow().clone();
                    return strings
                        .iter()
                        .map(|str| print_expression(*str, flags))
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }
        }

        ParseNodeDetails::String(d) => {
            let token = d.token.get();
            let token_flags = token.d().flags;
            let mut expr_string = String::new();
            if token_flags.any(StringTokenFlags::Raw) {
                expr_string += "r";
            }

            if token_flags.any(StringTokenFlags::Unicode) {
                expr_string += "u";
            }

            if token_flags.any(StringTokenFlags::Bytes) {
                expr_string += "b";
            }

            if token_flags.any(StringTokenFlags::Format) {
                expr_string += "f";
            }

            if token_flags.any(StringTokenFlags::Template) {
                expr_string += "t";
            }

            let mut escaped_string: &[u16] = &token.d().escaped_value;
            if (flags & PrintExpressionFlags::DoNotLimitStringLength).0 == 0 {
                let max_string_length = 32;
                escaped_string = escaped_string.js_slice(0, max_string_length);
            }
            let escaped_string = String::from_utf16_lossy(escaped_string);

            if token_flags.any(StringTokenFlags::Triplicate) {
                if token_flags.any(StringTokenFlags::SingleQuote) {
                    expr_string += &format!("'''{escaped_string}'''");
                } else {
                    expr_string += &format!("\"\"\"{escaped_string}\"\"\"");
                }
            } else {
                if token_flags.any(StringTokenFlags::SingleQuote) {
                    expr_string += &format!("'{escaped_string}'");
                } else {
                    expr_string += &format!("\"{escaped_string}\"");
                }
            }

            return expr_string;
        }

        ParseNodeDetails::FormatString(d) => {
            let mut expr_string = "f".to_string();

            // A middle token or a field expression: the two kinds the
            // TypeScript array mixes.
            enum ItemToPrint {
                Token(FStringMiddleToken),
                Node(ExpressionNode),
            }
            let item_start = |item: &ItemToPrint| match item {
                ItemToPrint::Token(token) => token.start,
                ItemToPrint::Node(node) => node.start.get(),
            };

            let mut escaped_string = String::new();
            let mut items_to_print: Vec<ItemToPrint> = d
                .middle_tokens
                .borrow()
                .iter()
                .map(|token| ItemToPrint::Token(*token))
                .chain(
                    d.field_exprs
                        .borrow()
                        .iter()
                        .map(|expr| ItemToPrint::Node(*expr)),
                )
                .collect();
            sort_by(&mut items_to_print, |a, b| item_start(a) - item_start(b));

            while items_to_print.len() > 0 {
                let item_to_print = items_to_print.remove(0);

                match item_to_print {
                    ItemToPrint::Node(item_to_print) => {
                        escaped_string += &format!(
                            "{{{}}}",
                            print_expression(item_to_print, PrintExpressionFlags::None)
                        );
                    }
                    ItemToPrint::Token(item_to_print) => {
                        escaped_string +=
                            &String::from_utf16_lossy(&item_to_print.d().escaped_value);
                    }
                }
            }

            let token_flags = d.token.get().d().flags;
            if token_flags.any(StringTokenFlags::Triplicate) {
                if token_flags.any(StringTokenFlags::SingleQuote) {
                    expr_string += &format!("'''{escaped_string}'''");
                } else {
                    expr_string += &format!("\"\"\"{escaped_string}\"\"\"");
                }
            } else {
                if token_flags.any(StringTokenFlags::SingleQuote) {
                    expr_string += &format!("'{escaped_string}'");
                } else {
                    expr_string += &format!("\"{escaped_string}\"");
                }
            }

            return expr_string;
        }

        ParseNodeDetails::Assignment(d) => {
            return print_expression(d.left_expr.get(), flags)
                + " = "
                + &print_expression(d.right_expr.get(), flags);
        }

        ParseNodeDetails::AssignmentExpression(d) => {
            return print_expression(d.name.get().node(), flags)
                + " := "
                + &print_expression(d.right_expr.get(), flags);
        }

        ParseNodeDetails::TypeAnnotation(d) => {
            return print_expression(d.value_expr.get(), flags)
                + ": "
                + &print_expression(d.annotation.get(), flags);
        }

        ParseNodeDetails::AugmentedAssignment(d) => {
            return print_expression(d.left_expr.get(), flags)
                + " "
                + print_operator(d.operator.get())
                + " "
                + &print_expression(d.right_expr.get(), flags);
        }

        ParseNodeDetails::Await(d) => {
            let expr_str = "await ".to_string() + &print_expression(d.expr.get(), flags);
            return if d.has_parens.get() {
                format!("({expr_str})")
            } else {
                expr_str
            };
        }

        ParseNodeDetails::Ternary(d) => {
            return print_expression(d.if_expr.get(), flags)
                + " if "
                + &print_expression(d.test_expr.get(), flags)
                + " else "
                + &print_expression(d.else_expr.get(), flags);
        }

        ParseNodeDetails::List(d) => {
            let items = d.items.borrow().clone();
            let expressions: Vec<String> = items
                .iter()
                .map(|expr| print_expression(*expr, flags))
                .collect();
            return format!("[{}]", expressions.join(", "));
        }

        ParseNodeDetails::Unpack(d) => {
            return "*".to_string() + &print_expression(d.expr.get(), flags);
        }

        ParseNodeDetails::Tuple(d) => {
            let items = d.items.borrow().clone();
            let expressions: Vec<String> = items
                .iter()
                .map(|expr| print_expression(*expr, flags))
                .collect();
            if expressions.len() == 1 {
                return format!("({}, )", expressions[0]);
            }
            return format!("({})", expressions.join(", "));
        }

        ParseNodeDetails::Yield(d) => {
            if let Some(expr) = d.expr.get() {
                return "yield ".to_string() + &print_expression(expr, flags);
            } else {
                return "yield".to_string();
            }
        }

        ParseNodeDetails::YieldFrom(d) => {
            return "yield from ".to_string() + &print_expression(d.expr.get(), flags);
        }

        ParseNodeDetails::Ellipsis(_) => {
            return "...".to_string();
        }

        ParseNodeDetails::Comprehension(d) => {
            let mut list_str = "<ListExpression>".to_string();

            let expr = d.expr.get();
            if is_expression_node(&expr) {
                list_str = print_expression(expr, flags);
            } else if let Some(entry) = DictionaryKeyEntryNode::cast(expr) {
                let key_str = print_expression(entry.d().key_expr.get(), flags);
                let value_str = print_expression(entry.d().value_expr.get(), flags);
                list_str = format!("{key_str}: {value_str}");
            }

            let for_if_nodes = d.for_if_nodes.borrow().clone();
            list_str = list_str
                + " "
                + &for_if_nodes
                    .iter()
                    .map(|expr| {
                        if let Some(expr) = ComprehensionForNode::cast(*expr) {
                            let async_str = if expr.d().is_async.get() == Some(true) {
                                "async "
                            } else {
                                ""
                            };
                            format!("{async_str}for ")
                                + &print_expression(expr.d().target_expr.get(), flags)
                                + &format!(
                                    " in {}",
                                    print_expression(expr.d().iterable_expr.get(), flags)
                                )
                        } else {
                            format!(
                                "if {}",
                                print_expression(
                                    ComprehensionIfNode(*expr).d().test_expr.get(),
                                    flags
                                )
                            )
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");

            return if d.has_parens.get() {
                format!("({list_str})")
            } else {
                list_str
            };
        }

        ParseNodeDetails::Slice(d) => {
            // The colons are significant here, so they must be emitted even when
            // the surrounding expressions are omitted. Otherwise "x[1:]" would be
            // printed as "x[1]" and "x[::2]" as "x[:2]".
            let mut result = String::new();

            if let Some(start_value) = d.start_value.get() {
                result += &print_expression(start_value, flags);
            }

            result += ":";

            if let Some(end_value) = d.end_value.get() {
                result += &print_expression(end_value, flags);
            }

            if let Some(step_value) = d.step_value.get() {
                result += &(":".to_string() + &print_expression(step_value, flags));
            }

            return result;
        }

        ParseNodeDetails::Lambda(d) => {
            let params = d.params.borrow().clone();
            return "lambda ".to_string()
                + &params
                    .iter()
                    .map(|param| {
                        let mut param_str = String::new();

                        if param.d().category.get() == ParamCategory::ArgsList {
                            param_str += "*";
                        } else if param.d().category.get() == ParamCategory::KwargsDict {
                            param_str += "**";
                        }

                        if let Some(name) = param.d().name.get() {
                            param_str += name.d().value();
                        } else if param.d().category.get() == ParamCategory::Simple {
                            param_str += "/";
                        }

                        if let Some(default_value) = param.d().default_value.get() {
                            param_str +=
                                &(" = ".to_string() + &print_expression(default_value, flags));
                        }
                        param_str
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
                + ": "
                + &print_expression(d.expr.get(), flags);
        }

        ParseNodeDetails::Constant(d) => {
            if d.const_type.get() == KeywordType::True {
                return "True".to_string();
            } else if d.const_type.get() == KeywordType::False {
                return "False".to_string();
            } else if d.const_type.get() == KeywordType::Debug {
                return "__debug__".to_string();
            } else if d.const_type.get() == KeywordType::None {
                return "None".to_string();
            }
        }

        ParseNodeDetails::Dictionary(d) => {
            // A template literal of an array joins its items with "," and no space.
            let items = d.items.borrow().clone();
            let dict_contents = items
                .iter()
                .map(|entry| match entry.d() {
                    ParseNodeDetails::DictionaryKeyEntry(entry) => {
                        format!("{}: ", print_expression(entry.key_expr.get(), flags))
                            + &print_expression(entry.value_expr.get(), flags)
                    }
                    ParseNodeDetails::DictionaryExpandEntry(entry) => {
                        format!("**{}", print_expression(entry.expr.get(), flags))
                    }
                    _ => print_expression(*entry, flags),
                })
                .collect::<Vec<_>>()
                .join(",");

            if !dict_contents.is_empty() {
                return format!("{{ {dict_contents} }}");
            }

            return "{}".to_string();
        }

        ParseNodeDetails::Set(d) => {
            let items = d.items.borrow().clone();
            return format!(
                "{{{}}}",
                items
                    .iter()
                    .map(|entry| print_expression(*entry, flags))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        ParseNodeDetails::Error(_) => {
            return "<Parse Error>".to_string();
        }

        _ => {
            assert_never(node.node_type, None);
        }
    }

    "<Expression>".to_string()
}

// analyzer/parseTreeUtils.ts:581
pub fn print_operator(operator: OperatorType) -> &'static str {
    let operator_name = operator_type_name_map(operator);
    if let Some(operator_name) = operator_name {
        operator_name
    } else {
        "unknown"
    }
}

// If the name node is the LHS of a call expression or is a member
// name in the LHS of a call expression, returns the call node.
// analyzer/parseTreeUtils.ts:592
pub fn get_call_for_name(node: NameNode) -> Option<CallNode> {
    if let Some(parent) = node.parent.get().and_then(CallNode::cast) {
        if parent.d().left_expr.get() == node {
            return Some(parent);
        }
    }

    if let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) {
        if parent.d().member.get() == node {
            if let Some(grandparent) = parent.parent.get().and_then(CallNode::cast) {
                if grandparent.d().left_expr.get() == parent {
                    return Some(grandparent);
                }
            }
        }
    }

    None
}

// analyzer/parseTreeUtils.ts:609
pub fn get_decorator_for_name(node: NameNode) -> Option<DecoratorNode> {
    if let Some(parent) = node.parent.get().and_then(DecoratorNode::cast) {
        if parent.d().expr.get() == node {
            return Some(parent);
        }
    }

    if let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) {
        if parent.d().member.get() == node {
            if let Some(grandparent) = parent.parent.get().and_then(DecoratorNode::cast) {
                if grandparent.d().expr.get() == parent {
                    return Some(grandparent);
                }
            }
        }
    }

    None
}

// analyzer/parseTreeUtils.ts:626
pub fn get_enclosing_suite(node: P<ParseNode>) -> Option<SuiteNode> {
    let mut cur_node = node.parent.get();

    while let Some(cur) = cur_node {
        if let Some(suite) = SuiteNode::cast(cur) {
            return Some(suite);
        }
        cur_node = cur.parent.get();
    }

    None
}

/// `stop_at_function` is TypeScript's default `false` when the caller passes none.
// analyzer/parseTreeUtils.ts:639
pub fn get_enclosing_class(node: P<ParseNode>, stop_at_function: bool) -> Option<ClassNode> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if let Some(class_node) = ClassNode::cast(cur) {
            return Some(class_node);
        }

        if cur.node_type == ParseNodeType::Module {
            return None;
        }

        if cur.node_type == ParseNodeType::Function {
            if stop_at_function {
                return None;
            }
        }

        cur_node = cur.parent.get();
    }

    None
}

// analyzer/parseTreeUtils.ts:662
pub fn get_enclosing_module(node: P<ParseNode>) -> ModuleNode {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if let Some(module) = ModuleNode::cast(cur) {
            return module;
        }

        cur_node = cur.parent.get();
    }

    fail(Some("Module node not found"));
}

/// Returns a `ClassNode` or a `ModuleNode`. `stop_at_function` is
/// TypeScript's default `false` when the caller passes none.
// analyzer/parseTreeUtils.ts:676
pub fn get_enclosing_class_or_module(
    node: P<ParseNode>,
    stop_at_function: bool,
) -> Option<P<ParseNode>> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if cur.node_type == ParseNodeType::Class {
            return Some(cur);
        }

        if cur.node_type == ParseNodeType::Module {
            return Some(cur);
        }

        if cur.node_type == ParseNodeType::Function {
            if stop_at_function {
                return None;
            }
        }

        cur_node = cur.parent.get();
    }

    None
}

// analyzer/parseTreeUtils.ts:699
pub fn get_enclosing_function(node: P<ParseNode>) -> Option<FunctionNode> {
    let mut cur_node = node.parent.get();
    let mut prev_node: Option<P<ParseNode>> = None;

    while let Some(cur) = cur_node {
        if let Some(function_node) = FunctionNode::cast(cur) {
            // Don't treat a decorator as being "enclosed" in the function.
            if !function_node
                .d()
                .decorators
                .borrow()
                .iter()
                .any(|decorator| Some(decorator.node()) == prev_node)
            {
                return Some(function_node);
            }
        }

        if cur.node_type == ParseNodeType::Class {
            return None;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    None
}

// This is similar to getEnclosingFunction except that it uses evaluation
// scopes rather than the parse tree to determine whether the specified node
// is within the scope. That means if the node is within a class decorator
// (for example), it will be considered part of its parent node rather than
// the class node.
// analyzer/parseTreeUtils.ts:727
pub fn get_enclosing_function_evaluation_scope(
    node: P<ParseNode>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Option<FunctionNode> {
    let mut cur_node = get_evaluation_scope_node(node, node_info).node;

    loop {
        if let Some(function_node) = FunctionNode::cast(cur_node) {
            return Some(function_node);
        }

        if cur_node.node_type == ParseNodeType::Class {
            return None;
        }
        let parent = cur_node.parent.get()?;

        cur_node = get_evaluation_scope_node(parent, node_info).node;
    }
}

// analyzer/parseTreeUtils.ts:748
pub fn get_enclosing_lambda(node: P<ParseNode>) -> Option<LambdaNode> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if let Some(lambda) = LambdaNode::cast(cur) {
            return Some(lambda);
        }

        if cur.node_type == ParseNodeType::Suite {
            return None;
        }

        cur_node = cur.parent.get();
    }

    None
}

/// Returns a `FunctionNode` or a `ClassNode`.
// analyzer/parseTreeUtils.ts:765
pub fn get_enclosing_class_or_function(node: P<ParseNode>) -> Option<P<ParseNode>> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if cur.node_type == ParseNodeType::Function {
            return Some(cur);
        }

        if cur.node_type == ParseNodeType::Class {
            return Some(cur);
        }

        cur_node = cur.parent.get();
    }

    None
}

// analyzer/parseTreeUtils.ts:782
pub fn get_enclosing_class_or_function_suite(node: P<ParseNode>) -> Option<SuiteNode> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if let Some(suite) = SuiteNode::cast(cur) {
            let parent_type = suite.parent.get().map(|parent| parent.node_type);
            if parent_type == Some(ParseNodeType::Function)
                || parent_type == Some(ParseNodeType::Class)
            {
                return Some(suite);
            }
        }

        cur_node = cur.parent.get();
    }

    None
}

/// Returns a `SuiteNode` or a `ModuleNode`. TypeScript's defaults are
/// `stop_at_function = false` and `stop_at_lambda = true`.
// analyzer/parseTreeUtils.ts:800
pub fn get_enclosing_suite_or_module(
    node: P<ParseNode>,
    stop_at_function: bool,
    stop_at_lambda: bool,
) -> Option<P<ParseNode>> {
    let mut cur_node = node.parent.get();
    while let Some(cur) = cur_node {
        if cur.node_type == ParseNodeType::Suite {
            return Some(cur);
        }

        if cur.node_type == ParseNodeType::Module {
            return Some(cur);
        }

        if cur.node_type == ParseNodeType::Lambda {
            if stop_at_lambda {
                return None;
            }
        }

        if cur.node_type == ParseNodeType::Function {
            if stop_at_function {
                return None;
            }
        }

        cur_node = cur.parent.get();
    }

    None
}

/// Returns a `LambdaNode`, `FunctionNode`, `ModuleNode` or `ClassNode`.
// analyzer/parseTreeUtils.ts:833
pub fn get_evaluation_node_for_assignment_expression(
    node: AssignmentExpressionNode,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Option<P<ParseNode>> {
    // PEP 572 indicates that the evaluation node for an assignment expression
    // target within a list comprehension is contained within a lambda,
    // function or module, but not a class.
    let mut saw_comprehension = false;
    let mut cur_node: Option<P<ParseNode>> =
        Some(get_evaluation_scope_node(node.node(), node_info).node);

    while let Some(cur) = cur_node {
        match cur.node_type {
            ParseNodeType::Function | ParseNodeType::Lambda | ParseNodeType::Module => {
                return Some(cur);
            }

            ParseNodeType::Class => {
                return if saw_comprehension { None } else { Some(cur) };
            }

            ParseNodeType::Comprehension => {
                saw_comprehension = true;
                cur_node =
                    Some(get_evaluation_scope_node(cur.parent.get().unwrap(), node_info).node);
            }

            _ => {
                return None;
            }
        }
    }

    None
}

// Returns the parse node corresponding to the scope that is used to evaluate
// a symbol referenced in the specified node.
// analyzer/parseTreeUtils.ts:868
pub fn get_evaluation_scope_node(
    node: P<ParseNode>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> EvaluationScopeInfo {
    let mut prev_node: Option<P<ParseNode>> = None;
    let mut prev_prev_node: Option<P<ParseNode>> = None;
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut is_param_name_node = false;
    let mut is_param_default_node = false;
    let mut use_chained_module_level_scopes = false;

    while let Some(cur) = cur_node {
        if let Some(param) = ParameterNode::cast(cur) {
            if prev_node.is_some() && prev_node == param.d().name.get().map(NameNode::node) {
                // Note that we passed through a parameter name node.
                is_param_name_node = true;
            } else if prev_node.is_some() && prev_node == param.d().default_value.get() {
                // Note that we passed through a parameter default value node.
                is_param_default_node = true;
            }
        }

        // We found a scope associated with this node. In most cases,
        // we'll return this scope, but in a few cases we need to return
        // the enclosing scope instead.
        match cur.d() {
            ParseNodeDetails::TypeParameterList(_) => {
                return EvaluationScopeInfo {
                    node: cur,
                    use_proxy_scope: Some(true),
                    use_chained_module_level_scopes: Some(use_chained_module_level_scopes),
                };
            }

            ParseNodeDetails::Function(d) => 'case: {
                let Some(prev) = prev_node else {
                    break 'case;
                };

                // Decorators are always evaluated outside of the function scope.
                if d.decorators
                    .borrow()
                    .iter()
                    .any(|decorator| *decorator == prev)
                {
                    break 'case;
                }

                // The name of the function is evaluated within the containing scope.
                if prev == d.name.get() {
                    break 'case;
                }

                if d.params.borrow().iter().any(|param| *param == prev) {
                    // Default argument expressions are evaluated outside of the function scope.
                    if is_param_default_node {
                        break 'case;
                    }

                    if is_param_name_node {
                        if analyzer_node_info::get_scope(cur, node_info).is_some() {
                            return EvaluationScopeInfo {
                                node: cur,
                                use_proxy_scope: None,
                                use_chained_module_level_scopes: None,
                            };
                        }
                    }
                }

                if prev == d.suite.get() {
                    if analyzer_node_info::get_scope(cur, node_info).is_some() {
                        return EvaluationScopeInfo {
                            node: cur,
                            use_proxy_scope: None,
                            use_chained_module_level_scopes: Some(true),
                        };
                    }
                }

                // All other nodes in the function are evaluated in the context
                // of the type parameter scope if it's present. Otherwise,
                // they are evaluated within the function's parent scope.
                if let Some(scope_node) = d.type_params.get() {
                    if analyzer_node_info::get_scope(scope_node, node_info).is_some() {
                        return EvaluationScopeInfo {
                            node: scope_node.node(),
                            use_proxy_scope: Some(true),
                            use_chained_module_level_scopes: Some(use_chained_module_level_scopes),
                        };
                    }
                }
            }

            ParseNodeDetails::Lambda(d) => {
                if d.params
                    .borrow()
                    .iter()
                    .any(|param| Some(param.node()) == prev_node)
                {
                    if is_param_name_node {
                        if analyzer_node_info::get_scope(cur, node_info).is_some() {
                            return EvaluationScopeInfo {
                                node: cur,
                                use_proxy_scope: None,
                                use_chained_module_level_scopes: None,
                            };
                        }
                    }
                } else if prev_node.is_none() || prev_node == Some(d.expr.get()) {
                    if analyzer_node_info::get_scope(cur, node_info).is_some() {
                        return EvaluationScopeInfo {
                            node: cur,
                            use_proxy_scope: None,
                            use_chained_module_level_scopes: Some(true),
                        };
                    }
                }
            }

            ParseNodeDetails::Class(d) => 'case: {
                let Some(prev) = prev_node else {
                    break 'case;
                };

                // Decorators are always evaluated outside of the class scope.
                if d.decorators
                    .borrow()
                    .iter()
                    .any(|decorator| *decorator == prev)
                {
                    break 'case;
                }

                if prev == d.suite.get() {
                    if analyzer_node_info::get_scope(cur, node_info).is_some() {
                        return EvaluationScopeInfo {
                            node: cur,
                            use_proxy_scope: None,
                            use_chained_module_level_scopes: None,
                        };
                    }
                }

                // Class header expressions (bases, keyword args, type params) are
                // evaluated in the enclosing scope. Enable chained lookup so that
                // earlier-cell globals are visible in those positions.
                use_chained_module_level_scopes = true;

                // All other nodes in the class are evaluated in the context
                // of the type parameter scope if it's present. Otherwise,
                // they are evaluated within the class' parent scope.
                if let Some(scope_node) = d.type_params.get() {
                    if analyzer_node_info::get_scope(scope_node, node_info).is_some() {
                        return EvaluationScopeInfo {
                            node: scope_node.node(),
                            use_proxy_scope: Some(true),
                            use_chained_module_level_scopes: Some(true),
                        };
                    }
                }
            }

            ParseNodeDetails::Comprehension(d) => {
                if analyzer_node_info::get_scope(cur, node_info).is_some() {
                    // The iterable expression of the first subnode of a list comprehension
                    // is evaluated within the scope of its parent.
                    let first_for_if_node = d.for_if_nodes.borrow().first().copied();
                    let is_first_iterable_expr = prev_node == first_for_if_node
                        && first_for_if_node.unwrap().node_type == ParseNodeType::ComprehensionFor
                        && Some(
                            ComprehensionForNode(first_for_if_node.unwrap())
                                .d()
                                .iterable_expr
                                .get(),
                        ) == prev_prev_node;

                    if !is_first_iterable_expr {
                        // Only enable chained scopes for comprehensions inside
                        // functions/lambdas; module-level comprehensions already
                        // see earlier-cell symbols through normal binding.
                        return EvaluationScopeInfo {
                            node: cur,
                            use_proxy_scope: None,
                            use_chained_module_level_scopes: Some(
                                get_enclosing_function(cur).is_some()
                                    || get_enclosing_lambda(cur).is_some(),
                            ),
                        };
                    }
                }
            }

            ParseNodeDetails::TypeAlias(d) => {
                if prev_node == Some(d.expr.get()) {
                    if let Some(scope_node) = d.type_params.get() {
                        if analyzer_node_info::get_scope(scope_node, node_info).is_some() {
                            return EvaluationScopeInfo {
                                node: scope_node.node(),
                                use_proxy_scope: None,
                                use_chained_module_level_scopes: None,
                            };
                        }
                    }
                }
            }

            ParseNodeDetails::Module(_)
                if analyzer_node_info::get_scope(cur, node_info).is_some() =>
            {
                return EvaluationScopeInfo {
                    node: cur,
                    use_proxy_scope: None,
                    use_chained_module_level_scopes: Some(use_chained_module_level_scopes),
                };
            }

            _ => {}
        }

        prev_prev_node = prev_node;
        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    fail(Some("Did not find evaluation scope"));
}

// Returns the parse node corresponding to the function, class, or type alias
// that potentially provides the scope for a type parameter.
// analyzer/parseTreeUtils.ts:1044
pub fn get_type_var_scope_node(node: P<ParseNode>) -> Option<TypeParameterScopeNode> {
    let mut prev_node: Option<P<ParseNode>> = None;
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        match cur.d() {
            ParseNodeDetails::Function(d) => {
                if !d
                    .decorators
                    .borrow()
                    .iter()
                    .any(|decorator| Some(decorator.node()) == prev_node)
                {
                    return Some(cur);
                }
            }

            ParseNodeDetails::Class(d) => {
                if !d
                    .decorators
                    .borrow()
                    .iter()
                    .any(|decorator| Some(decorator.node()) == prev_node)
                {
                    return Some(cur);
                }
            }

            ParseNodeDetails::TypeAlias(_) => {
                return Some(cur);
            }

            _ => {}
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    None
}

// Returns the parse node corresponding to the scope that is used
// for executing the code referenced in the specified node.
// analyzer/parseTreeUtils.ts:1078
pub fn get_execution_scope_node(
    node: P<ParseNode>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> ExecutionScopeNode {
    let mut evaluation_scope = get_evaluation_scope_node(node, node_info).node;

    // Classes are not considered execution scope because they are executed
    // within the context of their containing module or function. Likewise,
    // list comprehensions are executed within their container. Type parameter
    // scopes are special because they act as proxies for their containing
    // function or class scope.
    while evaluation_scope.node_type == ParseNodeType::TypeParameterList
        || evaluation_scope.node_type == ParseNodeType::Class
        || evaluation_scope.node_type == ParseNodeType::Comprehension
    {
        evaluation_scope =
            get_evaluation_scope_node(evaluation_scope.parent.get().unwrap(), node_info).node;
    }

    evaluation_scope
}

// Given a node within a type annotation expression, returns the type annotation
// node that contains it (if applicable).
// analyzer/parseTreeUtils.ts:1102
pub fn get_type_annotation_node(node: P<ParseNode>) -> Option<TypeAnnotationNode> {
    let mut prev_node = node;
    let mut cur_node = node.parent.get();

    while let Some(cur) = cur_node {
        if let Some(type_annotation) = TypeAnnotationNode::cast(cur) {
            if type_annotation.d().annotation.get() == prev_node {
                return Some(type_annotation);
            }

            break;
        }

        prev_node = cur;
        cur_node = cur.parent.get();
    }

    None
}

// In general, arguments passed to a call are evaluated by the runtime in
// left-to-right order. There is one exception, however, when an unpacked
// iterable is used after a keyword argument.
// analyzer/parseTreeUtils.ts:1125
pub fn get_args_by_runtime_order(node: CallNode) -> Vec<ArgumentNode> {
    let args = node.d().args.borrow().clone();
    let positional_args: Vec<ArgumentNode> = args
        .iter()
        .filter(|arg| {
            arg.d().name.get().is_none()
                && arg.d().arg_category.get() != ArgCategory::UnpackedDictionary
        })
        .copied()
        .collect();
    let keyword_args: Vec<ArgumentNode> = args
        .iter()
        .filter(|arg| {
            arg.d().name.get().is_some()
                || arg.d().arg_category.get() == ArgCategory::UnpackedDictionary
        })
        .copied()
        .collect();
    [positional_args, keyword_args].concat()
}

// PEP 591 spells out certain limited cases where an assignment target
// can be annotated with a "Final" annotation. This function determines
// whether Final is allowed for the specified node.
// analyzer/parseTreeUtils.ts:1138
pub fn is_final_allowed_for_assignment_target(target_node: ExpressionNode) -> bool {
    // Simple names always support Final.
    if target_node.node_type == ParseNodeType::Name {
        return true;
    }

    // Member access expressions like "self.x" are permitted only
    // within __init__ methods.
    if let Some(member_access) = MemberAccessNode::cast(target_node) {
        if member_access.d().left_expr.get().node_type != ParseNodeType::Name {
            return false;
        }

        let class_node = get_enclosing_class(target_node, false);
        if class_node.is_none() {
            return false;
        }

        let Some(method_node) = get_enclosing_function(target_node) else {
            return false;
        };

        if method_node.d().name.get().d().value() != "__init__" {
            return false;
        }

        return true;
    }

    false
}

// analyzer/parseTreeUtils.ts:1171
pub fn is_required_allowed_for_assignment_target(target_node: ExpressionNode) -> bool {
    let class_node = get_enclosing_class(target_node, /* stopAtFunction */ true);
    if class_node.is_none() {
        return false;
    }

    true
}

// analyzer/parseTreeUtils.ts:1180
pub fn is_node_contained_within(node: P<ParseNode>, potential_container: P<ParseNode>) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    while let Some(cur) = cur_node {
        if cur == potential_container {
            return true;
        }

        cur_node = cur.parent.get();
    }

    false
}

/// The TypeScript type parameter `T` is the caller's cast: the result is the
/// untyped node, which the caller casts with the typed handle's `cast`.
// analyzer/parseTreeUtils.ts:1193
pub fn get_parent_node_of_type(
    node: P<ParseNode>,
    container_type: ParseNodeType,
) -> Option<P<ParseNode>> {
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        if cur.node_type == container_type {
            return Some(cur);
        }

        cur_node = cur.parent.get();
    }

    None
}

// If the specified node is contained within an expression that is intended to be
// interpreted as a type annotation, this function returns the annotation node.
// analyzer/parseTreeUtils.ts:1209
pub fn get_parent_annotation_node(node: ExpressionNode) -> Option<ExpressionNode> {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;

    while let Some(cur) = cur_node {
        if let Some(function_node) = FunctionNode::cast(cur) {
            if prev_node == function_node.d().return_annotation.get() {
                return prev_node;
            }
            return None;
        }

        if let Some(param) = ParameterNode::cast(cur) {
            if prev_node == param.d().annotation.get()
                || prev_node == param.d().annotation_comment.get()
            {
                return prev_node;
            }
            return None;
        }

        if let Some(assignment) = AssignmentNode::cast(cur) {
            if prev_node == assignment.d().annotation_comment.get() {
                return prev_node;
            }
            return None;
        }

        if let Some(type_annotation) = TypeAnnotationNode::cast(cur) {
            if prev_node == Some(type_annotation.d().annotation.get()) {
                return prev_node;
            }
            return None;
        }

        if let ParseNodeDetails::FunctionAnnotation(d) = cur.d() {
            if prev_node == Some(d.return_annotation.get())
                || d.param_annotations
                    .borrow()
                    .iter()
                    .any(|p| Some(*p) == prev_node)
            {
                assert(prev_node.is_none_or(|prev| is_expression_node(&prev)), None);
                return prev_node;
            }
            return None;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    None
}

// analyzer/parseTreeUtils.ts:1257
pub fn is_node_contained_within_node_type(
    node: P<ParseNode>,
    container_type: ParseNodeType,
) -> bool {
    get_parent_node_of_type(node, container_type).is_some()
}

// analyzer/parseTreeUtils.ts:1261
pub fn is_suite_empty(node: SuiteNode) -> bool {
    let mut saw_ellipsis = false;

    let statements = node.d().statements.borrow().clone();
    for statement in statements {
        if let Some(statement) = StatementListNode::cast(statement) {
            let substatements = statement.d().statements.borrow().clone();
            for substatement in substatements {
                if substatement.node_type == ParseNodeType::Ellipsis {
                    // Allow an ellipsis
                    saw_ellipsis = true;
                } else if substatement.node_type == ParseNodeType::StringList {
                    // Allow doc strings
                } else {
                    return false;
                }
            }
        } else {
            return false;
        }
    }

    saw_ellipsis
}

// analyzer/parseTreeUtils.ts:1284
pub fn contains_await_node(node: P<ParseNode>) -> bool {
    // analyzer/parseTreeUtils.ts:1287
    struct AwaitNodeWalker {
        found_await: bool,
    }

    impl ParseTreeVisitor<bool> for AwaitNodeWalker {
        fn default_visit_result(&self) -> bool {
            true
        }

        // analyzer/parseTreeUtils.ts:1288
        fn visit_await(&mut self, _node: AwaitNode) -> bool {
            self.found_await = true;
            false
        }
    }

    impl ParseTreeWalker for AwaitNodeWalker {}

    let mut walker = AwaitNodeWalker { found_await: false };
    walker.walk(node);
    walker.found_await
}

// Determines whether two expressions match. Names are compared by value only
// unless an optional compareName function is provided. In that case, the
// compareName function is called to determine whether the two names match.
// This allows the caller to distinguish between names that are identical
// but defined in different scopes.
// analyzer/parseTreeUtils.ts:1304
pub fn is_matching_expression(
    reference: ExpressionNode,
    expression: ExpressionNode,
    compare_name: Option<&dyn Fn(NameNode, NameNode) -> bool>,
) -> bool {
    if let Some(reference) = NameNode::cast(reference) {
        let mut name_to_compare: Option<NameNode> = None;

        if let Some(expression) = NameNode::cast(expression) {
            name_to_compare = Some(expression);
        } else if let Some(expression) = AssignmentExpressionNode::cast(expression) {
            name_to_compare = Some(expression.d().name.get());
        }

        if let Some(name_to_compare) = name_to_compare {
            if reference.d().value() != name_to_compare.d().value() {
                return false;
            }

            if let Some(compare_name) = compare_name {
                return compare_name(reference, name_to_compare);
            }

            return true;
        }

        return false;
    } else if let (Some(reference), Some(expression)) = (
        MemberAccessNode::cast(reference),
        MemberAccessNode::cast(expression),
    ) {
        return is_matching_expression(
            reference.d().left_expr.get(),
            expression.d().left_expr.get(),
            None,
        ) && reference.d().member.get().d().value() == expression.d().member.get().d().value();
    } else if let (Some(reference), Some(expression)) =
        (IndexNode::cast(reference), IndexNode::cast(expression))
    {
        if !is_matching_expression(
            reference.d().left_expr.get(),
            expression.d().left_expr.get(),
            None,
        ) {
            return false;
        }

        let expression_items = expression.d().items.borrow().clone();
        if expression_items.len() != 1
            || expression.d().trailing_comma.get()
            || expression_items[0].d().name.get().is_some()
            || expression_items[0].d().arg_category.get() != ArgCategory::Simple
        {
            return false;
        }

        let expr = reference.d().items.borrow()[0].d().value_expr.get();
        if let Some(expr) = NumberNode::cast(expr) {
            let subscript_node = expression_items[0].d().value_expr.get();
            let Some(subscript_node) = NumberNode::cast(subscript_node).filter(|subscript_node| {
                !subscript_node.d().is_imaginary.get() && subscript_node.d().is_integer.get()
            }) else {
                return false;
            };

            return expr.d().value == subscript_node.d().value;
        }

        if let Some(expr) = UnaryOperationNode::cast(expr).filter(|expr| {
            expr.d().operator.get() == OperatorType::Subtract
                && expr.d().expr.get().node_type == ParseNodeType::Number
        }) {
            let subscript_node = expression_items[0].d().value_expr.get();
            let Some(subscript_node) =
                UnaryOperationNode::cast(subscript_node).filter(|subscript_node| {
                    subscript_node.d().operator.get() == OperatorType::Subtract
                        && NumberNode::cast(subscript_node.d().expr.get()).is_some_and(|number| {
                            !number.d().is_imaginary.get() && number.d().is_integer.get()
                        })
                })
            else {
                return false;
            };

            return NumberNode(expr.d().expr.get()).d().value
                == NumberNode(subscript_node.d().expr.get()).d().value;
        }

        if let Some(expr) = StringListNode::cast(expr) {
            let reference_string_list_node = expr;
            let subscript_node = expression_items[0].d().value_expr.get();
            let reference_strings = reference_string_list_node.d().strings.borrow().clone();
            if reference_strings.len() == 1
                && reference_strings[0].node_type == ParseNodeType::String
            {
                if let Some(subscript_node) = StringListNode::cast(subscript_node) {
                    let subscript_strings = subscript_node.d().strings.borrow().clone();
                    if subscript_strings.len() == 1
                        && subscript_strings[0].node_type == ParseNodeType::String
                    {
                        return StringNode(reference_strings[0]).d().value
                            == StringNode(subscript_strings[0]).d().value;
                    }
                }
            }
        }

        return false;
    }

    false
}

// analyzer/parseTreeUtils.ts:1406
pub fn is_partial_matching_expression(
    reference: ExpressionNode,
    expression: ExpressionNode,
) -> bool {
    if let Some(reference) = MemberAccessNode::cast(reference) {
        return is_matching_expression(reference.d().left_expr.get(), expression, None)
            || is_partial_matching_expression(reference.d().left_expr.get(), expression);
    } else if let Some(reference) = IndexNode::cast(reference) {
        return is_matching_expression(reference.d().left_expr.get(), expression, None)
            || is_partial_matching_expression(reference.d().left_expr.get(), expression);
    }

    false
}

// analyzer/parseTreeUtils.ts:1422
pub fn is_within_default_param_initializer(node: P<ParseNode>) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;

    while let Some(cur) = cur_node {
        if let Some(param) = ParameterNode::cast(cur) {
            if prev_node == param.d().default_value.get() {
                return true;
            }
        }

        if cur.node_type == ParseNodeType::Lambda
            || cur.node_type == ParseNodeType::Function
            || cur.node_type == ParseNodeType::Class
            || cur.node_type == ParseNodeType::Module
        {
            return false;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    false
}

// analyzer/parseTreeUtils.ts:1447
pub fn is_within_type_annotation(
    node: P<ParseNode>,
    require_quoted_annotation: bool,
    node_info: Option<&dyn AnalyzerNodeInfoReader>,
) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;
    let mut is_quoted = false;

    while let Some(cur) = cur_node {
        if let Some(param) = ParameterNode::cast(cur) {
            if prev_node == param.d().annotation.get()
                || prev_node == param.d().annotation_comment.get()
            {
                return is_quoted || !require_quoted_annotation;
            }
        }

        if let Some(function_node) = FunctionNode::cast(cur) {
            if prev_node == function_node.d().return_annotation.get() {
                return is_quoted || !require_quoted_annotation;
            }
        }

        if let Some(function_node) = FunctionNode::cast(cur) {
            if prev_node
                == function_node
                    .d()
                    .func_annotation_comment
                    .get()
                    .map(|n| n.node())
            {
                // Type comments are always considered forward declarations even though
                // they're not "quoted".
                return true;
            }
        }

        if let Some(type_annotation) = TypeAnnotationNode::cast(cur) {
            if prev_node == Some(type_annotation.d().annotation.get()) {
                return is_quoted || !require_quoted_annotation;
            }
        }

        if let Some(assignment) = AssignmentNode::cast(cur) {
            if prev_node == assignment.d().annotation_comment.get() {
                // Type comments are always considered forward declarations even though
                // they're not "quoted".
                return true;
            }
        }

        let string_annotation = if let Some(string_list) = StringListNode::cast(cur) {
            if let Some(node_info) = node_info {
                analyzer_node_info::get_string_annotation(string_list, node_info)
            } else {
                get_parser_string_annotation(string_list)
            }
        } else {
            None
        };
        if string_annotation.is_some() && prev_node == string_annotation {
            is_quoted = true;
        }

        if cur.node_type == ParseNodeType::Lambda
            || cur.node_type == ParseNodeType::Function
            || cur.node_type == ParseNodeType::Class
            || cur.node_type == ParseNodeType::Module
        {
            return false;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    false
}

// analyzer/parseTreeUtils.ts:1510
pub fn is_within_annotation_comment(node: P<ParseNode>) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;

    while let Some(cur) = cur_node {
        if let Some(function_node) = FunctionNode::cast(cur) {
            if prev_node
                == function_node
                    .d()
                    .func_annotation_comment
                    .get()
                    .map(|n| n.node())
            {
                // Type comments are always considered forward declarations even though
                // they're not "quoted".
                return true;
            }
        }

        if let Some(assignment) = AssignmentNode::cast(cur) {
            if prev_node == assignment.d().annotation_comment.get() {
                // Type comments are always considered forward declarations even though
                // they're not "quoted".
                return true;
            }
        }

        if cur.node_type == ParseNodeType::Lambda
            || cur.node_type == ParseNodeType::Function
            || cur.node_type == ParseNodeType::Class
            || cur.node_type == ParseNodeType::Module
        {
            return false;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    false
}

// analyzer/parseTreeUtils.ts:1543
pub fn is_within_loop(node: P<ParseNode>) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        match cur.node_type {
            ParseNodeType::For | ParseNodeType::While => {
                return true;
            }

            // The TypeScript `break` leaves only the switch.
            ParseNodeType::Module => {}

            _ => {}
        }

        cur_node = cur.parent.get();
    }

    false
}

// analyzer/parseTreeUtils.ts:1564
pub fn is_within_assert_expression(node: P<ParseNode>) -> bool {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;

    while let Some(cur) = cur_node {
        if let Some(assert_node) = AssertNode::cast(cur) {
            return Some(assert_node.d().test_expr.get()) == prev_node;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    false
}

/// The docstring is an unescaped string value, so it is a `JsString`.
// analyzer/parseTreeUtils.ts:1582
pub fn get_doc_string(statements: &[StatementNode]) -> Option<JsString> {
    // See if the first statement in the suite is a triple-quote string.
    if statements.len() == 0 {
        return None;
    }

    let statement_list = StatementListNode::cast(statements[0])?;

    if !is_doc_string(statement_list) {
        return None;
    }

    // It's up to the user to convert normalize/convert this as needed.
    let first_statement = statement_list.d().statements.borrow()[0];
    let strings = StringListNode(first_statement).d().strings.borrow().clone();
    if strings.len() == 1 {
        return Some(StringNode(strings[0]).d().value.clone());
    }

    Some(
        strings
            .iter()
            .flat_map(|s| StringNode(*s).d().value.units().iter().copied())
            .collect(),
    )
}

// analyzer/parseTreeUtils.ts:1605
pub fn is_doc_string(statement_list: StatementListNode) -> bool {
    // If the first statement in the suite isn't a StringNode,
    // assume there is no docString.
    let statements = statement_list.d().statements.borrow().clone();
    if statements.len() == 0 || statements[0].node_type != ParseNodeType::StringList {
        return false;
    }

    // A docstring can consist of multiple joined strings in a single expression.
    let strings = StringListNode(statements[0]).d().strings.borrow().clone();
    if strings.len() == 0 {
        return false;
    }

    // Any f-strings invalidate the entire docstring.
    if strings
        .iter()
        .any(|n| n.node_type == ParseNodeType::FormatString)
    {
        return false;
    }

    // It's up to the user to convert normalize/convert this as needed.
    true
}

// Sometimes a NamedTuple assignment statement is followed by a statement
// that looks like the following:
//    MyNamedTuple.__new__.__defaults__ = ...
// This pattern is commonly used to set the default values that are
// not specified in the original list.
// analyzer/parseTreeUtils.ts:1635
pub fn is_assignment_to_defaults_following_named_tuple(call_node: P<ParseNode>) -> bool {
    if call_node.node_type != ParseNodeType::Call {
        return false;
    }
    let Some(assignment) = call_node.parent.get().and_then(AssignmentNode::cast) else {
        return false;
    };
    if assignment.d().left_expr.get().node_type != ParseNodeType::Name {
        return false;
    }
    let Some(statement_list) = assignment.parent.get().and_then(StatementListNode::cast) else {
        return false;
    };

    let named_tuple_assigned_name = NameNode(assignment.d().left_expr.get()).d().value();
    if statement_list.d().statements.borrow().first().copied() != Some(assignment.node()) {
        return false;
    }
    let Some(module_or_suite) = statement_list.parent.get() else {
        return false;
    };
    if !(module_or_suite.node_type == ParseNodeType::Module
        || module_or_suite.node_type == ParseNodeType::Suite)
    {
        return false;
    }

    let statements = match module_or_suite.d() {
        ParseNodeDetails::Module(d) => d.statements.borrow().clone(),
        ParseNodeDetails::Suite(d) => d.statements.borrow().clone(),
        _ => unreachable!(),
    };
    let mut statement_index = statements
        .iter()
        .position(|s| *s == statement_list)
        .map_or(-1, |index| index as i32);

    if statement_index < 0 {
        return false;
    }
    statement_index += 1;

    while (statement_index as usize) < statements.len() {
        let Some(next_statement) = StatementListNode::cast(statements[statement_index as usize])
        else {
            break;
        };

        let first_statement = next_statement.d().statements.borrow().first().copied();
        if first_statement.map(|s| s.node_type) == Some(ParseNodeType::StringList) {
            // Skip over comments
            statement_index += 1;
            continue;
        }

        if let Some(assign_node) = first_statement.and_then(AssignmentNode::cast) {
            if let Some(left_expr) = MemberAccessNode::cast(assign_node.d().left_expr.get()) {
                if left_expr.d().member.get().d().value() == "__defaults__" {
                    let default_target = left_expr.d().left_expr.get();
                    if let Some(default_target) = MemberAccessNode::cast(default_target) {
                        if default_target.d().member.get().d().value() == "__new__" {
                            if let Some(name) = NameNode::cast(default_target.d().left_expr.get()) {
                                if name.d().value() == named_tuple_assigned_name {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }

        break;
    }

    false
}

// This simple parse tree walker calls a callback function
// for each NameNode it encounters.
// analyzer/parseTreeUtils.ts:1706
pub struct NameNodeWalker<F: FnMut(NameNode, Option<i32>, Option<ExpressionNode>)> {
    _subscript_index: Option<i32>,
    _base_expression: Option<ExpressionNode>,
    _callback: F,
}

impl<F: FnMut(NameNode, Option<i32>, Option<ExpressionNode>)> NameNodeWalker<F> {
    // analyzer/parseTreeUtils.ts:1710
    pub fn new(_callback: F) -> NameNodeWalker<F> {
        NameNodeWalker {
            _subscript_index: None,
            _base_expression: None,
            _callback,
        }
    }
}

impl<F: FnMut(NameNode, Option<i32>, Option<ExpressionNode>)> ParseTreeVisitor<bool>
    for NameNodeWalker<F>
{
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/parseTreeUtils.ts:1720
    fn visit_name(&mut self, node: NameNode) -> bool {
        (self._callback)(node, self._subscript_index, self._base_expression);
        true
    }

    // analyzer/parseTreeUtils.ts:1725
    fn visit_index(&mut self, node: IndexNode) -> bool {
        self.walk(node.d().left_expr.get());

        let prev_subscript_index = self._subscript_index;
        let prev_base_expression = self._base_expression;
        self._base_expression = Some(node.d().left_expr.get());

        let items = node.d().items.borrow().clone();
        for (index, item) in items.iter().enumerate() {
            self._subscript_index = Some(index as i32);
            self.walk(item.node());
        }

        self._subscript_index = prev_subscript_index;
        self._base_expression = prev_base_expression;

        false
    }
}

impl<F: FnMut(NameNode, Option<i32>, Option<ExpressionNode>)> ParseTreeWalker
    for NameNodeWalker<F>
{
}

// analyzer/parseTreeUtils.ts:1744
pub struct CallNodeWalker<F: FnMut(CallNode)> {
    _callback: F,
}

impl<F: FnMut(CallNode)> CallNodeWalker<F> {
    // analyzer/parseTreeUtils.ts:1745
    pub fn new(_callback: F) -> CallNodeWalker<F> {
        CallNodeWalker { _callback }
    }
}

impl<F: FnMut(CallNode)> ParseTreeVisitor<bool> for CallNodeWalker<F> {
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/parseTreeUtils.ts:1749
    fn visit_call(&mut self, node: CallNode) -> bool {
        (self._callback)(node);
        true
    }
}

impl<F: FnMut(CallNode)> ParseTreeWalker for CallNodeWalker<F> {}

// analyzer/parseTreeUtils.ts:1755
pub fn get_enclosing_param(node: P<ParseNode>) -> Option<ParameterNode> {
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        if let Some(param) = ParameterNode::cast(cur) {
            return Some(param);
        }

        if cur.node_type == ParseNodeType::Function {
            return None;
        }

        cur_node = cur.parent.get();
    }

    None
}

// analyzer/parseTreeUtils.ts:1773
pub fn get_call_node_and_active_param_index(
    node: P<ParseNode>,
    insertion_offset: i32,
    tokens: &TextRangeCollection<P<Token>>,
) -> Option<CallNodeAndActiveParamIndex> {
    // Find the call node that contains the specified node.
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut call_node: Option<CallNode> = None;

    while let Some(cur) = cur_node {
        // make sure we only look at callNodes when we are inside their arguments
        if let Some(cur) = CallNode::cast(cur) {
            if is_offset_inside_call_args(tokens, cur, insertion_offset) {
                call_node = Some(cur);
                break;
            }
        }
        cur_node = cur.parent.get();
    }

    // `!callNode.d.args` never holds: `args` is always an array.
    let call_node = call_node?;

    let end_position = TextRange::get_end(&call_node);
    if insertion_offset > end_position {
        return None;
    }

    let token_at_end = get_token_at(tokens, end_position - 1);
    if insertion_offset == end_position
        && token_at_end.map(|t| t.type_) == Some(TokenType::CloseParenthesis)
    {
        return None;
    }

    let mut added_active = false;
    let mut active_index = -1;
    let mut active_or_fake = false;
    let args = call_node.d().args.borrow().clone();
    for (index, arg) in args.iter().enumerate() {
        if added_active {
            continue;
        }

        // Calculate the argument's bounds including whitespace and colons.
        let mut start = arg.start.get();
        let start_token_index = tokens.get_item_at_position(start);
        if start_token_index >= 0 {
            start = TextRange::get_end(&tokens.get_item_at(start_token_index - 1));
        }

        let mut end = TextRange::get_end(arg);
        let end_token_index = tokens.get_item_at_position(end);
        if end_token_index >= 0 {
            // Find the true end of the argument by searching for the
            // terminating comma or parenthesis.
            for i in end_token_index..tokens.count() {
                let tok = tokens.get_item_at(i);

                match tok.type_ {
                    TokenType::Comma | TokenType::CloseParenthesis => {}
                    _ => continue,
                }

                end = TextRange::get_end(&tok);
                break;
            }
        }

        // If no terminating comma or close paren was found (e.g., an incomplete
        // call with no closing parenthesis), extend end past the call boundary so
        // the argument is still considered active at the cursor position.
        if end == TextRange::get_end(arg) {
            end = end_position + 1;
        }

        if insertion_offset < end {
            active_index = index as i32;
            active_or_fake = insertion_offset >= start;
            added_active = true;
        }
    }

    if !added_active {
        active_index = call_node.d().args.borrow().len() as i32 + 1;
    }

    return Some(CallNodeAndActiveParamIndex {
        call_node,
        active_index,
        active_or_fake,
    });

    // analyzer/parseTreeUtils.ts:1867
    fn is_offset_inside_call_args(
        tokens: &TextRangeCollection<P<Token>>,
        node: CallNode,
        offset: i32,
    ) -> bool {
        let left_expr = node.d().left_expr.get();
        let argument_start = if left_expr.length.get() > 0 {
            TextRange::get_end(&left_expr) - 1
        } else {
            left_expr.start.get()
        };

        // Handle obvious case first.
        let call_end_offset = TextRange::get_end(&node);
        if offset < argument_start || call_end_offset < offset {
            return false;
        }

        let args = node.d().args.borrow().clone();
        if args.len() > 0 {
            let start = args[0].start.get();
            let end = TextRange::get_end(&args[args.len() - 1]);
            if start <= offset && offset < end {
                return true;
            }
        }

        let index = tokens.get_item_at_position(argument_start);
        if index < 0 || tokens.count() <= index {
            return true;
        }

        let next_token = tokens.get_item_at(index + 1);
        if next_token.type_ == TokenType::OpenParenthesis
            && offset < TextRange::get_end(&next_token)
        {
            // Position must be after '('.
            return false;
        }

        true
    }
}

/// TypeScript's defaults are `include_whitespace = false` and
/// `include_zero_length_token = false`.
// analyzer/parseTreeUtils.ts:1900
pub fn get_token_index_at_left(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
    include_whitespace: bool,
    include_zero_length_token: bool,
) -> i32 {
    let index = tokens.get_item_at_position(position);
    if index < 0 {
        return -1;
    }

    for i in (0..=index).rev() {
        let token = tokens.get_item_at(i);
        if !include_zero_length_token && token.length == 0 {
            continue;
        }

        if !include_whitespace && Tokenizer::is_whitespace(&token) {
            continue;
        }

        if TextRange::get_end(&token) <= position {
            return i;
        }
    }

    -1
}

/// TypeScript's defaults are `include_whitespace = false` and
/// `include_zero_length_token = false`.
// analyzer/parseTreeUtils.ts:1929
pub fn get_token_at_left(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
    include_whitespace: bool,
    include_zero_length_token: bool,
) -> Option<P<Token>> {
    let index = get_token_index_at_left(
        tokens,
        position,
        include_whitespace,
        include_zero_length_token,
    );
    if index < 0 {
        return None;
    }

    Some(tokens.get_item_at(index))
}

// analyzer/parseTreeUtils.ts:1943
pub fn get_token_index_after(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
    predicate: &dyn Fn(P<Token>) -> bool,
) -> i32 {
    let index = tokens.get_item_at_position(position);
    if index < 0 {
        return -1;
    }

    // `tokens.length` is the length of the text the tokens span, not the
    // token count.
    for i in index..tokens.length() {
        let token = tokens.get_item_at(i);
        if predicate(token) {
            return i;
        }
    }

    -1
}

// analyzer/parseTreeUtils.ts:1963
pub fn get_token_after(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
    predicate: &dyn Fn(P<Token>) -> bool,
) -> Option<P<Token>> {
    let index = get_token_index_after(tokens, position, predicate);
    if index < 0 {
        return None;
    }

    Some(tokens.get_item_at(index))
}

// analyzer/parseTreeUtils.ts:1972
pub fn get_token_at_index(tokens: &TextRangeCollection<P<Token>>, index: i32) -> Option<P<Token>> {
    if index < 0 {
        return None;
    }

    Some(tokens.get_item_at(index))
}

// analyzer/parseTreeUtils.ts:1980
pub fn get_token_at(tokens: &TextRangeCollection<P<Token>>, position: i32) -> Option<P<Token>> {
    get_token_at_index(tokens, tokens.get_item_at_position(position))
}

// analyzer/parseTreeUtils.ts:1984
pub fn get_token_overlapping(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
) -> Option<P<Token>> {
    let index = get_index_of_token_overlapping(tokens, position);
    get_token_at_index(tokens, index)
}

// analyzer/parseTreeUtils.ts:1989
pub fn get_index_of_token_overlapping(
    tokens: &TextRangeCollection<P<Token>>,
    position: i32,
) -> i32 {
    let index = tokens.get_item_at_position(position);
    if index < 0 {
        return -1;
    }

    let token = tokens.get_item_at(index);

    if TextRange::overlaps(&token, position) {
        index
    } else {
        -1
    }
}

/// `None` is both TypeScript `undefined` results: no token at `index`, and a
/// token without comments.
// analyzer/parseTreeUtils.ts:2000
pub fn get_comments_at_token_index(
    tokens: &TextRangeCollection<P<Token>>,
    index: i32,
) -> Option<&'static [Comment]> {
    let mut token = get_token_at_index(tokens, index)?;

    // If the preceding token has the same start offset
    // (in other words, when tokens have zero length and they're piled on top of each other)
    // look back through the tokens until we find the first token with that start offset.
    // That's where the comments (if any) will be.
    let mut preceding_index = index - 1;
    while preceding_index >= 0 {
        let preceding_token = get_token_at_index(tokens, preceding_index);
        if let Some(preceding_token) = preceding_token.filter(|t| t.start == token.start) {
            token = preceding_token;
        } else {
            break;
        }
        preceding_index -= 1;
    }

    token.comments()
}

// analyzer/parseTreeUtils.ts:2022
pub fn print_parse_node_type(type_: ParseNodeType) -> &'static str {
    // `parse_node_type_name_map` returns "" where the map has no entry.
    let name = parse_node_type_name_map(type_);
    if name.is_empty() { "Unknown" } else { name }
}

// analyzer/parseTreeUtils.ts:2026
pub fn is_write_access(node: NameNode) -> bool {
    let mut prev_node: P<ParseNode> = node.node();
    let mut cur_node: Option<P<ParseNode>> = prev_node.parent.get();

    while let Some(cur) = cur_node {
        match cur.d() {
            ParseNodeDetails::Assignment(d) => {
                return prev_node == d.left_expr.get();
            }

            ParseNodeDetails::AugmentedAssignment(d) => {
                return prev_node == d.left_expr.get();
            }

            ParseNodeDetails::AssignmentExpression(d) => {
                return prev_node == d.name.get();
            }

            ParseNodeDetails::Del(_) => {
                return true;
            }

            ParseNodeDetails::For(d) => {
                return prev_node == d.target_expr.get();
            }

            ParseNodeDetails::ImportAs(d) => {
                return d.alias.get().is_some_and(|alias| alias == prev_node) || {
                    let name_parts = d.module.get().d().name_parts.borrow();
                    name_parts.len() > 0 && prev_node == name_parts[0]
                };
            }

            ParseNodeDetails::ImportFromAs(d) => {
                return d.alias.get().is_some_and(|alias| alias == prev_node)
                    || (d.alias.get().is_none() && prev_node == d.name.get());
            }

            ParseNodeDetails::MemberAccess(d) => {
                if prev_node != d.member.get() {
                    return false;
                }
            }

            ParseNodeDetails::Except(d) => {
                return d.name.get().is_some_and(|name| name == prev_node);
            }

            ParseNodeDetails::With(d) => {
                return d.with_items.borrow().iter().any(|item| *item == prev_node);
            }

            ParseNodeDetails::ComprehensionFor(d) => {
                return prev_node == d.target_expr.get();
            }

            ParseNodeDetails::TypeAnnotation(d) => {
                if prev_node == d.annotation.get() {
                    return false;
                }
            }

            ParseNodeDetails::Function(_)
            | ParseNodeDetails::Class(_)
            | ParseNodeDetails::Module(_) => {
                return false;
            }

            _ => {}
        }

        prev_node = cur;
        cur_node = cur.parent.get();
    }

    false
}

// analyzer/parseTreeUtils.ts:2103
pub fn get_matching_descendants(
    node: P<ParseNode>,
    match_: &dyn Fn(P<ParseNode>) -> bool,
) -> Vec<P<ParseNode>> {
    let mut matches: Vec<P<ParseNode>> = vec![];
    let children = get_child_nodes(node, None);
    for child in children {
        if let Some(child) = child {
            if match_(child) {
                matches.push(child);
            }
        }
        if let Some(child) = child {
            matches.extend(get_matching_descendants(child, match_));
        }
    }
    matches
}

// analyzer/parseTreeUtils.ts:2117
pub fn get_module_node(node: P<ParseNode>) -> Option<ModuleNode> {
    let mut current: Option<P<ParseNode>> = Some(node);
    while let Some(cur) = current.filter(|cur| cur.node_type != ParseNodeType::Module) {
        current = cur.parent.get();
    }

    current.map(ModuleNode)
}

/// Calls `get_file_info_if_available`: TypeScript's `getFileInfo` asserts
/// non-null only in the type system and returns `undefined` at runtime, where
/// the Rust `get_file_info` panics.
// analyzer/parseTreeUtils.ts:2126
pub fn get_file_info_from_node(
    node: P<ParseNode>,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<P<AnalyzerFileInfo>> {
    let current = get_module_node(node);
    current.and_then(|current| analyzer_node_info::get_file_info_if_available(current, reader))
}

// analyzer/parseTreeUtils.ts:2131
pub fn is_function_suite_empty(node: FunctionNode) -> bool {
    let mut is_empty = true;

    let statements = node.d().suite.get().d().statements.borrow().clone();
    for statement in statements {
        if statement.node_type == ParseNodeType::Error {
            continue;
        } else if let Some(statement) = StatementListNode::cast(statement) {
            for sub_statement in statement.d().statements.borrow().iter() {
                // Allow docstrings, ellipsis, and pass statements.
                if sub_statement.node_type != ParseNodeType::Ellipsis
                    && sub_statement.node_type != ParseNodeType::StringList
                    && sub_statement.node_type != ParseNodeType::Pass
                {
                    is_empty = false;
                }
            }
        } else {
            is_empty = false;
        }
    }

    is_empty
}

// analyzer/parseTreeUtils.ts:2156
pub fn get_type_annotation_for_param(
    node: FunctionNode,
    param_index: i32,
) -> Option<ExpressionNode> {
    let params = node.d().params.borrow().clone();
    if param_index >= params.len() as i32 {
        return None;
    }

    let param = params[param_index as usize];
    if let Some(annotation) = param.d().annotation.get() {
        return Some(annotation);
    } else if let Some(annotation_comment) = param.d().annotation_comment.get() {
        return Some(annotation_comment);
    }

    let func_annotation_comment = node
        .d()
        .func_annotation_comment
        .get()
        .filter(|comment| !comment.d().is_ellipsis.get())?;

    let mut first_comment_annotation_index = 0;
    let param_annotations = func_annotation_comment
        .d()
        .param_annotations
        .borrow()
        .clone();
    if param_annotations.len() < params.len() {
        first_comment_annotation_index = 1;
    }

    let adj_index = param_index - first_comment_annotation_index;
    if adj_index < 0 || adj_index >= param_annotations.len() as i32 {
        return None;
    }

    Some(param_annotations[adj_index as usize])
}

// analyzer/parseTreeUtils.ts:2186
pub fn is_import_module_name(node: P<ParseNode>) -> bool {
    get_first_ancestor_or_self_of_kind(Some(node), ParseNodeType::ModuleName)
        .and_then(|module_name| module_name.parent.get())
        .map(|parent| parent.node_type)
        == Some(ParseNodeType::ImportAs)
}

// analyzer/parseTreeUtils.ts:2190
pub fn is_import_alias(node: P<ParseNode>) -> bool {
    node.parent
        .get()
        .and_then(ImportAsNode::cast)
        .is_some_and(|parent| parent.d().alias.get().is_some_and(|alias| alias == node))
}

// analyzer/parseTreeUtils.ts:2194
pub fn is_from_import_module_name(node: P<ParseNode>) -> bool {
    get_first_ancestor_or_self_of_kind(Some(node), ParseNodeType::ModuleName)
        .and_then(|module_name| module_name.parent.get())
        .map(|parent| parent.node_type)
        == Some(ParseNodeType::ImportFrom)
}

// analyzer/parseTreeUtils.ts:2198
pub fn is_from_import_name(node: P<ParseNode>) -> bool {
    node.parent
        .get()
        .and_then(ImportFromAsNode::cast)
        .is_some_and(|parent| parent.d().name.get() == node)
}

// analyzer/parseTreeUtils.ts:2202
pub fn is_from_import_alias(node: P<ParseNode>) -> bool {
    node.parent
        .get()
        .and_then(ImportFromAsNode::cast)
        .is_some_and(|parent| parent.d().alias.get().is_some_and(|alias| alias == node))
}

// analyzer/parseTreeUtils.ts:2206
pub fn is_last_name_of_module_name(node: NameNode) -> bool {
    let Some(module) = node.parent.get().and_then(ModuleNameNode::cast) else {
        return false;
    };

    let name_parts = module.d().name_parts.borrow();
    if name_parts.len() == 0 {
        return false;
    }

    name_parts[name_parts.len() - 1] == node
}

/// The generator becomes a lazy iterator: it reads each `parent` when the
/// caller asks for the next node.
// analyzer/parseTreeUtils.ts:2219
pub fn get_ancestors_including_self(
    node: Option<P<ParseNode>>,
) -> impl Iterator<Item = P<ParseNode>> {
    std::iter::successors(node, |node| node.parent.get())
}

/// The TypeScript return type narrows to the node type of `type_`; the caller
/// casts the result with that type's `cast`.
// analyzer/parseTreeUtils.ts:2228
pub fn get_first_ancestor_or_self_of_kind(
    node: Option<P<ParseNode>>,
    type_: ParseNodeType,
) -> Option<P<ParseNode>> {
    get_first_ancestor_or_self(node, &|n| n.node_type == type_)
}

// analyzer/parseTreeUtils.ts:2235
pub fn get_first_ancestor_or_self(
    node: Option<P<ParseNode>>,
    predicate: &dyn Fn(P<ParseNode>) -> bool,
) -> Option<P<ParseNode>> {
    get_ancestors_including_self(node).find(|&current| predicate(current))
}

/// Returns a `MemberAccessNode` or a `NameNode`.
// analyzer/parseTreeUtils.ts:2248
pub fn get_dotted_name_with_given_node_as_last_name(node: NameNode) -> P<ParseNode> {
    // Shape of dotted name is
    //    MemberAccess (ex, a.b)
    //  Name        Name
    // or
    //           MemberAccess (ex, a.b.c)
    //    MemberAccess     Name
    //  Name       Name
    let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) else {
        return node.node();
    };

    if parent.d().left_expr.get() == node {
        return node.node();
    }

    parent.node()
}

//
// Returns the dotted name that makes up the expression for the decorator.
// Example:
// @pytest.fixture()
// def my_fixture():
//    pass
//
// would return `pytest.fixture`
// analyzer/parseTreeUtils.ts:2275
pub fn get_decorator_name(decorator: DecoratorNode) -> Option<String> {
    // analyzer/parseTreeUtils.ts:2276
    fn get_expression_name(node: ExpressionNode) -> Option<String> {
        if node.node_type == ParseNodeType::Name || node.node_type == ParseNodeType::MemberAccess {
            return get_dotted_name(node).map(|names| {
                names
                    .iter()
                    .map(|n| n.d().value())
                    .collect::<Vec<_>>()
                    .join(".")
            });
        }
        if let Some(call) = CallNode::cast(node) {
            return get_expression_name(call.d().left_expr.get());
        }

        None
    }

    get_expression_name(decorator.d().expr.get())
}

/// `node` is a `MemberAccessNode` or a `NameNode`.
// analyzer/parseTreeUtils.ts:2292
pub fn get_dotted_name(node: P<ParseNode>) -> Option<Vec<NameNode>> {
    // ex) [a] or [a].b
    // simple case, [a]
    if let Some(node) = NameNode::cast(node) {
        return Some(vec![node]);
    }

    // dotted name case.
    let mut names: Vec<NameNode> = vec![];
    if _get_dotted_name(node, &mut names) {
        names.reverse();
        return Some(names);
    }

    return None;

    // analyzer/parseTreeUtils.ts:2307
    fn _get_dotted_name(node: P<ParseNode>, names: &mut Vec<NameNode>) -> bool {
        if let Some(node) = NameNode::cast(node) {
            names.push(node);
            return true;
        }

        let node = MemberAccessNode(node);
        names.push(node.d().member.get());

        let left_expr = node.d().left_expr.get();
        if left_expr.node_type == ParseNodeType::Name
            || left_expr.node_type == ParseNodeType::MemberAccess
        {
            return _get_dotted_name(left_expr, names);
        }

        false
    }
}

/// `node` is a `MemberAccessNode` or a `NameNode`.
// analyzer/parseTreeUtils.ts:2326
pub fn get_first_name_of_dotted_name(node: P<ParseNode>) -> Option<NameNode> {
    // ex) [a] or [a].b
    if let Some(node) = NameNode::cast(node) {
        return Some(node);
    }

    let left_expr = MemberAccessNode(node).d().left_expr.get();
    if left_expr.node_type == ParseNodeType::Name
        || left_expr.node_type == ParseNodeType::MemberAccess
    {
        return get_first_name_of_dotted_name(left_expr);
    }

    None
}

// Returns the MemberAccessNode at or directly enclosing the given node: the node
// itself when it is a member access, otherwise its parent when that is a member
// access. Used by code actions that operate on `a.b` style access.
// analyzer/parseTreeUtils.ts:2342
pub fn get_enclosing_member_access_node(node: Option<P<ParseNode>>) -> Option<MemberAccessNode> {
    let node = node?;

    if let Some(node) = MemberAccessNode::cast(node) {
        return Some(node);
    }

    if let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) {
        return Some(parent);
    }

    None
}

// analyzer/parseTreeUtils.ts:2358
pub fn is_first_name_of_dotted_name(node: NameNode) -> bool {
    // ex) [A] or [A].B.C.D
    let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) else {
        return true;
    };

    if parent.d().left_expr.get() == node {
        return true;
    }

    false
}

// analyzer/parseTreeUtils.ts:2371
pub fn is_last_name_of_dotted_name(node: NameNode) -> bool {
    // ex) A or D.C.B.[A]
    let Some(parent) = node.parent.get().and_then(MemberAccessNode::cast) else {
        return true;
    };

    let left_expr = parent.d().left_expr.get();
    if left_expr.node_type != ParseNodeType::Name
        && left_expr.node_type != ParseNodeType::MemberAccess
    {
        return false;
    }

    if left_expr == node {
        return false;
    }

    parent.parent.get().map(|grandparent| grandparent.node_type)
        != Some(ParseNodeType::MemberAccess)
}

// analyzer/parseTreeUtils.ts:2391
pub fn get_string_node_value_range(node: StringNode) -> TextRange {
    get_string_value_range(node.d().token.get())
}

// analyzer/parseTreeUtils.ts:2395
pub fn get_string_value_range(token: StringToken) -> TextRange {
    let length = token.d().quote_mark_length;
    let has_ending = !token.d().flags.any(StringTokenFlags::Unterminated);
    TextRange::create(
        token.start + length,
        token.length - length - if has_ending { length } else { 0 },
    )
}

/// `None` for `include_trailing_blank_lines` is TypeScript's omitted `options`.
// analyzer/parseTreeUtils.ts:2401
pub fn get_full_statement_range(
    statement_node: P<ParseNode>,
    parse_file_results: &ParseFileResults,
    include_trailing_blank_lines: Option<bool>,
) -> Range {
    let range = convert_text_range_to_range(
        &statement_node.text_range(),
        &parse_file_results.tokenizer_output.lines,
    );

    let start = _get_start_position_if_multiple_statements_are_on_same_line(
        &range,
        statement_node.start.get(),
        &parse_file_results.tokenizer_output,
    )
    .unwrap_or(Position {
        line: range.start.line,
        character: 0,
    });

    // First, see whether there are other tokens except semicolon or new line on the same line.
    let end = _get_end_position_if_multiple_statements_are_on_same_line(
        &range,
        TextRange::get_end(&statement_node),
        &parse_file_results.tokenizer_output,
    );

    if let Some(end) = end {
        return Range { start, end };
    }

    // If not, delete the whole line.
    if range.end.line == parse_file_results.tokenizer_output.lines.count() - 1 {
        return Range {
            start,
            end: range.end,
        };
    }

    let mut line_delta_to_add = 1;
    if let Some(include_trailing_blank_lines) = include_trailing_blank_lines {
        if include_trailing_blank_lines {
            let mut i = line_delta_to_add;
            while range.end.line + i < parse_file_results.tokenizer_output.lines.count() {
                if !is_blank_line(
                    &parse_file_results.tokenizer_output,
                    &parse_file_results.text,
                    range.end.line + i,
                ) {
                    line_delta_to_add = i;
                    break;
                }
                i += 1;
            }
        }
    }

    Range {
        start,
        end: Position {
            line: range.end.line + line_delta_to_add,
            character: 0,
        },
    }
}

// analyzer/parseTreeUtils.ts:2448
pub fn is_blank_line(tokenizer_output: &TokenizerOutput, text: &[u16], line: i32) -> bool {
    let span = tokenizer_output.lines.get_item_at(line);
    // A lone surrogate decodes to U+FFFD, which keeps the UTF-16 offsets and is
    // not whitespace either.
    contains_only_whitespace(
        &String::from_utf16_lossy(text),
        Some(span.start),
        Some(TextRange::get_end(&span)),
    )
}

// analyzer/parseTreeUtils.ts:2453
pub fn is_unannotated_function(node: FunctionNode) -> bool {
    node.d().return_annotation.get().is_none()
        && node.d().params.borrow().iter().all(|param| {
            param.d().annotation.get().is_none() && param.d().annotation_comment.get().is_none()
        })
}

// Verifies that an import of the form "from __future__ import x"
// occurs only at the top of a file. This mirrors the algorithm used
// in the CPython interpreter.
// analyzer/parseTreeUtils.ts:2463
pub fn is_valid_location_for_future_import(node: ImportFromNode) -> bool {
    let module = get_module_node(node.node());
    assert(module.is_some(), None);
    let module = module.unwrap();

    let mut saw_doc_string = false;

    let statements = module.d().statements.borrow().clone();
    for statement in statements {
        let Some(statement) = StatementListNode::cast(statement) else {
            return false;
        };

        let simple_statements = statement.d().statements.borrow().clone();
        for simple_statement in simple_statements {
            if simple_statement == node {
                return true;
            }

            if simple_statement.node_type == ParseNodeType::StringList {
                if saw_doc_string {
                    return false;
                }
                saw_doc_string = true;
            } else if let Some(simple_statement) = ImportFromNode::cast(simple_statement) {
                let module_name = simple_statement.d().module.get();
                let name_parts = module_name.d().name_parts.borrow();
                if module_name.d().leading_dots.get() != 0
                    || name_parts.len() != 1
                    || name_parts[0].d().value() != "__future__"
                {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    false
}

// "Chaining" is when binary operators can be chained together
// as a shorthand. For example, "a < b < c" is shorthand for
// "a < b and b < c".
// analyzer/parseTreeUtils.ts:2504
pub fn operator_supports_chaining(op: OperatorType) -> bool {
    match op {
        OperatorType::Equals
        | OperatorType::NotEquals
        | OperatorType::LessThan
        | OperatorType::LessThanOrEqual
        | OperatorType::GreaterThan
        | OperatorType::GreaterThanOrEqual
        | OperatorType::Is
        | OperatorType::IsNot
        | OperatorType::In
        | OperatorType::NotIn => {
            return true;
        }
        _ => {}
    }

    false
}

// If the statement is a part of multiple statements on the same line
// and the statement is not the first statement on the line, then it will return
// appropriate start position. otherwise, return undefined.
// ex) a = 1; [|b = 1|]
// analyzer/parseTreeUtils.ts:2526
fn _get_start_position_if_multiple_statements_are_on_same_line(
    range: &Range,
    token_position: i32,
    tokenizer_output: &TokenizerOutput,
) -> Option<Position> {
    let token_index = tokenizer_output.tokens.get_item_at_position(token_position);
    if token_index < 0 {
        return None;
    }

    // Find the last token index on the previous line or the first token.
    let mut current_index = token_index;
    while current_index > 0 {
        let token = tokenizer_output.tokens.get_item_at(current_index);
        let token_range = convert_text_range_to_range(&token.text_range(), &tokenizer_output.lines);
        if token_range.end.line != range.start.line {
            break;
        }
        current_index -= 1;
    }

    // Find the previous token of the first token of the statement.
    for index in (current_index + 1..token_index).rev() {
        let token = tokenizer_output.tokens.get_item_at(index);

        // Eat up indentation
        if token.type_ == TokenType::Indent || token.type_ == TokenType::Dedent {
            continue;
        }

        // If previous token is new line, use default.
        if token.type_ == TokenType::NewLine {
            return None;
        }

        // Anything else (ex, semicolon), use statement start as it is.
        return Some(range.start);
    }

    None
}

// If the statement is a part of multiple statements on the same line
// and the statement is not the last statement on the line, then it will return
// appropriate end position. otherwise, return undefined.
// ex) [|a = 1|]; b = 1
// analyzer/parseTreeUtils.ts:2571
fn _get_end_position_if_multiple_statements_are_on_same_line(
    range: &Range,
    token_position: i32,
    tokenizer_output: &TokenizerOutput,
) -> Option<Position> {
    let token_index = tokenizer_output.tokens.get_item_at_position(token_position);
    if token_index < 0 {
        return None;
    }

    // Find the first token index on the next line or the last token.
    let mut current_index = token_index;
    while current_index < tokenizer_output.tokens.count() {
        let token = tokenizer_output.tokens.get_item_at(current_index);
        let token_range = convert_text_range_to_range(&token.text_range(), &tokenizer_output.lines);
        if range.end.line != token_range.start.line {
            break;
        }
        current_index += 1;
    }

    // Find the next token of the last token of the statement.
    let mut found_statement_end = false;
    for index in token_index..current_index {
        let token = tokenizer_output.tokens.get_item_at(index);

        // Eat up semicolon or new line.
        if token.type_ == TokenType::Semicolon || token.type_ == TokenType::NewLine {
            found_statement_end = true;
            continue;
        }

        if !found_statement_end {
            continue;
        }

        let token_range = convert_text_range_to_range(&token.text_range(), &tokenizer_output.lines);
        return Some(token_range.start);
    }

    None
}

// analyzer/parseTreeUtils.ts:2613
pub fn get_variable_doc_string_node(node: ExpressionNode) -> Option<StringListNode> {
    // Walk up the parse tree to find an assignment or type alias statement.
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut annotation_node: Option<TypeAnnotationNode> = None;

    while let Some(cur) = cur_node {
        if cur.node_type == ParseNodeType::Assignment {
            break;
        }

        if cur.node_type == ParseNodeType::TypeAlias {
            break;
        }

        if cur.node_type == ParseNodeType::Suite {
            break;
        }

        if let Some(type_annotation) = TypeAnnotationNode::cast(cur) {
            if annotation_node.is_none() {
                annotation_node = Some(type_annotation);
            }
        }

        cur_node = cur.parent.get();
    }

    let mut cur_node: P<ParseNode> = match cur_node {
        Some(cur)
            if cur.node_type == ParseNodeType::Assignment
                || cur.node_type == ParseNodeType::TypeAlias =>
        {
            cur
        }
        // Allow a simple annotation statement to have a docstring even
        // though PEP 258 doesn't mention this case. This PEP pre-dated
        // PEP 526, so it didn't contemplate this situation.
        _ => annotation_node?.node(),
    };

    // Chained assignments (e.g. `a = b = c = value`) parse into nested Assignment
    // nodes where only the outermost node's parent is the StatementList that can
    // hold the trailing PEP 258 attribute docstring. Walk up through the chain so
    // every target (a, b, c) resolves the same docstring. This is a no-op for
    // non-chained assignments.
    while cur_node.node_type == ParseNodeType::Assignment {
        let Some(parent) = cur_node
            .parent
            .get()
            .filter(|parent| parent.node_type == ParseNodeType::Assignment)
        else {
            break;
        };
        cur_node = parent;
    }

    let parent_node = cur_node.parent.get().and_then(StatementListNode::cast)?;

    let suite_or_module = parent_node.parent.get().filter(|suite_or_module| {
        suite_or_module.node_type == ParseNodeType::Module
            || suite_or_module.node_type == ParseNodeType::Suite
    })?;

    let statements = match suite_or_module.d() {
        ParseNodeDetails::Module(d) => d.statements.borrow().clone(),
        ParseNodeDetails::Suite(d) => d.statements.borrow().clone(),
        _ => unreachable!(),
    };
    let assignment_index = statements
        .iter()
        .position(|node| *node == parent_node)
        .map_or(-1, |index| index as i32);
    if assignment_index < 0 || assignment_index == statements.len() as i32 - 1 {
        return None;
    }

    let next_statement = statements[(assignment_index + 1) as usize];

    let next_statement = StatementListNode::cast(next_statement)
        .filter(|next_statement| is_doc_string(*next_statement))?;

    // See if the assignment is within one of the contexts specified in PEP 258.
    let mut is_valid_context = false;
    let parent_of_parent_node = parent_node.parent.get();
    if parent_of_parent_node.map(|n| n.node_type) == Some(ParseNodeType::Module) {
        // If we're at the top level of a module, the attribute docstring is valid.
        is_valid_context = true;
    } else if parent_of_parent_node.map(|n| n.node_type) == Some(ParseNodeType::Suite)
        && parent_of_parent_node
            .and_then(|n| n.parent.get())
            .map(|n| n.node_type)
            == Some(ParseNodeType::Class)
    {
        // If we're at the top level of a class, the attribute docstring is valid.
        is_valid_context = true;
    } else {
        let func = get_enclosing_function(parent_node.node());

        // If we're within an __init__ method, the attribute docstring is valid.
        if let Some(func) = func {
            if func.d().name.get().d().value() == "__init__"
                && get_enclosing_class(func.node(), /* stopAtFunction */ true).is_some()
            {
                is_valid_context = true;
            }
        }
    }

    if !is_valid_context {
        return None;
    }

    // A docstring can consist of multiple joined strings in a single expression.
    Some(StringListNode(next_statement.d().statements.borrow()[0]))
}

// Creates an ID that identifies this parse node in a way that will
// not change each time the file is parsed (unless, of course, the
// file contents change).
// analyzer/parseTreeUtils.ts:2713
pub fn get_scope_id_for_node(node: P<ParseNode>, node_info: &dyn AnalyzerNodeInfoReader) -> TypeVarScopeId {
    // The id depends only on the node's file, start and name, which never change after the
    // parse, so the port remembers it per node; pyright builds it each time
    // (`PYRIGHT_RS_SCOPE_ID_CACHE`).
    thread_local! {
        static SCOPE_IDS: std::cell::RefCell<jsrt::FxHashMap<P<ParseNode>, TypeVarScopeId>> =
            std::cell::RefCell::new(jsrt::FxHashMap::default());
    }
    let mode = scope_id_cache_mode();
    if mode == ScopeIdCacheMode::Off {
        return build_scope_id_for_node(node, node_info);
    }
    match SCOPE_IDS.with_borrow(|ids| ids.get(&node).copied()) {
        Some(scope_id) => {
            if mode == ScopeIdCacheMode::Check {
                let built = build_scope_id_for_node(node, node_info);
                assert!(scope_id == built, "PYRIGHT_RS_SCOPE_ID_CACHE=check: cached {scope_id:?}, built {built:?}");
            }
            scope_id
        }
        None => {
            let scope_id = build_scope_id_for_node(node, node_info);
            SCOPE_IDS.with_borrow_mut(|ids| ids.insert(node, scope_id));
            scope_id
        }
    }
}

/// `PYRIGHT_RS_SCOPE_ID_CACHE`: `on` (the default) remembers each node's scope id, `off`
/// builds it on every call, and `check` builds it on every reuse and panics when it differs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScopeIdCacheMode {
    Off,
    On,
    Check,
}

fn scope_id_cache_mode() -> ScopeIdCacheMode {
    static MODE: std::sync::OnceLock<ScopeIdCacheMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("PYRIGHT_RS_SCOPE_ID_CACHE").as_deref() {
        Err(_) | Ok("") | Ok("on") => ScopeIdCacheMode::On,
        Ok("off") => ScopeIdCacheMode::Off,
        Ok("check") => ScopeIdCacheMode::Check,
        Ok(other) => panic!("PYRIGHT_RS_SCOPE_ID_CACHE must be on, off or check, not {other:?}"),
    })
}

/// The body of `getScopeIdForNode`: the text is built in one buffer per thread and interned.
fn build_scope_id_for_node(node: P<ParseNode>, node_info: &dyn AnalyzerNodeInfoReader) -> TypeVarScopeId {
    let mut name: &str = "";
    if let Some(class_node) = ClassNode::cast(node) {
        name = class_node.d().name.get().d().value();
    } else if let Some(function_node) = FunctionNode::cast(node) {
        name = function_node.d().name.get().d().value();
    }

    let file_info = analyzer_node_info::get_file_info(node, node_info);
    thread_local! {
        static SCOPE_ID: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    SCOPE_ID.with_borrow_mut(|scope_id| {
        scope_id.clear();
        scope_id.push_str(&file_info.file_id);
        scope_id.push('.');
        jsrt::number::push_integer(scope_id, node.start.get().into());
        scope_id.push('-');
        scope_id.push_str(name);
        TypeVarScopeId::new(scope_id)
    })
}

// Walks up the parse tree and finds all scopes that can provide
// a context for a TypeVar and returns the scope ID for each.
// analyzer/parseTreeUtils.ts:2727
pub fn get_type_var_scopes_for_node(
    node: P<ParseNode>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Vec<TypeVarScopeId> {
    let mut scope_ids: Vec<TypeVarScopeId> = vec![];

    let mut cur_node: Option<P<ParseNode>> = Some(node);
    while let Some(cur) = cur_node {
        let Some(scope_node) = get_type_var_scope_node(cur) else {
            break;
        };

        scope_ids.push(get_scope_id_for_node(scope_node, node_info));
        cur_node = scope_node.parent.get();
    }

    scope_ids
}

// analyzer/parseTreeUtils.ts:2747
pub fn check_decorator(node: DecoratorNode, value: &str) -> bool {
    NameNode::cast(node.d().expr.get()).is_some_and(|expr| expr.d().value() == value)
}

// analyzer/parseTreeUtils.ts:2751
pub fn is_simple_default(node: ExpressionNode) -> bool {
    match node.d() {
        ParseNodeDetails::Number(_)
        | ParseNodeDetails::Constant(_)
        | ParseNodeDetails::MemberAccess(_) => true,

        ParseNodeDetails::String(d) => {
            (d.token.get().d().flags & (StringTokenFlags::Format | StringTokenFlags::Template)).0
                == 0
        }

        ParseNodeDetails::StringList(d) => {
            let strings = d.strings.borrow().clone();
            strings.iter().all(|s| is_simple_default(*s))
        }

        ParseNodeDetails::UnaryOperation(d) => is_simple_default(d.expr.get()),

        ParseNodeDetails::BinaryOperation(d) => {
            is_simple_default(d.left_expr.get()) && is_simple_default(d.right_expr.get())
        }

        _ => false,
    }
}

// analyzer/parseTreeUtils.ts:2775
pub fn get_previous_non_whitespace_token(
    tokens: &TextRangeCollection<P<Token>>,
    offset: i32,
) -> Option<P<Token>> {
    let mut token_index = tokens.get_item_at_position(offset);

    while token_index >= 0 {
        let token = tokens.get_item_at(token_index);
        if !Tokenizer::is_whitespace(&token) {
            return Some(token);
        }

        token_index -= 1;
    }

    None
}

// analyzer/parseTreeUtils.ts:2790
pub fn get_next_non_whitespace_token(
    tokens: &TextRangeCollection<P<Token>>,
    offset: i32,
) -> Option<P<Token>> {
    get_next_matching_token(
        tokens,
        offset,
        &|token| !Tokenizer::is_whitespace(&token),
        None,
    )
}

/// `None` for `exit` is TypeScript's default `() => false`.
// analyzer/parseTreeUtils.ts:2794
pub fn get_next_matching_token(
    tokens: &TextRangeCollection<P<Token>>,
    offset: i32,
    match_: &dyn Fn(P<Token>) -> bool,
    exit: Option<&dyn Fn(P<Token>) -> bool>,
) -> Option<P<Token>> {
    let default_exit = |_: P<Token>| false;
    let exit: &dyn Fn(P<Token>) -> bool = exit.unwrap_or(&default_exit);

    let mut token_index = tokens.get_item_at_position(offset) + 1;
    while token_index < tokens.count() {
        let token = tokens.get_item_at(token_index);
        if match_(token) {
            return Some(token);
        }
        if exit(token) {
            return None;
        }
        token_index += 1;
    }

    None
}

#[cfg(test)]
#[path = "parse_tree_utils_tests.rs"]
mod tests;
