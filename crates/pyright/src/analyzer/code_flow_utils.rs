//! Port of `packages/pyright-internal/src/analyzer/codeFlowUtils.ts`.
//!
//! Utility functions that operate on code flow nodes and graphs.
//!
//! `formatControlFlowGraph` declares its types and helpers inside its body.
//! Here they are module items. The helpers that close over the locals
//! `links`, `nodes`, `edges` and `nodeInfo` are methods of
//! `ControlFlowGraphFormatter`, which holds those locals; the others take
//! what they close over as parameters.

// `P` hashes by address, so its interior mutability never changes a key.
#![allow(clippy::mutable_key_type)]

use jsrt::flag_ops;
use jsrt::cell::{Cell, RefCell};

use jsrt::{FxHashMap, FxHashSet, JsStr, P};

use crate::analyzer::analyzer_node_info::AnalyzerNodeInfoAccessor;
use crate::analyzer::code_flow_types::{
    FlowAssignment, FlowCall, FlowCondition, FlowFlags, FlowLabel, FlowNarrowForPattern, FlowNode,
    FlowWildcardImport,
};
use crate::common::position_utils::convert_offset_to_position;
use crate::parser::parse_nodes::ParseNode;

// analyzer/codeFlowUtils.ts:29
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoxCharacter {
    lr,
    ud,
    dr,
    dl,
    ul,
    ur,
    udr,
    udl,
    dlr,
    ulr,
    udlr,
}

impl BoxCharacter {
    fn as_str(self) -> &'static str {
        match self {
            BoxCharacter::lr => "─",
            BoxCharacter::ud => "│",
            BoxCharacter::dr => "╭",
            BoxCharacter::dl => "╮",
            BoxCharacter::ul => "╯",
            BoxCharacter::ur => "╰",
            BoxCharacter::udr => "├",
            BoxCharacter::udl => "┤",
            BoxCharacter::dlr => "┬",
            BoxCharacter::ulr => "┴",
            BoxCharacter::udlr => "╫",
        }
    }
}

// analyzer/codeFlowUtils.ts:43
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Connection(u32);

impl Connection {
    const None: Connection = Connection(0);
    const Up: Connection = Connection(1 << 0);
    const Down: Connection = Connection(1 << 1);
    const Left: Connection = Connection(1 << 2);
    const Right: Connection = Connection(1 << 3);

    const UpDown: Connection = Connection(Connection::Up.0 | Connection::Down.0);
    const LeftRight: Connection = Connection(Connection::Left.0 | Connection::Right.0);
    const UpLeft: Connection = Connection(Connection::Up.0 | Connection::Left.0);
    const UpRight: Connection = Connection(Connection::Up.0 | Connection::Right.0);
    const DownLeft: Connection = Connection(Connection::Down.0 | Connection::Left.0);
    const DownRight: Connection = Connection(Connection::Down.0 | Connection::Right.0);
    const UpDownLeft: Connection = Connection(Connection::UpDown.0 | Connection::Left.0);
    const UpDownRight: Connection = Connection(Connection::UpDown.0 | Connection::Right.0);
    const UpLeftRight: Connection = Connection(Connection::Up.0 | Connection::LeftRight.0);
    const DownLeftRight: Connection = Connection(Connection::Down.0 | Connection::LeftRight.0);
    const UpDownLeftRight: Connection = Connection(Connection::UpDown.0 | Connection::LeftRight.0);

    const NoChildren: Connection = Connection(1 << 4);
}

flag_ops!(Connection);

// analyzer/codeFlowUtils.ts:65
#[derive(jsrt::Trace)]
struct FlowGraphNode {
    // Written and never read, as in the TypeScript.
    #[allow(dead_code)]
    id: i64,
    flow_node: P<FlowNode>,
    edges: RefCell<Vec<P<FlowGraphEdge>>>,
    text: RefCell<String>,
    lane: Cell<i32>,
    end_lane: Cell<i32>,
    level: Cell<i32>,
    circular: bool,
}

// analyzer/codeFlowUtils.ts:76
#[derive(jsrt::Trace)]
struct FlowGraphEdge {
    source: P<FlowGraphNode>,
    target: P<FlowGraphNode>,
}

