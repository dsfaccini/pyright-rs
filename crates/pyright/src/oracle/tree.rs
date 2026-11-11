//! Rust side of the `tree` dump stage. Format: `oracle/FORMAT.md`.
//!
//! `dump_parser_output` writes a whole tree-stage file: the `ParserOutput`
//! without `lines` and `stringAnnotations`, the parse tree nested by
//! `getChildNodes` with the parser's string annotations.

use jsrt::{NumberValue, P};

use super::format::{self, DumpWriter};
use super::tokens::write_token;
use crate::parser::parse_node_utils::parse_node_type_name_map;
use crate::parser::parse_nodes::{
    ParseNode, ParseNodeDetails, ParseNodeType, StringListNode, get_parser_string_annotation,
};
use crate::parser::parse_tree_utils::get_child_nodes;
use crate::parser::parser::ParserOutput;
use crate::parser::tokenizer_types::Token;

/// One `d` field of a parse node, by the kind of value it prints as.
/// The `Opt` variants print nothing for `None` (an `undefined` field).
enum Field<'a> {
    Node(P<ParseNode>),
    OptNode(Option<P<ParseNode>>),
    Nodes(Vec<P<ParseNode>>),
    OptNodes(Option<Vec<P<ParseNode>>>),
    Token(P<Token>),
    OptToken(Option<P<Token>>),
    Tokens(Vec<P<Token>>),
    Bool(bool),
    OptBool(Option<bool>),
    Int(i32),
    OptInt(Option<i32>),
    Str(&'a str),
    JsStr(&'a [u16]),
    Number(&'a NumberValue),
}

/// A typed node handle or a `P<ParseNode>`, as a `P<ParseNode>`.
fn as_node(node: impl Into<P<ParseNode>>) -> P<ParseNode> {
    node.into()
}

/// The `d` fields of a node, under their TypeScript names, in declaration order.
fn fields(node: P<ParseNode>) -> Vec<(&'static str, Field<'static>)> {
    match node.d() {
        ParseNodeDetails::Module(d) => vec![(
            "statements",
            Field::Nodes(d.statements.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::Suite(d) => vec![
            (
                "statements",
                Field::Nodes(d.statements.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            (
                "typeComment",
                Field::OptToken(d.type_comment.get().map(|t| t.0)),
            ),
        ],
        ParseNodeDetails::If(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("testExpr", Field::Node(as_node(d.test_expr.get()))),
            ("ifSuite", Field::Node(as_node(d.if_suite.get()))),
            ("elseSuite", Field::OptNode(d.else_suite.get().map(as_node))),
        ],
        ParseNodeDetails::While(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("testExpr", Field::Node(as_node(d.test_expr.get()))),
            ("whileSuite", Field::Node(as_node(d.while_suite.get()))),
            ("elseSuite", Field::OptNode(d.else_suite.get().map(as_node))),
        ],
        ParseNodeDetails::For(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("isAsync", Field::OptBool(d.is_async.get())),
            ("asyncToken", Field::OptToken(d.async_token.get())),
            ("targetExpr", Field::Node(as_node(d.target_expr.get()))),
            ("iterableExpr", Field::Node(as_node(d.iterable_expr.get()))),
            ("forSuite", Field::Node(as_node(d.for_suite.get()))),
            ("elseSuite", Field::OptNode(d.else_suite.get().map(as_node))),
            (
                "typeComment",
                Field::OptToken(d.type_comment.get().map(|t| t.0)),
            ),
        ],
        ParseNodeDetails::ComprehensionFor(d) => vec![
            ("isAsync", Field::OptBool(d.is_async.get())),
            ("asyncToken", Field::OptToken(d.async_token.get())),
            ("targetExpr", Field::Node(as_node(d.target_expr.get()))),
            ("iterableExpr", Field::Node(as_node(d.iterable_expr.get()))),
        ],
        ParseNodeDetails::ComprehensionIf(d) => {
            vec![("testExpr", Field::Node(as_node(d.test_expr.get())))]
        }
        ParseNodeDetails::Try(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("trySuite", Field::Node(as_node(d.try_suite.get()))),
            (
                "exceptClauses",
                Field::Nodes(
                    d.except_clauses
                        .borrow()
                        .iter()
                        .map(|&n| as_node(n))
                        .collect(),
                ),
            ),
            ("elseSuite", Field::OptNode(d.else_suite.get().map(as_node))),
            (
                "finallySuite",
                Field::OptNode(d.finally_suite.get().map(as_node)),
            ),
        ],
        ParseNodeDetails::Except(d) => vec![
            ("typeExpr", Field::OptNode(d.type_expr.get().map(as_node))),
            ("name", Field::OptNode(d.name.get().map(as_node))),
            ("exceptSuite", Field::Node(as_node(d.except_suite.get()))),
            ("isExceptGroup", Field::Bool(d.is_except_group.get())),
            ("exceptToken", Field::Token(d.except_token.get())),
        ],
        ParseNodeDetails::Function(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            (
                "decorators",
                Field::Nodes(d.decorators.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("isAsync", Field::Bool(d.is_async.get())),
            ("name", Field::Node(as_node(d.name.get()))),
            (
                "typeParams",
                Field::OptNode(d.type_params.get().map(as_node)),
            ),
            (
                "params",
                Field::Nodes(d.params.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            (
                "returnAnnotation",
                Field::OptNode(d.return_annotation.get().map(as_node)),
            ),
            (
                "funcAnnotationComment",
                Field::OptNode(d.func_annotation_comment.get().map(as_node)),
            ),
            ("suite", Field::Node(as_node(d.suite.get()))),
        ],
        ParseNodeDetails::Parameter(d) => vec![
            ("category", Field::Int(d.category.get() as i32)),
            ("name", Field::OptNode(d.name.get().map(as_node))),
            (
                "annotation",
                Field::OptNode(d.annotation.get().map(as_node)),
            ),
            (
                "annotationComment",
                Field::OptNode(d.annotation_comment.get().map(as_node)),
            ),
            (
                "defaultValue",
                Field::OptNode(d.default_value.get().map(as_node)),
            ),
        ],
        ParseNodeDetails::Class(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            (
                "decorators",
                Field::Nodes(d.decorators.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("name", Field::Node(as_node(d.name.get()))),
            (
                "typeParams",
                Field::OptNode(d.type_params.get().map(as_node)),
            ),
            (
                "arguments",
                Field::Nodes(d.arguments.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("suite", Field::Node(as_node(d.suite.get()))),
        ],
        ParseNodeDetails::With(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("isAsync", Field::OptBool(d.is_async.get())),
            ("asyncToken", Field::OptToken(d.async_token.get())),
            (
                "withItems",
                Field::Nodes(d.with_items.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("suite", Field::Node(as_node(d.suite.get()))),
            (
                "typeComment",
                Field::OptToken(d.type_comment.get().map(|t| t.0)),
            ),
        ],
        ParseNodeDetails::WithItem(d) => vec![
            ("expr", Field::Node(as_node(d.expr.get()))),
            ("target", Field::OptNode(d.target.get().map(as_node))),
        ],
        ParseNodeDetails::Decorator(d) => vec![("expr", Field::Node(as_node(d.expr.get())))],
        ParseNodeDetails::StatementList(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            (
                "statements",
                Field::Nodes(d.statements.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
        ],
        ParseNodeDetails::Error(d) => vec![
            ("category", Field::Int(d.category.get() as i32)),
            ("child", Field::OptNode(d.child.get().map(as_node))),
            (
                "decorators",
                Field::OptNodes(
                    d.decorators
                        .as_ref()
                        .map(|v| v.iter().map(|&n| as_node(n)).collect()),
                ),
            ),
        ],
        ParseNodeDetails::UnaryOperation(d) => vec![
            ("expr", Field::Node(as_node(d.expr.get()))),
            ("operatorToken", Field::Token(d.operator_token.get())),
            ("operator", Field::Int(d.operator.get() as i32)),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::BinaryOperation(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            ("operatorToken", Field::Token(d.operator_token.get())),
            ("operator", Field::Int(d.operator.get() as i32)),
            ("rightExpr", Field::Node(as_node(d.right_expr.get()))),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::AssignmentExpression(d) => vec![
            ("name", Field::Node(as_node(d.name.get()))),
            ("walrusToken", Field::Token(d.walrus_token.get())),
            ("rightExpr", Field::Node(as_node(d.right_expr.get()))),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::Assignment(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            ("rightExpr", Field::Node(as_node(d.right_expr.get()))),
            (
                "annotationComment",
                Field::OptNode(d.annotation_comment.get().map(as_node)),
            ),
            (
                "chainedAnnotationComment",
                Field::OptNode(d.chained_annotation_comment.get().map(as_node)),
            ),
        ],
        ParseNodeDetails::TypeParameter(d) => vec![
            ("name", Field::Node(as_node(d.name.get()))),
            ("typeParamKind", Field::Int(d.type_param_kind.get() as i32)),
            ("boundExpr", Field::OptNode(d.bound_expr.get().map(as_node))),
            (
                "defaultExpr",
                Field::OptNode(d.default_expr.get().map(as_node)),
            ),
        ],
        ParseNodeDetails::TypeParameterList(d) => vec![(
            "params",
            Field::Nodes(d.params.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::TypeAlias(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("name", Field::Node(as_node(d.name.get()))),
            (
                "typeParams",
                Field::OptNode(d.type_params.get().map(as_node)),
            ),
            ("expr", Field::Node(as_node(d.expr.get()))),
        ],
        ParseNodeDetails::TypeAnnotation(d) => vec![
            ("valueExpr", Field::Node(as_node(d.value_expr.get()))),
            ("annotation", Field::Node(as_node(d.annotation.get()))),
        ],
        ParseNodeDetails::FunctionAnnotation(d) => vec![
            ("isEllipsis", Field::Bool(d.is_ellipsis.get())),
            (
                "paramAnnotations",
                Field::Nodes(
                    d.param_annotations
                        .borrow()
                        .iter()
                        .map(|&n| as_node(n))
                        .collect(),
                ),
            ),
            (
                "returnAnnotation",
                Field::Node(as_node(d.return_annotation.get())),
            ),
        ],
        ParseNodeDetails::AugmentedAssignment(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            ("operator", Field::Int(d.operator.get() as i32)),
            ("rightExpr", Field::Node(as_node(d.right_expr.get()))),
            ("destExpr", Field::Node(as_node(d.dest_expr.get()))),
        ],
        ParseNodeDetails::Await(d) => vec![
            ("expr", Field::Node(as_node(d.expr.get()))),
            ("awaitToken", Field::Token(d.await_token.get())),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::Ternary(d) => vec![
            ("ifExpr", Field::Node(as_node(d.if_expr.get()))),
            ("testExpr", Field::Node(as_node(d.test_expr.get()))),
            ("elseExpr", Field::Node(as_node(d.else_expr.get()))),
        ],
        ParseNodeDetails::Unpack(d) => vec![
            ("expr", Field::Node(as_node(d.expr.get()))),
            ("starToken", Field::Token(d.star_token.get())),
        ],
        ParseNodeDetails::Tuple(d) => vec![
            (
                "items",
                Field::Nodes(d.items.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::Call(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            (
                "args",
                Field::Nodes(d.args.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("trailingComma", Field::Bool(d.trailing_comma.get())),
        ],
        ParseNodeDetails::Comprehension(d) => vec![
            ("expr", Field::Node(as_node(d.expr.get()))),
            (
                "forIfNodes",
                Field::Nodes(
                    d.for_if_nodes
                        .borrow()
                        .iter()
                        .map(|&n| as_node(n))
                        .collect(),
                ),
            ),
            ("isGenerator", Field::Bool(d.is_generator.get())),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::Index(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            (
                "items",
                Field::Nodes(d.items.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("trailingComma", Field::Bool(d.trailing_comma.get())),
        ],
        ParseNodeDetails::Slice(d) => vec![
            (
                "startValue",
                Field::OptNode(d.start_value.get().map(as_node)),
            ),
            ("endValue", Field::OptNode(d.end_value.get().map(as_node))),
            ("stepValue", Field::OptNode(d.step_value.get().map(as_node))),
        ],
        ParseNodeDetails::Yield(d) => vec![("expr", Field::OptNode(d.expr.get().map(as_node)))],
        ParseNodeDetails::YieldFrom(d) => vec![("expr", Field::Node(as_node(d.expr.get())))],
        ParseNodeDetails::MemberAccess(d) => vec![
            ("leftExpr", Field::Node(as_node(d.left_expr.get()))),
            ("member", Field::Node(as_node(d.member.get()))),
        ],
        ParseNodeDetails::Lambda(d) => vec![
            (
                "params",
                Field::Nodes(d.params.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("expr", Field::Node(as_node(d.expr.get()))),
        ],
        ParseNodeDetails::Name(d) => vec![
            ("token", Field::Token(d.token.get().0)),
            ("value", Field::Str(d.value())),
        ],
        ParseNodeDetails::Constant(d) => vec![("constType", Field::Int(d.const_type.get() as i32))],
        ParseNodeDetails::Ellipsis(_) => vec![],
        ParseNodeDetails::Number(d) => vec![
            ("value", Field::Number(&d.value)),
            ("isInteger", Field::Bool(d.is_integer.get())),
            ("isImaginary", Field::Bool(d.is_imaginary.get())),
        ],
        ParseNodeDetails::String(d) => vec![
            ("token", Field::Token(d.token.get().0)),
            ("value", Field::JsStr(&d.value)),
        ],
        ParseNodeDetails::FormatString(d) => vec![
            ("token", Field::Token(d.token.get().0)),
            (
                "middleTokens",
                Field::Tokens(d.middle_tokens.borrow().iter().map(|t| t.0).collect()),
            ),
            (
                "fieldExprs",
                Field::Nodes(d.field_exprs.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            (
                "formatExprs",
                Field::Nodes(
                    d.format_exprs
                        .borrow()
                        .iter()
                        .map(|&n| as_node(n))
                        .collect(),
                ),
            ),
            ("value", Field::JsStr(&d.value)),
        ],
        ParseNodeDetails::StringList(d) => vec![
            (
                "strings",
                Field::Nodes(d.strings.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("hasParens", Field::Bool(d.has_parens.get())),
        ],
        ParseNodeDetails::Dictionary(d) => vec![
            (
                "items",
                Field::Nodes(d.items.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            (
                "trailingCommaToken",
                Field::OptToken(d.trailing_comma_token.get()),
            ),
        ],
        ParseNodeDetails::DictionaryKeyEntry(d) => vec![
            ("keyExpr", Field::Node(as_node(d.key_expr.get()))),
            ("valueExpr", Field::Node(as_node(d.value_expr.get()))),
        ],
        ParseNodeDetails::DictionaryExpandEntry(d) => {
            vec![("expr", Field::Node(as_node(d.expr.get())))]
        }
        ParseNodeDetails::Set(d) => vec![(
            "items",
            Field::Nodes(d.items.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::List(d) => vec![(
            "items",
            Field::Nodes(d.items.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::Argument(d) => vec![
            ("argCategory", Field::Int(d.arg_category.get() as i32)),
            ("name", Field::OptNode(d.name.get().map(as_node))),
            ("valueExpr", Field::Node(as_node(d.value_expr.get()))),
        ],
        ParseNodeDetails::Del(d) => vec![(
            "targets",
            Field::Nodes(d.targets.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::Pass(_) => vec![],
        ParseNodeDetails::Import(d) => vec![
            (
                "list",
                Field::Nodes(d.list.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("isLazy", Field::OptBool(d.is_lazy.get())),
            (
                "lazyToken",
                Field::OptToken(d.lazy_token.get().map(|t| t.0)),
            ),
        ],
        ParseNodeDetails::ModuleName(d) => vec![
            ("leadingDots", Field::Int(d.leading_dots.get())),
            (
                "nameParts",
                Field::Nodes(d.name_parts.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("hasTrailingDot", Field::OptBool(d.has_trailing_dot.get())),
        ],
        ParseNodeDetails::ImportAs(d) => vec![
            ("module", Field::Node(as_node(d.module.get()))),
            ("alias", Field::OptNode(d.alias.get().map(as_node))),
        ],
        ParseNodeDetails::ImportFrom(d) => vec![
            ("module", Field::Node(as_node(d.module.get()))),
            (
                "imports",
                Field::Nodes(d.imports.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("isWildcardImport", Field::Bool(d.is_wildcard_import.get())),
            ("usesParens", Field::Bool(d.uses_parens.get())),
            ("wildcardToken", Field::OptToken(d.wildcard_token.get())),
            ("missingImport", Field::OptBool(d.missing_import.get())),
            ("isLazy", Field::OptBool(d.is_lazy.get())),
            (
                "lazyToken",
                Field::OptToken(d.lazy_token.get().map(|t| t.0)),
            ),
        ],
        ParseNodeDetails::ImportFromAs(d) => vec![
            ("name", Field::Node(as_node(d.name.get()))),
            ("alias", Field::OptNode(d.alias.get().map(as_node))),
        ],
        ParseNodeDetails::Global(d) => vec![(
            "targets",
            Field::Nodes(d.targets.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::Nonlocal(d) => vec![(
            "targets",
            Field::Nodes(d.targets.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::Assert(d) => vec![
            ("testExpr", Field::Node(as_node(d.test_expr.get()))),
            (
                "exceptionExpr",
                Field::OptNode(d.exception_expr.get().map(as_node)),
            ),
        ],
        ParseNodeDetails::Break(_) => vec![],
        ParseNodeDetails::Continue(_) => vec![],
        ParseNodeDetails::Return(d) => vec![("expr", Field::OptNode(d.expr.get().map(as_node)))],
        ParseNodeDetails::Raise(d) => vec![
            ("expr", Field::OptNode(d.expr.get().map(as_node))),
            ("fromExpr", Field::OptNode(d.from_expr.get().map(as_node))),
        ],
        ParseNodeDetails::Match(d) => vec![
            ("firstToken", Field::Token(d.first_token.get())),
            ("expr", Field::Node(as_node(d.expr.get()))),
            (
                "cases",
                Field::Nodes(d.cases.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
        ],
        ParseNodeDetails::Case(d) => vec![
            ("pattern", Field::Node(as_node(d.pattern.get()))),
            ("isIrrefutable", Field::Bool(d.is_irrefutable.get())),
            ("guardExpr", Field::OptNode(d.guard_expr.get().map(as_node))),
            ("suite", Field::Node(as_node(d.suite.get()))),
        ],
        ParseNodeDetails::PatternSequence(d) => vec![
            (
                "entries",
                Field::Nodes(d.entries.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("starEntryIndex", Field::OptInt(d.star_entry_index.get())),
        ],
        ParseNodeDetails::PatternAs(d) => vec![
            (
                "orPatterns",
                Field::Nodes(d.or_patterns.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
            ("target", Field::OptNode(d.target.get().map(as_node))),
        ],
        ParseNodeDetails::PatternLiteral(d) => vec![("expr", Field::Node(as_node(d.expr.get())))],
        ParseNodeDetails::PatternClass(d) => vec![
            ("className", Field::Node(as_node(d.class_name.get()))),
            (
                "args",
                Field::Nodes(d.args.borrow().iter().map(|&n| as_node(n)).collect()),
            ),
        ],
        ParseNodeDetails::PatternClassArgument(d) => vec![
            ("name", Field::OptNode(d.name.get().map(as_node))),
            ("pattern", Field::Node(as_node(d.pattern.get()))),
        ],
        ParseNodeDetails::PatternCapture(d) => vec![
            ("target", Field::Node(as_node(d.target.get()))),
            ("isStar", Field::Bool(d.is_star.get())),
            ("isWildcard", Field::Bool(d.is_wildcard.get())),
        ],
        ParseNodeDetails::PatternMapping(d) => vec![(
            "entries",
            Field::Nodes(d.entries.borrow().iter().map(|&n| as_node(n)).collect()),
        )],
        ParseNodeDetails::PatternMappingKeyEntry(d) => vec![
            ("keyPattern", Field::Node(as_node(d.key_pattern.get()))),
            ("valuePattern", Field::Node(as_node(d.value_pattern.get()))),
        ],
        ParseNodeDetails::PatternMappingExpandEntry(d) => {
            vec![("target", Field::Node(as_node(d.target.get())))]
        }
        ParseNodeDetails::PatternValue(d) => vec![("expr", Field::Node(as_node(d.expr.get())))],
    }
}

/// `<NodeTypeName>@<start>+<length>`: a parse node printed as a reference.
pub fn node_ref(node: &ParseNode) -> String {
    format!(
        "{}@{}",
        parse_node_type_name_map(node.node_type),
        format::text_range(node)
    )
}

fn write_node(
    w: &mut DumpWriter,
    depth: usize,
    key: &str,
    node: P<ParseNode>,
    holder: Option<P<ParseNode>>,
) {
    let mut header = format!(
        "{} {}",
        parse_node_type_name_map(node.node_type),
        format::text_range(&*node)
    );
    if let Some(holder) = holder
        && node.parent.get() != Some(holder)
    {
        header.push_str(" !parent=");
        match node.parent.get() {
            Some(parent) => header.push_str(&node_ref(&parent)),
            None => header.push_str("undefined"),
        }
    }
    w.field(depth, key, &header);

    let children = get_child_nodes(node, None);
    let mut node_fields = fields(node);
    node_fields.sort_by(|a, b| a.0.cmp(b.0));
    for (field_key, field) in &node_fields {
        write_field(w, depth + 1, field_key, field, node, &children);
    }

    if node.node_type == ParseNodeType::StringList
        && let Some(annotation) = get_parser_string_annotation(StringListNode(node))
    {
        write_node(w, depth + 1, "(annotation)", annotation, Some(node));
    }
}

/// A parse node in a `d` field: nested when it is one of the holder's children, else a reference.
fn write_detail_node(
    w: &mut DumpWriter,
    depth: usize,
    key: &str,
    value: P<ParseNode>,
    holder: P<ParseNode>,
    children: &[Option<P<ParseNode>>],
) {
    if children.contains(&Some(value)) {
        write_node(w, depth, key, value, Some(holder));
    } else {
        w.field(depth, key, &node_ref(&value));
    }
}

fn write_detail_nodes(
    w: &mut DumpWriter,
    depth: usize,
    key: &str,
    values: &[P<ParseNode>],
    holder: P<ParseNode>,
    children: &[Option<P<ParseNode>>],
) {
    w.field(depth, key, &format::array_header(values.len()));
    for (i, &value) in values.iter().enumerate() {
        write_detail_node(w, depth + 1, &format::index_key(i), value, holder, children);
    }
}

fn write_field(
    w: &mut DumpWriter,
    depth: usize,
    key: &str,
    field: &Field<'_>,
    holder: P<ParseNode>,
    children: &[Option<P<ParseNode>>],
) {
    match field {
        Field::Node(value) | Field::OptNode(Some(value)) => {
            write_detail_node(w, depth, key, *value, holder, children)
        }
        Field::Nodes(values) | Field::OptNodes(Some(values)) => {
            write_detail_nodes(w, depth, key, values, holder, children)
        }
        Field::Token(token) | Field::OptToken(Some(token)) => write_token(w, depth, key, *token),
        Field::Tokens(tokens) => {
            w.field(depth, key, &format::array_header(tokens.len()));
            for (i, token) in tokens.iter().enumerate() {
                write_token(w, depth + 1, &format::index_key(i), *token);
            }
        }
        Field::Bool(b) | Field::OptBool(Some(b)) => w.field(depth, key, format::boolean(*b)),
        Field::Int(n) | Field::OptInt(Some(n)) => w.field(depth, key, &format::number(*n as f64)),
        Field::Str(s) => w.field(depth, key, &format::str(s)),
        Field::JsStr(s) => w.field(depth, key, &format::string(s)),
        Field::Number(n) => w.field(depth, key, &format::number_value(n)),
        Field::OptNode(None)
        | Field::OptNodes(None)
        | Field::OptToken(None)
        | Field::OptBool(None)
        | Field::OptInt(None) => {}
    }
}

/// The tree-stage file for one source file: the `ParserOutput` the parser returned.
pub fn dump_parser_output(output: &ParserOutput) -> String {
    let mut w = DumpWriter::new();
    w.field(
        0,
        "containsWildcardImport",
        format::boolean(output.contains_wildcard_import),
    );
    w.field(
        0,
        "futureImports",
        &format!("Set({})", output.future_imports.len()),
    );
    for (i, name) in output.future_imports.iter().enumerate() {
        w.field(1, &format::index_key(i), &format::str(name));
    }
    w.field(
        0,
        "hasTypeAnnotations",
        format::boolean(output.has_type_annotations),
    );
    w.field(
        0,
        "importedModules",
        &format::array_header(output.imported_modules.len()),
    );
    for (i, module) in output.imported_modules.iter().enumerate() {
        w.open(1, &format::index_key(i));
        if let Some(imported_symbols) = &module.imported_symbols {
            w.field(
                2,
                "importedSymbols",
                &format!("Set({})", imported_symbols.len()),
            );
            for (j, symbol) in imported_symbols.iter().enumerate() {
                w.field(3, &format::index_key(j), &format::str(symbol));
            }
        }
        w.field(
            2,
            "leadingDots",
            &format::number(module.leading_dots as f64),
        );
        w.field(2, "nameNode", &node_ref(&module.name_node));
        w.field(
            2,
            "nameParts",
            &format::array_header(module.name_parts.len()),
        );
        for (j, part) in module.name_parts.iter().enumerate() {
            w.field(3, &format::index_key(j), &format::str(part));
        }
    }
    write_node(&mut w, 0, "parseTree", output.parse_tree.0, None);
    w.field(
        0,
        "typingSymbolAliases",
        &format::map_header(output.typing_symbol_aliases.len()),
    );
    for (alias, name) in &output.typing_symbol_aliases {
        w.field(1, &format::str(alias), &format::str(name));
    }
    w.finish()
}
