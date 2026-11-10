//! Port of `packages/pyright-internal/src/analyzer/scopeUtils.ts`.
//!
//! Static utility methods related to scopes and their related
//! symbol tables.

use jsrt::P;

use super::analyzer_node_info::{AnalyzerNodeInfoReader, get_scope};
use super::parse_tree_utils::get_evaluation_scope_node;
use super::scope::{Scope, ScopeType};
use crate::parser::parse_nodes::{EvaluationScopeNode, ParseNode};

// analyzer/scopeUtils.ts:16
pub fn get_built_in_scope(current_scope: P<Scope>) -> P<Scope> {
    // Starting at the current scope, find the built-in scope, which should
    // be the top-most parent.
    let mut built_in_scope = current_scope;

    while built_in_scope.type_ != ScopeType::Builtin {
        built_in_scope = built_in_scope.parent.unwrap();
    }

    built_in_scope
}

// Locates the evaluation scope associated with the specified parse node.
// analyzer/scopeUtils.ts:29
pub fn get_scope_for_node(
    node: P<ParseNode>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Option<P<Scope>> {
    let scope_node = get_evaluation_scope_node(node, node_info).node;
    get_scope(scope_node, node_info)
}

// Returns a list of scopes associated with the node and its ancestor nodes.
// If stopScope is provided, the search will stop at that scope.
// Returns undefined if stopScope is not found.
// analyzer/scopeUtils.ts:37
pub fn get_scope_hierarchy(
    node: P<ParseNode>,
    stop_scope: Option<P<Scope>>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Option<Vec<P<Scope>>> {
    let mut scope_hierarchy: Vec<P<Scope>> = vec![];
    let mut cur_node: Option<P<ParseNode>> = Some(node);

    while let Some(cur) = cur_node {
        let scope_node: EvaluationScopeNode = get_evaluation_scope_node(cur, node_info).node;
        let cur_scope = get_scope(scope_node, node_info)?;

        if scope_hierarchy.len() == 0 || scope_hierarchy[scope_hierarchy.len() - 1] != cur_scope {
            scope_hierarchy.push(cur_scope);
        }

        if Some(cur_scope) == stop_scope {
            return Some(scope_hierarchy);
        }

        cur_node = scope_node.parent.get();
    }

    if stop_scope.is_some() {
        None
    } else {
        Some(scope_hierarchy)
    }
}

// Walks up the parse tree from the specified node to find the top-most node
// that is within specified scope.
// analyzer/scopeUtils.ts:69
pub fn find_top_node_in_scope(
    node: P<ParseNode>,
    scope: P<Scope>,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> Option<P<ParseNode>> {
    let mut cur_node: Option<P<ParseNode>> = Some(node);
    let mut prev_node: Option<P<ParseNode>> = None;
    let mut found_scope = false;

    while let Some(cur) = cur_node {
        if get_scope(cur, node_info) == Some(scope) {
            found_scope = true;
        } else if found_scope {
            return prev_node;
        }

        prev_node = Some(cur);
        cur_node = cur.parent.get();
    }

    None
}

// analyzer/scopeUtils.ts:92
pub fn is_scope_contained_within(scope: P<Scope>, potential_parent_scope: P<Scope>) -> bool {
    let mut cur_scope: Option<P<Scope>> = Some(scope);

    while let Some(cur) = cur_scope {
        if cur.parent == Some(potential_parent_scope) {
            return true;
        }

        cur_scope = cur.parent;
    }

    false
}