/// The locals of `formatControlFlowGraph` that its nested functions write.
struct ControlFlowGraphFormatter<'a> {
    node_info: &'a AnalyzerNodeInfoAccessor,
    // `Record<number, FlowGraphNode>`: looked up by id, never iterated.
    links: FxHashMap<u32, P<FlowGraphNode>>,
    nodes: Vec<P<FlowGraphNode>>,
    edges: Vec<P<FlowGraphEdge>>,
}

// analyzer/codeFlowUtils.ts:28
pub fn format_control_flow_graph(
    flow_node: P<FlowNode>,
    node_info: &AnalyzerNodeInfoAccessor,
) -> String {
    let mut graph = ControlFlowGraphFormatter {
        node_info,
        links: FxHashMap::default(),
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    let root = graph.build_graph_node(flow_node, &mut FxHashSet::default());

    for node in &graph.nodes {
        let text = graph.render_flow_node(node.flow_node, node.circular);
        *node.text.borrow_mut() = text;
        compute_level(*node);
    }

    let height = compute_height(root);
    let column_widths = graph.compute_column_widths(height);
    compute_lanes(root, 0);
    graph.render_graph(&column_widths)
}

// analyzer/codeFlowUtils.ts:96
fn get_antecedents(f: P<FlowNode>) -> Vec<P<FlowNode>> {
    if f.flags
        .get()
        .any(FlowFlags::LoopLabel | FlowFlags::BranchLabel)
    {
        return FlowLabel::cast(f).unwrap().d().antecedents.borrow().clone();
    }

    if f.flags.get().any(
        FlowFlags::Assignment
            | FlowFlags::VariableAnnotation
            | FlowFlags::WildcardImport
            | FlowFlags::TrueCondition
            | FlowFlags::FalseCondition
            | FlowFlags::TrueNeverCondition
            | FlowFlags::FalseNeverCondition
            | FlowFlags::NarrowForPattern
            | FlowFlags::ExhaustedMatch
            | FlowFlags::Call
            | FlowFlags::PreFinallyGate
            | FlowFlags::PostFinally,
    ) {
        // `FlowAssignment | FlowVariableAnnotation | FlowWildcardImport | FlowCondition
        // | FlowExhaustedMatch | FlowCall | FlowPreFinallyGate | FlowPostFinally`
        let typed_flow_node = f.get();
        return vec![typed_flow_node.antecedent().unwrap()];
    }

    vec![]
}

// analyzer/codeFlowUtils.ts:131
fn get_children(node: P<FlowGraphNode>) -> Vec<P<FlowGraphNode>> {
    let mut children: Vec<P<FlowGraphNode>> = Vec::new();
    for edge in node.edges.borrow().iter() {
        if edge.source == node {
            children.push(edge.target);
        }
    }
    children
}

// analyzer/codeFlowUtils.ts:141
fn get_parents(node: P<FlowGraphNode>) -> Vec<P<FlowGraphNode>> {
    let mut parents: Vec<P<FlowGraphNode>> = Vec::new();
    for edge in node.edges.borrow().iter() {
        if edge.target == node {
            parents.push(edge.source);
        }
    }
    parents
}

impl ControlFlowGraphFormatter<'_> {
    // analyzer/codeFlowUtils.ts:151
    fn build_graph_node(
        &mut self,
        flow_node: P<FlowNode>,
        seen: &mut FxHashSet<P<FlowNode>>,
    ) -> P<FlowGraphNode> {
        let id = flow_node.id;
        let mut graph_node = self.links.get(&id).copied();

        if graph_node.is_some() && seen.contains(&flow_node) {
            let graph_node = P::new(FlowGraphNode {
                id: -1,
                flow_node,
                edges: RefCell::new(Vec::new()),
                text: RefCell::new(String::new()),
                lane: Cell::new(-1),
                end_lane: Cell::new(-1),
                level: Cell::new(-1),
                circular: true,
            });
            self.nodes.push(graph_node);
            return graph_node;
        }
        seen.insert(flow_node);

        if graph_node.is_none() {
            let new_graph_node = P::new(FlowGraphNode {
                id: i64::from(id),
                flow_node,
                edges: RefCell::new(Vec::new()),
                text: RefCell::new(String::new()),
                lane: Cell::new(-1),
                end_lane: Cell::new(-1),
                level: Cell::new(-1),
                circular: false,
            });
            self.links.insert(id, new_graph_node);
            graph_node = Some(new_graph_node);

            self.nodes.push(new_graph_node);

            let antecedents = get_antecedents(flow_node);
            for antecedent in antecedents {
                self.build_graph_edge(new_graph_node, antecedent, seen);
            }
        }

        seen.remove(&flow_node);
        graph_node.unwrap()
    }

    // analyzer/codeFlowUtils.ts:195
    fn build_graph_edge(
        &mut self,
        source: P<FlowGraphNode>,
        antecedent: P<FlowNode>,
        seen: &mut FxHashSet<P<FlowNode>>,
    ) {
        let target = self.build_graph_node(antecedent, seen);
        let edge = P::new(FlowGraphEdge { source, target });
        self.edges.push(edge);
        source.edges.borrow_mut().push(edge);
        target.edges.borrow_mut().push(edge);
    }

    // analyzer/codeFlowUtils.ts:222
    fn compute_column_widths(&self, height: i32) -> Vec<i32> {
        // `fill(Array(height), 0)`
        let mut columns: Vec<i32> = vec![0; usize::try_from(height).unwrap()];
        for node in &self.nodes {
            let level = usize::try_from(node.level.get()).unwrap();
            columns[level] = columns[level].max(node.text.borrow().js_len());
        }
        columns
    }

    // analyzer/codeFlowUtils.ts:293
    fn get_node_text(&self, f: P<FlowNode>) -> Option<String> {
        let parse_node = get_parse_node(f)?;

        let file_info = self.node_info.get_file_info(parse_node);
        let start_pos = convert_offset_to_position(parse_node.start.get(), file_info.lines);

        Some(format!(
            "[{}:{}]",
            start_pos.line + 1,
            start_pos.character + 1
        ))
    }

    // analyzer/codeFlowUtils.ts:306
    fn render_flow_node(&self, flow_node: P<FlowNode>, circular: bool) -> String {
        let text = format!(
            "{}@{}{}",
            get_header(flow_node.flags.get()),
            flow_node.id,
            self.get_node_text(flow_node).unwrap_or_default()
        );
        if circular {
            format!("Circular({text})")
        } else {
            text
        }
    }

    // analyzer/codeFlowUtils.ts:311
    fn render_graph(&self, column_widths: &[i32]) -> String {
        let column_count = column_widths.len();
        let lane_count = usize::try_from(
            self.nodes
                .iter()
                .fold(0, |x, n| std::cmp::max(x, n.lane.get()))
                + 1,
        )
        .unwrap();
        // `fill(Array(laneCount), '')`
        let mut lanes: Vec<String> = vec![String::new(); lane_count];
        let mut grid: Vec<Vec<Option<P<FlowGraphNode>>>> = column_widths
            .iter()
            .map(|_| vec![None; lane_count])
            .collect();
        let mut connectors: Vec<Vec<Connection>> = column_widths
            .iter()
            .map(|_| vec![Connection::None; lane_count])
            .collect();

        // Build connectors.
        for node in &self.nodes {
            let node_level = usize::try_from(node.level.get()).unwrap();
            let node_lane = usize::try_from(node.lane.get()).unwrap();
            grid[node_level][node_lane] = Some(*node);
            let children = get_children(*node);
            for i in 0..children.len() {
                let child = children[i];
                let mut connector: Connection = Connection::Right;
                if child.lane.get() == node.lane.get() {
                    connector |= Connection::Left;
                }
                if i > 0 {
                    connector |= Connection::Up;
                }
                if i < children.len() - 1 {
                    connector |= Connection::Down;
                }
                connectors[node_level][usize::try_from(child.lane.get()).unwrap()] |= connector;
            }
            if children.len() == 0 {
                connectors[node_level][node_lane] |= Connection::NoChildren;
            }
            let parents = get_parents(*node);
            for i in 0..parents.len() {
                let parent = parents[i];
                let mut connector: Connection = Connection::Left;
                if i > 0 {
                    connector |= Connection::Up;
                }
                if i < parents.len() - 1 {
                    connector |= Connection::Down;
                }
                connectors[node_level - 1][usize::try_from(parent.lane.get()).unwrap()] |=
                    connector;
            }
        }

        // Fill in missing connectors.
        for column in 0..column_count {
            for lane in 0..lane_count {
                let left = if column > 0 {
                    connectors[column - 1][lane]
                } else {
                    Connection::None
                };
                let above = if lane > 0 {
                    connectors[column][lane - 1]
                } else {
                    Connection::None
                };
                let mut connector = connectors[column][lane];
                if connector == Connection::None {
                    connector = Connection::None;

                    if left.any(Connection::Right) {
                        connector |= Connection::LeftRight;
                    }
                    if above.any(Connection::Down) {
                        connector |= Connection::UpDown;
                    }
                    connectors[column][lane] = connector;
                }
            }
        }

        for column in 0..column_count {
            for lane in 0..lanes.len() {
                let connector = connectors[column][lane];
                let fill = if connector.any(Connection::Left) {
                    BoxCharacter::lr.as_str()
                } else {
                    " "
                };
                let node = grid[column][lane];
                match node {
                    None => {
                        if column < column_count - 1 {
                            write_lane(&mut lanes, lane, &repeat(fill, column_widths[column] + 1));
                        }
                    }
                    Some(node) => {
                        let text = node.text.borrow().clone();
                        write_lane(&mut lanes, lane, &text);
                        if column < column_count - 1 {
                            write_lane(&mut lanes, lane, " ");
                            write_lane(
                                &mut lanes,
                                lane,
                                &repeat(fill, column_widths[column] - text.js_len()),
                            );
                        }
                    }
                }
                write_lane(&mut lanes, lane, get_box_character(connector));
                write_lane(
                    &mut lanes,
                    lane,
                    if connector.any(Connection::Right)
                        && column < column_count - 1
                        && grid[column + 1][lane].is_none()
                    {
                        BoxCharacter::lr.as_str()
                    } else {
                        " "
                    },
                );
            }
        }

        format!("{}\n", lanes.join("\n"))
    }
}

