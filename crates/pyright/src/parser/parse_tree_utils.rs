//! Port of `packages/pyright-internal/src/parser/parseTreeUtils.ts`.

use jsrt::P;

use super::parse_nodes::{
    ExpressionNode, ParseNode, ParseNodeArray, ParseNodeDetails, StringListNode,
    get_parser_string_annotation,
};

// parser/parseTreeUtils.ts:4
pub type StringAnnotationProvider<'a> = &'a dyn Fn(StringListNode) -> Option<ExpressionNode>;

fn some<T: Into<P<ParseNode>>>(node: T) -> Option<P<ParseNode>> {
    Some(node.into())
}

fn opt<T: Into<P<ParseNode>>>(node: Option<T>) -> Option<P<ParseNode>> {
    node.map(Into::into)
}

fn all<T: Into<P<ParseNode>> + Copy>(
    nodes: &[T],
) -> impl Iterator<Item = Option<P<ParseNode>>> + '_ {
    nodes.iter().map(|&n| Some(n.into()))
}

/// A list `get_child_nodes` can return: a `ParseNodeArray`, or the `ChildNodes`
/// that `ParseTreeWalker::walk` keeps on the stack.
pub trait ChildNodeList: Default + Extend<Option<P<ParseNode>>> {
    fn push(&mut self, node: Option<P<ParseNode>>);
}

impl ChildNodeList for ParseNodeArray {
    fn push(&mut self, node: Option<P<ParseNode>>) {
        Vec::push(self, node);
    }
}

/// Child nodes, the first eight inline: most nodes have no more, so a walk
/// needs no heap array per node.
#[derive(Default)]
pub struct ChildNodes {
    len: usize,
    inline: [Option<P<ParseNode>>; 8],
    /// Every child, once there are more than eight.
    spilled: ParseNodeArray,
}

impl ChildNodeList for ChildNodes {
    fn push(&mut self, node: Option<P<ParseNode>>) {
        if self.spilled.is_empty() {
            if let Some(slot) = self.inline.get_mut(self.len) {
                *slot = node;
                self.len += 1;
                return;
            }
            self.spilled.reserve(2 * self.inline.len());
            self.spilled.extend_from_slice(&self.inline);
        }
        self.spilled.push(node);
    }
}

impl Extend<Option<P<ParseNode>>> for ChildNodes {
    fn extend<I: IntoIterator<Item = Option<P<ParseNode>>>>(&mut self, nodes: I) {
        for node in nodes {
            self.push(node);
        }
    }
}

impl std::ops::Deref for ChildNodes {
    type Target = [Option<P<ParseNode>>];

    fn deref(&self) -> &Self::Target {
        if self.spilled.is_empty() { &self.inline[..self.len] } else { &self.spilled }
    }
}

