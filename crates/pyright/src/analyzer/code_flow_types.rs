//! Port of `packages/pyright-internal/src/analyzer/codeFlowTypes.ts`.
//!
//! A flow node is one `FlowNode` struct: `flags`, `id` and `d`, an enum with
//! one variant per TypeScript interface that extends `FlowNode`. The variant
//! records which object literal the binder built, so the oracle prints the
//! same fields. Each interface also gets a typed handle (`FlowLabel`,
//! `FlowAssignment`, ...) with `cast` and `d()`, as parse nodes do.

use jsrt::flag_ops;
use jsrt::cell::{Cell, RefCell};
use std::ops::Deref;

use jsrt::{JsSet, P};

use crate::common::debug::{assert, fail};
use crate::parser::parse_nodes::{
    ArgCategory, CallNode, ExpressionNode, ImportFromNode, MatchNode, NameNode, ParseNode,
    ParseNodeDetails as D, ParseNodeType, SuiteNode,
};
use crate::parser::tokenizer_types::OperatorType;

// analyzer/codeFlowTypes.ts:35
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[derive(jsrt::Trace)]
pub struct FlowFlags(pub u32);

impl FlowFlags {
    pub const UnreachableStructural: FlowFlags = FlowFlags(1 << 0); // Code that is structurally unreachable (e.g. following a return statement)
    pub const UnreachableStaticCondition: FlowFlags = FlowFlags(1 << 1); // code that is unreachable due to a condition that the binder evaluates to False
    pub const Start: FlowFlags = FlowFlags(1 << 2); // Entry point
    pub const BranchLabel: FlowFlags = FlowFlags(1 << 3); // Junction for forward control flow
    pub const LoopLabel: FlowFlags = FlowFlags(1 << 4); // Junction for backward control flow
    pub const Assignment: FlowFlags = FlowFlags(1 << 5); // Assignment statement
    pub const Unbind: FlowFlags = FlowFlags(1 << 6); // Used with assignment to indicate target should be unbound
    pub const WildcardImport: FlowFlags = FlowFlags(1 << 7); // For "from X import *" statements
    pub const TrueCondition: FlowFlags = FlowFlags(1 << 8); // Condition known to be true
    pub const FalseCondition: FlowFlags = FlowFlags(1 << 9); // Condition known to be false
    pub const Call: FlowFlags = FlowFlags(1 << 10); // Call node
    pub const PreFinallyGate: FlowFlags = FlowFlags(1 << 11); // Injected edge that links pre-finally label and pre-try flow
    pub const PostFinally: FlowFlags = FlowFlags(1 << 12); // Injected edge that links post-finally flow with the rest of the graph
    pub const VariableAnnotation: FlowFlags = FlowFlags(1 << 14); // Separates a variable annotation from its name node
    pub const PostContextManager: FlowFlags = FlowFlags(1 << 15); // Label that's used for context managers that suppress exceptions
    pub const TrueNeverCondition: FlowFlags = FlowFlags(1 << 16); // Condition whose type evaluates to never when narrowed in positive test
    pub const FalseNeverCondition: FlowFlags = FlowFlags(1 << 17); // Condition whose type evaluates to never when narrowed in negative test
    pub const NarrowForPattern: FlowFlags = FlowFlags(1 << 18); // Narrow the type of the subject expression within a case statement
    pub const ExhaustedMatch: FlowFlags = FlowFlags(1 << 19); // Control flow gate that is closed when match is provably exhaustive
}


flag_ops!(FlowFlags);

thread_local! {
    // analyzer/codeFlowTypes.ts:57
    static _NEXT_FLOW_NODE_ID: Cell<u32> = const { Cell::new(1) };
}

// analyzer/codeFlowTypes.ts:59
/// `NameNode | MemberAccessNode | IndexNode | AssignmentExpressionNode`.
pub type CodeFlowReferenceExpressionNode = P<ParseNode>;

