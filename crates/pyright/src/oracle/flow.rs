//! The flow stage (`oracle/FORMAT.md`, "flow stage").

use jsrt::{FxHashMap, P};

use crate::analyzer::analyzer_node_info::AnalyzerNodeInfoReader;
use crate::analyzer::code_flow_types::{FlowNode, FlowNodeDetails};
use crate::analyzer::symbol::INDETERMINATE_SYMBOL_ID;
use crate::oracle::bind::{ScopeNumbering, Value, node_ref};
use crate::oracle::format::{self, DumpWriter};
use crate::parser::parse_nodes::ParseNode;

/// Flow nodes numbered in first-visit order.
struct FlowNumbering {
    numbers: FxHashMap<P<FlowNode>, usize>,
    flow_nodes: Vec<P<FlowNode>>,
}

impl FlowNumbering {
    fn visit(&mut self, flow_node: P<FlowNode>) {
        if !self.numbers.contains_key(&flow_node) {
            self.numbers.insert(flow_node, self.flow_nodes.len());
            self.flow_nodes.push(flow_node);
        }
    }

    fn reference(&self, flow_node: P<FlowNode>) -> Value {
        Value::Line(match self.numbers.get(&flow_node) {
            Some(index) => format!("F{index}"),
            None => "F?".to_string(),
        })
    }
}

/// The flow references of a flow node, in printed field order: `antecedent`,
/// `antecedents`, `preBranchAntecedent`, `preFinallyGate`.
fn flow_references(flow_node: P<FlowNode>) -> Vec<P<FlowNode>> {
    let mut refs = Vec::new();
    if let Some(antecedent) = flow_node.d().antecedent() {
        refs.push(antecedent);
    }
    match flow_node.d() {
        FlowNodeDetails::Label(d) => refs.extend(d.antecedents.borrow().iter().copied()),
        FlowNodeDetails::BranchLabel(d) => {
            refs.extend(d.label.antecedents.borrow().iter().copied());
            refs.extend(d.pre_branch_antecedent);
        }
        FlowNodeDetails::PostContextManagerLabel(d) => {
            refs.extend(d.label.antecedents.borrow().iter().copied())
        }
        FlowNodeDetails::PostFinally(d) => refs.push(d.pre_finally_gate.node()),
        _ => {}
    }
    refs
}

fn flow_node_value(flow_node: P<FlowNode>, numbering: &FlowNumbering, symbol_names: &FxHashMap<u32, String>) -> Value {
    let flow_ref = |n: P<FlowNode>| numbering.reference(n);
    let node = |n: P<ParseNode>| Value::node(n);
    let target_symbol = |id: u32| {
        Value::Line(if id == INDETERMINATE_SYMBOL_ID {
            "0".to_string()
        } else {
            symbol_names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "external".to_string())
        })
    };

    let mut fields: Vec<(&'static str, Option<Value>)> = vec![(
        "flags",
        Some(Value::number(flow_node.flags().0 as f64)),
    )];

    let label_fields = |label: &crate::analyzer::code_flow_types::FlowLabelDetails| {
        vec![
            (
                "affectedExpressions",
                label
                    .affected_expressions
                    .get()
                    .map(|set| Value::Set(set.borrow().iter().map(|s| Value::str(s)).collect())),
            ),
            (
                "antecedents",
                Some(Value::Array(
                    label.antecedents.borrow().iter().map(|a| flow_ref(*a)).collect(),
                )),
            ),
        ]
    };

    match flow_node.d() {
        FlowNodeDetails::Plain => {}
        FlowNodeDetails::Label(d) => fields.extend(label_fields(d)),
        FlowNodeDetails::BranchLabel(d) => {
            fields.extend(label_fields(&d.label));
            fields.push(("preBranchAntecedent", d.pre_branch_antecedent.map(flow_ref)));
        }
        FlowNodeDetails::Assignment(d) => fields.extend([
            ("node", Some(node(d.node))),
            ("antecedent", Some(flow_ref(d.antecedent))),
            ("targetSymbolId", Some(target_symbol(d.target_symbol_id))),
        ]),
        FlowNodeDetails::VariableAnnotation(d) => {
            fields.push(("antecedent", Some(flow_ref(d.antecedent))))
        }
        FlowNodeDetails::WildcardImport(d) => fields.extend([
            ("node", Some(node(d.node.node()))),
            (
                "names",
                Some(Value::Array(d.names.iter().map(|s| Value::str(s)).collect())),
            ),
            ("antecedent", Some(flow_ref(d.antecedent))),
        ]),
        FlowNodeDetails::Condition(d) => fields.extend([
            ("reference", d.reference.map(|r| node(r.node()))),
            ("expression", Some(node(d.expression))),
            ("antecedent", Some(flow_ref(d.antecedent))),
        ]),
        FlowNodeDetails::NarrowForPattern(d) => fields.extend([
            ("subjectExpression", Some(node(d.subject_expression))),
            ("statement", Some(node(d.statement))),
            ("antecedent", Some(flow_ref(d.antecedent))),
        ]),
        FlowNodeDetails::ExhaustedMatch(d) => fields.extend([
            ("node", Some(node(d.node.node()))),
            ("antecedent", Some(flow_ref(d.antecedent))),
            ("subjectExpression", Some(node(d.subject_expression))),
        ]),
        FlowNodeDetails::Call(d) => fields.extend([
            ("node", Some(node(d.node.node()))),
            ("antecedent", Some(flow_ref(d.antecedent))),
        ]),
        FlowNodeDetails::PreFinallyGate(d) => {
            fields.push(("antecedent", Some(flow_ref(d.antecedent))))
        }
        FlowNodeDetails::PostFinally(d) => fields.extend([
            ("finallyNode", Some(node(d.finally_node.node()))),
            ("antecedent", Some(flow_ref(d.antecedent))),
            ("preFinallyGate", Some(flow_ref(d.pre_finally_gate.node()))),
        ]),
        FlowNodeDetails::PostContextManagerLabel(d) => {
            fields.extend(label_fields(&d.label));
            fields.extend([
                (
                    "expressions",
                    Some(Value::Array(d.expressions.iter().map(|e| node(*e)).collect())),
                ),
                ("isAsync", Some(Value::bool(d.is_async))),
                (
                    "blockIfSwallowsExceptions",
                    Some(Value::bool(d.block_if_swallows_exceptions)),
                ),
            ]);
        }
    }

    Value::Object(fields)
}