/// `None` for `get_string_annotation` is the default, `getParserStringAnnotation`.
// parser/parseTreeUtils.ts:6
pub fn get_child_nodes<L: ChildNodeList>(
    node: P<ParseNode>,
    get_string_annotation: Option<StringAnnotationProvider<'_>>,
) -> L {
    let mut v = L::default();
    match node.d() {
        ParseNodeDetails::Error(d) => {
            if let Some(decorators) = &d.decorators {
                v.extend(all(decorators));
            }
            v.push(d.child.get());
        }

        ParseNodeDetails::Argument(d) => v.extend([opt(d.name.get()), some(d.value_expr.get())]),

        ParseNodeDetails::Assert(d) => v.extend([some(d.test_expr.get()), d.exception_expr.get()]),

        ParseNodeDetails::AssignmentExpression(d) => {
            v.extend([some(d.name.get()), some(d.right_expr.get())])
        }

        ParseNodeDetails::Assignment(d) => v.extend([
            some(d.left_expr.get()),
            some(d.right_expr.get()),
            d.annotation_comment.get(),
        ]),

        ParseNodeDetails::AugmentedAssignment(d) => {
            v.extend([some(d.left_expr.get()), some(d.right_expr.get())])
        }

        ParseNodeDetails::Await(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::BinaryOperation(d) => {
            v.extend([some(d.left_expr.get()), some(d.right_expr.get())])
        }

        ParseNodeDetails::Break(_) => {}

        ParseNodeDetails::Call(d) => {
            v.push(some(d.left_expr.get()));
            v.extend(all(&d.args.borrow()));
        }

        ParseNodeDetails::Case(d) => v.extend([
            some(d.pattern.get()),
            d.guard_expr.get(),
            some(d.suite.get()),
        ]),

        ParseNodeDetails::Class(d) => {
            v.extend(all(&d.decorators.borrow()));
            v.push(some(d.name.get()));
            v.push(opt(d.type_params.get()));
            v.extend(all(&d.arguments.borrow()));
            v.push(some(d.suite.get()));
        }

        ParseNodeDetails::Comprehension(d) => {
            v.push(some(d.expr.get()));
            v.extend(all(&d.for_if_nodes.borrow()));
        }

        ParseNodeDetails::ComprehensionFor(d) => {
            v.extend([some(d.target_expr.get()), some(d.iterable_expr.get())])
        }

        ParseNodeDetails::ComprehensionIf(d) => v.push(some(d.test_expr.get())),

        ParseNodeDetails::Constant(_) => {}

        ParseNodeDetails::Continue(_) => {}

        ParseNodeDetails::Decorator(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::Del(d) => v.extend(all(&d.targets.borrow())),

        ParseNodeDetails::Dictionary(d) => v.extend(all(&d.items.borrow())),

        ParseNodeDetails::DictionaryExpandEntry(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::DictionaryKeyEntry(d) => {
            v.extend([some(d.key_expr.get()), some(d.value_expr.get())])
        }

        ParseNodeDetails::Ellipsis(_) => {}

        ParseNodeDetails::If(d) => v.extend([
            some(d.test_expr.get()),
            some(d.if_suite.get()),
            d.else_suite.get(),
        ]),

        ParseNodeDetails::Import(d) => v.extend(all(&d.list.borrow())),

        ParseNodeDetails::ImportAs(d) => v.extend([some(d.module.get()), opt(d.alias.get())]),

        ParseNodeDetails::ImportFrom(d) => {
            v.push(some(d.module.get()));
            v.extend(all(&d.imports.borrow()));
        }

        ParseNodeDetails::ImportFromAs(d) => v.extend([some(d.name.get()), opt(d.alias.get())]),

        ParseNodeDetails::Index(d) => {
            v.push(some(d.left_expr.get()));
            v.extend(all(&d.items.borrow()));
        }

        ParseNodeDetails::Except(d) => v.extend([
            d.type_expr.get(),
            opt(d.name.get()),
            some(d.except_suite.get()),
        ]),

        ParseNodeDetails::For(d) => v.extend([
            some(d.target_expr.get()),
            some(d.iterable_expr.get()),
            some(d.for_suite.get()),
            opt(d.else_suite.get()),
        ]),

        ParseNodeDetails::FormatString(d) => {
            v.extend(all(&d.field_exprs.borrow()));
            v.extend(all(&d.format_exprs.borrow()));
        }

        ParseNodeDetails::Function(d) => {
            v.extend(all(&d.decorators.borrow()));
            v.push(some(d.name.get()));
            v.push(opt(d.type_params.get()));
            v.extend(all(&d.params.borrow()));
            v.push(d.return_annotation.get());
            v.push(opt(d.func_annotation_comment.get()));
            v.push(some(d.suite.get()));
        }

        ParseNodeDetails::FunctionAnnotation(d) => {
            v.extend(all(&d.param_annotations.borrow()));
            v.push(some(d.return_annotation.get()));
        }

        ParseNodeDetails::Global(d) => v.extend(all(&d.targets.borrow())),

        ParseNodeDetails::Lambda(d) => {
            v.extend(all(&d.params.borrow()));
            v.push(some(d.expr.get()));
        }

        ParseNodeDetails::List(d) => v.extend(all(&d.items.borrow())),

        ParseNodeDetails::Match(d) => {
            v.push(some(d.expr.get()));
            v.extend(all(&d.cases.borrow()));
        }

        ParseNodeDetails::MemberAccess(d) => {
            v.extend([some(d.left_expr.get()), some(d.member.get())])
        }

        ParseNodeDetails::ModuleName(d) => v.extend(all(&d.name_parts.borrow())),

        ParseNodeDetails::Module(d) => v.extend(all(&d.statements.borrow())),

        ParseNodeDetails::Name(_) => {}

        ParseNodeDetails::Nonlocal(d) => v.extend(all(&d.targets.borrow())),

        ParseNodeDetails::Number(_) => {}

        ParseNodeDetails::Parameter(d) => v.extend([
            opt(d.name.get()),
            d.annotation.get(),
            d.annotation_comment.get(),
            d.default_value.get(),
        ]),

        ParseNodeDetails::Pass(_) => {}

        ParseNodeDetails::PatternAs(d) => {
            v.extend(all(&d.or_patterns.borrow()));
            v.push(opt(d.target.get()));
        }

        ParseNodeDetails::PatternClass(d) => {
            v.push(some(d.class_name.get()));
            v.extend(all(&d.args.borrow()));
        }

        ParseNodeDetails::PatternClassArgument(d) => {
            v.extend([opt(d.name.get()), some(d.pattern.get())])
        }

        ParseNodeDetails::PatternCapture(d) => v.push(some(d.target.get())),

        ParseNodeDetails::PatternLiteral(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::PatternMappingExpandEntry(d) => v.push(some(d.target.get())),

        ParseNodeDetails::PatternMappingKeyEntry(d) => {
            v.extend([some(d.key_pattern.get()), some(d.value_pattern.get())])
        }

        ParseNodeDetails::PatternMapping(d) => v.extend(all(&d.entries.borrow())),

        ParseNodeDetails::PatternSequence(d) => v.extend(all(&d.entries.borrow())),

        ParseNodeDetails::PatternValue(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::Raise(d) => v.extend([d.expr.get(), d.from_expr.get()]),

        ParseNodeDetails::Return(d) => v.push(d.expr.get()),

        ParseNodeDetails::Set(d) => v.extend(all(&d.items.borrow())),

        ParseNodeDetails::Slice(d) => {
            v.extend([d.start_value.get(), d.end_value.get(), d.step_value.get()])
        }

        ParseNodeDetails::StatementList(d) => v.extend(all(&d.statements.borrow())),

        ParseNodeDetails::StringList(d) => {
            let string_list = StringListNode(node);
            v.push(match get_string_annotation {
                Some(provider) => provider(string_list),
                None => get_parser_string_annotation(string_list),
            });
            v.extend(all(&d.strings.borrow()));
        }

        ParseNodeDetails::String(_) => {}

        ParseNodeDetails::Suite(d) => v.extend(all(&d.statements.borrow())),

        ParseNodeDetails::Ternary(d) => v.extend([
            some(d.if_expr.get()),
            some(d.test_expr.get()),
            some(d.else_expr.get()),
        ]),

        ParseNodeDetails::Tuple(d) => v.extend(all(&d.items.borrow())),

        ParseNodeDetails::Try(d) => {
            v.push(some(d.try_suite.get()));
            v.extend(all(&d.except_clauses.borrow()));
            v.push(opt(d.else_suite.get()));
            v.push(opt(d.finally_suite.get()));
        }

        ParseNodeDetails::TypeAlias(d) => v.extend([
            some(d.name.get()),
            opt(d.type_params.get()),
            some(d.expr.get()),
        ]),

        ParseNodeDetails::TypeAnnotation(d) => {
            v.extend([some(d.value_expr.get()), some(d.annotation.get())])
        }

        ParseNodeDetails::TypeParameter(d) => {
            v.extend([some(d.name.get()), d.bound_expr.get(), d.default_expr.get()])
        }

        ParseNodeDetails::TypeParameterList(d) => v.extend(all(&d.params.borrow())),

        ParseNodeDetails::UnaryOperation(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::Unpack(d) => v.push(some(d.expr.get())),

        ParseNodeDetails::While(d) => v.extend([
            some(d.test_expr.get()),
            some(d.while_suite.get()),
            opt(d.else_suite.get()),
        ]),

        ParseNodeDetails::With(d) => {
            v.extend(all(&d.with_items.borrow()));
            v.push(some(d.suite.get()));
        }

        ParseNodeDetails::WithItem(d) => v.extend([some(d.expr.get()), d.target.get()]),

        ParseNodeDetails::Yield(d) => v.push(d.expr.get()),

        ParseNodeDetails::YieldFrom(d) => v.push(some(d.expr.get())),
    }
    v
}