// analyzer/codeFlowTypes.ts:61
pub fn get_unique_flow_node_id() -> u32 {
    _NEXT_FLOW_NODE_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

// analyzer/codeFlowTypes.ts:65
#[derive(jsrt::Trace)]
pub struct FlowNode {
    pub flags: Cell<FlowFlags>,
    pub id: u32,
    pub d: FlowNodeDetails,
}

/// The interface the binder built the node as. `Plain` is a bare `FlowNode`
/// (start and unreachable nodes).
#[derive(jsrt::Trace)]
pub enum FlowNodeDetails {
    Plain,
    Label(FlowLabelDetails),
    BranchLabel(FlowBranchLabelDetails),
    Assignment(FlowAssignmentDetails),
    VariableAnnotation(FlowVariableAnnotationDetails),
    WildcardImport(FlowWildcardImportDetails),
    Condition(FlowConditionDetails),
    NarrowForPattern(FlowNarrowForPatternDetails),
    ExhaustedMatch(FlowExhaustedMatchDetails),
    Call(FlowCallDetails),
    PreFinallyGate(FlowPreFinallyGateDetails),
    PostFinally(FlowPostFinallyDetails),
    PostContextManagerLabel(FlowPostContextManagerLabelDetails),
}

impl FlowNode {
    pub fn create(flags: FlowFlags, id: u32, d: FlowNodeDetails) -> P<FlowNode> {
        P::new(FlowNode {
            flags: Cell::new(flags),
            id,
            d,
        })
    }

    /// The `antecedent` field of the interfaces that have one.
    pub fn antecedent(&self) -> Option<P<FlowNode>> {
        match &self.d {
            FlowNodeDetails::Assignment(d) => Some(d.antecedent),
            FlowNodeDetails::VariableAnnotation(d) => Some(d.antecedent),
            FlowNodeDetails::WildcardImport(d) => Some(d.antecedent),
            FlowNodeDetails::Condition(d) => Some(d.antecedent),
            FlowNodeDetails::NarrowForPattern(d) => Some(d.antecedent),
            FlowNodeDetails::ExhaustedMatch(d) => Some(d.antecedent),
            FlowNodeDetails::Call(d) => Some(d.antecedent),
            FlowNodeDetails::PreFinallyGate(d) => Some(d.antecedent),
            FlowNodeDetails::PostFinally(d) => Some(d.antecedent),
            FlowNodeDetails::Plain
            | FlowNodeDetails::Label(_)
            | FlowNodeDetails::BranchLabel(_)
            | FlowNodeDetails::PostContextManagerLabel(_) => None,
        }
    }
}

macro_rules! flow_handle {
    ($handle:ident, $details:ident, $($variant:ident),+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, jsrt::Trace)]
        pub struct $handle(pub P<FlowNode>);

        impl $handle {
            pub fn cast(node: P<FlowNode>) -> Option<$handle> {
                match &node.get().d {
                    $(FlowNodeDetails::$variant(_))|+ => Some($handle(node)),
                    _ => None,
                }
            }

            pub fn node(self) -> P<FlowNode> {
                self.0
            }
        }

        impl Deref for $handle {
            type Target = FlowNode;
            fn deref(&self) -> &FlowNode {
                self.0.get()
            }
        }

        impl From<$handle> for P<FlowNode> {
            fn from(handle: $handle) -> P<FlowNode> {
                handle.0
            }
        }
    };
}

// analyzer/codeFlowTypes.ts:70
#[derive(jsrt::Trace)]
pub struct FlowLabelDetails {
    pub antecedents: RefCell<Vec<P<FlowNode>>>,

    // Set of all expressions that require code flow analysis
    // through the loop or in branch paths to determine their types.
    // If an expression is not within this map, branch or loop analysis
    // can be skipped and determined from the first antecedent only.
    pub affected_expressions: Cell<Option<P<RefCell<JsSet<String>>>>>,
}