// analyzer/codeFlowUtils.ts:203
fn compute_level(node: P<FlowGraphNode>) -> i32 {
    if node.level.get() != -1 {
        return node.level.get();
    }
    let mut level = 0;
    for parent in get_parents(node) {
        level = std::cmp::max(level, compute_level(parent) + 1);
    }
    node.level.set(level);
    level
}

// analyzer/codeFlowUtils.ts:214
fn compute_height(node: P<FlowGraphNode>) -> i32 {
    let mut height = 0;
    for child in get_children(node) {
        height = std::cmp::max(height, compute_height(child));
    }
    height + 1
}

// analyzer/codeFlowUtils.ts:230
fn compute_lanes(node: P<FlowGraphNode>, mut lane: i32) {
    if node.lane.get() == -1 {
        node.lane.set(lane);
        node.end_lane.set(lane);
        let children = get_children(node);
        for i in 0..children.len() {
            if i > 0 {
                lane += 1;
            }
            let child = children[i];
            compute_lanes(child, lane);
            if child.end_lane.get() > node.end_lane.get() {
                lane = child.end_lane.get();
            }
        }
        node.end_lane.set(lane);
    }
}

// analyzer/codeFlowUtils.ts:247
fn get_header(flags: FlowFlags) -> &'static str {
    if flags.any(FlowFlags::Start) {
        return "Start";
    }
    if flags.any(FlowFlags::BranchLabel) {
        return "Branch";
    }
    if flags.any(FlowFlags::LoopLabel) {
        return "Loop";
    }
    if flags.any(FlowFlags::Unbind) {
        return "Unbind";
    }
    if flags.any(FlowFlags::Assignment) {
        return "Assign";
    }
    if flags.any(FlowFlags::TrueCondition) {
        return "True";
    }
    if flags.any(FlowFlags::FalseCondition) {
        return "False";
    }
    if flags.any(FlowFlags::Call) {
        return "Call";
    }
    if flags.any(FlowFlags::UnreachableStaticCondition) {
        return "UnreachableStaticCondition";
    }
    if flags.any(FlowFlags::UnreachableStructural) {
        return "UnreachableStructural";
    }
    if flags.any(FlowFlags::WildcardImport) {
        return "Wildcard";
    }
    if flags.any(FlowFlags::PreFinallyGate) {
        return "PreFinal";
    }
    if flags.any(FlowFlags::PostFinally) {
        return "PostFinal";
    }
    if flags.any(FlowFlags::VariableAnnotation) {
        return "Annotate";
    }
    if flags.any(FlowFlags::TrueNeverCondition) {
        return "TrueNever";
    }
    if flags.any(FlowFlags::FalseNeverCondition) {
        return "FalseNever";
    }
    if flags.any(FlowFlags::NarrowForPattern) {
        return "Pattern";
    }
    if flags.any(FlowFlags::ExhaustedMatch) {
        return "Exhaust";
    }
    // `throw new Error()`
    panic!("Error");
}