/// The flow dump of one file.
pub fn dump_flow(
    walk: &[P<ParseNode>],
    scopes: &ScopeNumbering,
    reader: &dyn AnalyzerNodeInfoReader,
) -> String {
    let mut numbering = FlowNumbering {
        numbers: FxHashMap::default(),
        flow_nodes: Vec::new(),
    };

    let mut node_entries: Vec<(P<ParseNode>, Option<P<FlowNode>>, Option<P<FlowNode>>)> = Vec::new();
    for node in walk {
        let Some(info) = reader.get(*node) else {
            continue;
        };
        let after_flow_node = info.cold().and_then(|c| c.after_flow_node.get());
        let flow_node = info.flow_node.get();
        if let Some(after) = after_flow_node {
            numbering.visit(after);
        }
        if let Some(flow) = flow_node {
            numbering.visit(flow);
        }
        if after_flow_node.is_some() || flow_node.is_some() {
            node_entries.push((*node, after_flow_node, flow_node));
        }
    }

    let mut i = 0;
    while i < numbering.flow_nodes.len() {
        for reference in flow_references(numbering.flow_nodes[i]) {
            numbering.visit(reference);
        }
        i += 1;
    }

    let mut symbol_names: FxHashMap<u32, String> = FxHashMap::default();
    for (index, scope) in scopes.scopes.iter().enumerate() {
        for (name, symbol) in scope.symbol_table.entries() {
            symbol_names
                .entry(symbol.id)
                .or_insert_with(|| format!("#{index} {}", format::str(&name)));
        }
    }

    let mut w = DumpWriter::new();
    w.open(0, "nodes");
    for (node, after_flow_node, flow_node) in &node_entries {
        w.open(1, &node_ref(node));
        if let Some(after) = after_flow_node {
            numbering.reference(*after).emit(&mut w, 2, "afterFlowNode");
        }
        if let Some(flow) = flow_node {
            numbering.reference(*flow).emit(&mut w, 2, "flowNode");
        }
    }

    w.field(0, "flowNodes", &format::array_header(numbering.flow_nodes.len()));
    for (i, flow_node) in numbering.flow_nodes.iter().enumerate() {
        flow_node_value(*flow_node, &numbering, &symbol_names).emit(&mut w, 1, &format::index_key(i));
    }

    w.finish()
}