// FlowLabel represents a junction with multiple possible
// preceding control flows.
flow_handle!(
    FlowLabel,
    FlowLabelDetails,
    Label,
    BranchLabel,
    PostContextManagerLabel
);

impl FlowLabel {
    pub fn d(&self) -> &'static FlowLabelDetails {
        match &self.0.get().d {
            FlowNodeDetails::Label(d) => d,
            FlowNodeDetails::BranchLabel(d) => &d.label,
            FlowNodeDetails::PostContextManagerLabel(d) => &d.label,
            _ => unreachable!(),
        }
    }
}

impl FlowLabelDetails {
    pub fn new() -> FlowLabelDetails {
        FlowLabelDetails {
            antecedents: RefCell::new(Vec::new()),
            affected_expressions: Cell::new(None),
        }
    }
}

// analyzer/codeFlowTypes.ts:82
#[derive(jsrt::Trace)]
pub struct FlowBranchLabelDetails {
    pub label: FlowLabelDetails,

    // If specified, this label represents a flow node that precedes
    // (i.e. is higher up in the control flow graph) than all of
    // the antecedents of this branch label. If an expression is
    // not affected by the branch label, the entire flow node can be
    // skipped, and processing can proceed at this label.
    pub pre_branch_antecedent: Option<P<FlowNode>>,
}
flow_handle!(FlowBranchLabel, FlowBranchLabelDetails, BranchLabel);

impl FlowBranchLabel {
    pub fn d(&self) -> &'static FlowBranchLabelDetails {
        match &self.0.get().d {
            FlowNodeDetails::BranchLabel(d) => d,
            _ => unreachable!(),
        }
    }

    pub fn label(self) -> FlowLabel {
        FlowLabel(self.0)
    }
}

macro_rules! simple_flow_handle {
    ($handle:ident, $details:ident, $variant:ident) => {
        flow_handle!($handle, $details, $variant);

        impl $handle {
            pub fn d(&self) -> &'static $details {
                match &self.0.get().d {
                    FlowNodeDetails::$variant(d) => d,
                    _ => unreachable!(),
                }
            }
        }
    };
}

// analyzer/codeFlowTypes.ts:91
// FlowAssignment represents a node that assigns a value.
#[derive(jsrt::Trace)]
pub struct FlowAssignmentDetails {
    pub node: CodeFlowReferenceExpressionNode,
    pub antecedent: P<FlowNode>,
    pub target_symbol_id: u32,
}
simple_flow_handle!(FlowAssignment, FlowAssignmentDetails, Assignment);