// analyzer/codeFlowUtils.ts:269
fn get_parse_node(f: P<FlowNode>) -> Option<P<ParseNode>> {
    if f.flags.get().any(FlowFlags::Assignment) {
        return Some(FlowAssignment::cast(f).unwrap().d().node);
    }

    if f.flags.get().any(FlowFlags::WildcardImport) {
        return Some(FlowWildcardImport::cast(f).unwrap().d().node.node());
    }

    if f.flags
        .get()
        .any(FlowFlags::TrueCondition | FlowFlags::FalseCondition)
    {
        return Some(FlowCondition::cast(f).unwrap().d().expression);
    }

    if f.flags.get().any(FlowFlags::NarrowForPattern) {
        return Some(FlowNarrowForPattern::cast(f).unwrap().d().statement);
    }

    if f.flags.get().any(FlowFlags::Call) {
        return Some(FlowCall::cast(f).unwrap().d().node.node());
    }

    None
}

// analyzer/codeFlowUtils.ts:391
fn write_lane(lanes: &mut [String], lane: usize, text: &str) {
    lanes[lane] += text;
}

// analyzer/codeFlowUtils.ts:396
fn get_box_character(connector: Connection) -> &'static str {
    match connector {
        Connection::UpDown => return BoxCharacter::ud.as_str(),
        Connection::LeftRight => return BoxCharacter::lr.as_str(),
        Connection::UpLeft => return BoxCharacter::ul.as_str(),
        Connection::UpRight => return BoxCharacter::ur.as_str(),
        Connection::DownLeft => return BoxCharacter::dl.as_str(),
        Connection::DownRight => return BoxCharacter::dr.as_str(),
        Connection::UpDownLeft => return BoxCharacter::udl.as_str(),
        Connection::UpDownRight => return BoxCharacter::udr.as_str(),
        Connection::UpLeftRight => return BoxCharacter::ulr.as_str(),
        Connection::DownLeftRight => return BoxCharacter::dlr.as_str(),
        Connection::UpDownLeftRight => return BoxCharacter::udlr.as_str(),
        _ => {}
    }
    " "
}