// FlowVariableAnnotation separates a variable annotation
// node from its type annotation. For example, the declaration
// "foo: bar", the "bar" needs to be associated with a flow
// node that precedes the "foo". This is important if the
// same name is used for both (e.g. "foo: foo") and we need
// to determine that the annotation refers to a symbol within
// an outer scope.
// analyzer/codeFlowTypes.ts:105
#[derive(jsrt::Trace)]
pub struct FlowVariableAnnotationDetails {
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(
    FlowVariableAnnotation,
    FlowVariableAnnotationDetails,
    VariableAnnotation
);

// analyzer/codeFlowTypes.ts:111
// Similar to FlowAssignment but used specifically for
// wildcard "from X import *" statements.
#[derive(jsrt::Trace)]
pub struct FlowWildcardImportDetails {
    pub node: ImportFromNode,
    pub names: Vec<String>,
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(
    FlowWildcardImport,
    FlowWildcardImportDetails,
    WildcardImport
);

// analyzer/codeFlowTypes.ts:117
// FlowCondition represents a condition that is known to
// be true or false at the node's location in the control flow.
#[derive(jsrt::Trace)]
pub struct FlowConditionDetails {
    pub expression: ExpressionNode,
    pub reference: Option<NameNode>,
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(FlowCondition, FlowConditionDetails, Condition);

// analyzer/codeFlowTypes.ts:125
/// `statement` is a `CaseNode | MatchNode`.
#[derive(jsrt::Trace)]
pub struct FlowNarrowForPatternDetails {
    pub subject_expression: ExpressionNode,
    pub statement: P<ParseNode>,
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(
    FlowNarrowForPattern,
    FlowNarrowForPatternDetails,
    NarrowForPattern
);

// analyzer/codeFlowTypes.ts:134
// FlowExhaustedMatch represents a control flow gate that is "closed"
// if a match statement can be statically proven to exhaust all cases
// (i.e. the narrowed type of the subject expression is Never at the bottom).
#[derive(jsrt::Trace)]
pub struct FlowExhaustedMatchDetails {
    pub node: MatchNode,
    pub subject_expression: ExpressionNode,
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(
    FlowExhaustedMatch,
    FlowExhaustedMatchDetails,
    ExhaustedMatch
);

// analyzer/codeFlowTypes.ts:142
// Records a call, which may raise exceptions, thus affecting
// the code flow and making subsequent code unreachable.
#[derive(jsrt::Trace)]
pub struct FlowCallDetails {
    pub node: CallNode,
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(FlowCall, FlowCallDetails, Call);

// analyzer/codeFlowTypes.ts:149
// See comment in the visitTry method in binder.ts for a full
// explanation of the FlowPreFinally and FlowPostFinally nodes.
#[derive(jsrt::Trace)]
pub struct FlowPreFinallyGateDetails {
    pub antecedent: P<FlowNode>,
}
simple_flow_handle!(
    FlowPreFinallyGate,
    FlowPreFinallyGateDetails,
    PreFinallyGate
);

// analyzer/codeFlowTypes.ts:153
#[derive(jsrt::Trace)]
pub struct FlowPostFinallyDetails {
    pub antecedent: P<FlowNode>,
    pub finally_node: SuiteNode,
    pub pre_finally_gate: FlowPreFinallyGate,
}
simple_flow_handle!(FlowPostFinally, FlowPostFinallyDetails, PostFinally);

// analyzer/codeFlowTypes.ts:159
#[derive(jsrt::Trace)]
pub struct FlowPostContextManagerLabelDetails {
    pub label: FlowLabelDetails,
    pub expressions: Vec<ExpressionNode>,
    pub is_async: bool,

    // If the context manager swallows exceptions and this value
    // is true, block any code flow analysis along this path. Conversely,
    // if the context manager does not swallow exceptions and this
    // value is false, block any code flow analysis along this path.
    pub block_if_swallows_exceptions: bool,
}
flow_handle!(
    FlowPostContextManagerLabel,
    FlowPostContextManagerLabelDetails,
    PostContextManagerLabel
);

impl FlowPostContextManagerLabel {
    pub fn d(&self) -> &'static FlowPostContextManagerLabelDetails {
        match &self.0.get().d {
            FlowNodeDetails::PostContextManagerLabel(d) => d,
            _ => unreachable!(),
        }
    }