// analyzer/codeFlowUtils.ts:435
fn repeat(ch: &str, length: i32) -> String {
    if length > 0 {
        ch.repeat(usize::try_from(length).unwrap())
    } else {
        String::new()
    }
}

// Pyright has no unit tests for this file. The expected graphs come from node
// running `codeFlowUtils.ts` on the same hand-built flow nodes.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::analyzer_node_info::{
        AnalyzerNodeInfoContextImpl, create_analyzer_node_info_accessor,
    };
    use crate::analyzer::code_flow_types::{
        FlowBranchLabelDetails, FlowConditionDetails, FlowLabelDetails, FlowNodeDetails,
        FlowPostContextManagerLabelDetails, FlowPreFinallyGateDetails,
        FlowVariableAnnotationDetails,
    };
    use crate::parser::parse_nodes::{NameNode, create_parse_tree_key};
    use crate::parser::tokenizer_types::IdentifierToken;

    fn format(flow_node: P<FlowNode>) -> String {
        let context = AnalyzerNodeInfoContextImpl::new();
        let node_info = create_analyzer_node_info_accessor(context.get(), None);
        format_control_flow_graph(flow_node, &node_info)
    }

    fn plain(flags: FlowFlags, id: u32) -> P<FlowNode> {
        FlowNode::create(flags, id, FlowNodeDetails::Plain)
    }

    fn label_details(antecedents: Vec<P<FlowNode>>) -> FlowLabelDetails {
        let label = FlowLabelDetails::new();
        label.antecedents.borrow_mut().extend(antecedents);
        label
    }

    fn branch(id: u32, antecedents: Vec<P<FlowNode>>) -> P<FlowNode> {
        FlowNode::create(
            FlowFlags::BranchLabel,
            id,
            FlowNodeDetails::BranchLabel(FlowBranchLabelDetails {
                label: label_details(antecedents),
                pre_branch_antecedent: None,
            }),
        )
    }

    fn loop_label(id: u32, antecedents: Vec<P<FlowNode>>) -> P<FlowNode> {
        FlowNode::create(
            FlowFlags::LoopLabel,
            id,
            FlowNodeDetails::Label(label_details(antecedents)),
        )
    }

    fn condition(flags: FlowFlags, id: u32, antecedent: P<FlowNode>) -> P<FlowNode> {
        let token = IdentifierToken::create(0, 1, "x".to_string(), None);
        let expression = NameNode::create(create_parse_tree_key(), token).node();
        FlowNode::create(
            flags,
            id,
            FlowNodeDetails::Condition(FlowConditionDetails {
                expression,
                reference: None,
                antecedent,
            }),
        )
    }

    #[test]
    fn single_node() {
        assert_eq!(format(plain(FlowFlags::Start, 1)), "Start@1  \n");
    }

    #[test]
    fn labels_gates_and_circular_loop() {
        let start = plain(FlowFlags::Start, 1);
        let gate = FlowNode::create(
            FlowFlags::PreFinallyGate,
            2,
            FlowNodeDetails::PreFinallyGate(FlowPreFinallyGateDetails { antecedent: start }),
        );
        let unreachable = plain(FlowFlags::UnreachableStructural, 3);
        let branch_label = branch(4, vec![gate, start, unreachable]);
        let loop_node = loop_label(5, vec![branch_label]);
        let annotate = FlowNode::create(
            FlowFlags::VariableAnnotation,
            6,
            FlowNodeDetails::VariableAnnotation(FlowVariableAnnotationDetails {
                antecedent: loop_node,
            }),
        );
        FlowLabel::cast(loop_node)
            .unwrap()
            .d()
            .antecedents
            .borrow_mut()
            .push(annotate);
        let context_manager_label = FlowNode::create(
            FlowFlags::PostContextManager | FlowFlags::BranchLabel,
            7,
            FlowNodeDetails::PostContextManagerLabel(FlowPostContextManagerLabelDetails {
                label: label_details(vec![annotate, branch_label]),
                expressions: Vec::new(),
                is_async: false,
                block_if_swallows_exceptions: false,
            }),
        );

        assert_eq!(
            format(context_manager_label),
            "Branch@7 ╫ Annotate@6 ─ Loop@5 ╫ Branch@4 ────────────╫ PreFinal@2 ─────────────╫ Start@1  \n         │                     │                      │                         │   \n         │                     │                      ╰ UnreachableStructural@3     \n         │                     ╰ Circular(Annotate@6)                               \n"
        );
    }

    #[test]
    fn conditions_and_nested_loop() {
        let start = plain(FlowFlags::Start, 1);
        let true_never = condition(FlowFlags::TrueNeverCondition, 2, start);
        let false_never = condition(FlowFlags::FalseNeverCondition, 3, start);
        let unreachable = plain(FlowFlags::UnreachableStaticCondition, 4);
        let join = branch(5, vec![true_never, false_never, unreachable]);
        let loop_node = loop_label(6, vec![join]);
        let inner = branch(7, vec![loop_node, join]);
        FlowLabel::cast(loop_node)
            .unwrap()
            .d()
            .antecedents
            .borrow_mut()
            .push(inner);

        assert_eq!(
            format(inner),
            "Branch@7 ╫ Loop@6 ╫ Branch@5 ──────────┬ TrueNever@2 ─────────────────┬ Start@1  \n         │        │                    ├ FalseNever@3 ────────────────╯   \n         │        │                    ╰ UnreachableStaticCondition@4     \n         │        ╰ Circular(Branch@7)                                    \n"
        );
    }
}