    pub fn label(self) -> FlowLabel {
        FlowLabel(self.0)
    }
}

// analyzer/codeFlowTypes.ts:170
pub fn is_code_flow_supported_for_reference(reference: ExpressionNode) -> bool {
    if reference.node_type == ParseNodeType::Name {
        return true;
    }

    if let D::MemberAccess(d) = &reference.get().d {
        return is_code_flow_supported_for_reference(d.left_expr.get());
    }

    if reference.node_type == ParseNodeType::AssignmentExpression {
        return true;
    }

    if let D::Index(d) = &reference.get().d {
        // Allow index expressions that have a single subscript that is a
        // literal integer or string value.
        let items = d.items.borrow().clone();
        if items.len() != 1
            || d.trailing_comma.get()
            || items[0].d().name.get().is_some()
            || items[0].d().arg_category.get() != ArgCategory::Simple
        {
            return false;
        }

        let subscript_node = items[0].d().value_expr.get();
        let is_integer_index = match &subscript_node.get().d {
            D::Number(n) => !n.is_imaginary.get() && n.is_integer.get(),
            _ => false,
        };
        let is_negative_integer_index = match &subscript_node.get().d {
            D::UnaryOperation(u) => {
                u.operator.get() == OperatorType::Subtract
                    && match &u.expr.get().get().d {
                        D::Number(n) => !n.is_imaginary.get() && n.is_integer.get(),
                        _ => false,
                    }
            }
            _ => false,
        };
        let is_string_index = match &subscript_node.get().d {
            D::StringList(s) => {
                let strings = s.strings.borrow();
                strings.len() == 1 && strings[0].node_type == ParseNodeType::String
            }
            _ => false,
        };

        if !is_integer_index && !is_negative_integer_index && !is_string_index {
            return false;
        }

        return is_code_flow_supported_for_reference(d.left_expr.get());
    }

    false
}

// analyzer/codeFlowTypes.ts:223
pub fn create_key_for_reference(reference: CodeFlowReferenceExpressionNode) -> String {
    let key;
    match &reference.get().d {
        D::Name(d) => {
            key = d.value.clone();
        }
        D::AssignmentExpression(d) => {
            key = d.name.get().d().value.clone();
        }
        D::MemberAccess(d) => {
            let left_key = create_key_for_reference(d.left_expr.get());
            key = format!("{}.{}", left_key, d.member.get().d().value);
        }
        D::Index(d) => {
            let left_key = create_key_for_reference(d.left_expr.get());
            let items = d.items.borrow();
            assert(items.len() == 1, None);
            let expr = items[0].d().value_expr.get();
            drop(items);
            match &expr.get().d {
                D::Number(n) => {
                    key = format!("{}[{}]", left_key, n.value.to_js_string());
                }
                D::StringList(val_expr) => {
                    let strings = val_expr.strings.borrow();
                    assert(
                        strings.len() == 1 && strings[0].node_type == ParseNodeType::String,
                        None,
                    );
                    let D::String(s) = &strings[0].get().d else {
                        unreachable!()
                    };
                    key = format!("{}[\"{}\"]", left_key, s.value.to_string_lossy());
                }
                D::UnaryOperation(u)
                    if u.operator.get() == OperatorType::Subtract
                        && u.expr.get().node_type == ParseNodeType::Number =>
                {
                    let D::Number(n) = &u.expr.get().get().d else {
                        unreachable!()
                    };
                    key = format!("{}[-{}]", left_key, n.value.to_js_string());
                }
                _ => {
                    fail(Some("createKeyForReference received unexpected index type"));
                }
            }
        }
        _ => {
            fail(Some("createKeyForReference received unexpected expression type"));
        }
    }

    key
}

// analyzer/codeFlowTypes.ts:258
pub fn create_keys_for_reference_subexpressions(
    reference: CodeFlowReferenceExpressionNode,
) -> Vec<String> {
    match &reference.get().d {
        D::Name(_) => vec![create_key_for_reference(reference)],
        D::AssignmentExpression(d) => vec![create_key_for_reference(d.name.get().node())],
        D::MemberAccess(d) => {
            let mut keys = create_keys_for_reference_subexpressions(d.left_expr.get());
            keys.push(create_key_for_reference(reference));
            keys
        }
        D::Index(d) => {
            let mut keys = create_keys_for_reference_subexpressions(d.left_expr.get());
            keys.push(create_key_for_reference(reference));
            keys
        }
        _ => fail(Some("createKeyForReference received unexpected expression type")),
    }
}

// analyzer/codeFlowTypes.ts:285
// A reference key that corresponds to a wildcard import.
pub const WILDCARD_IMPORT_REFERENCE_KEY: &str = "*";
