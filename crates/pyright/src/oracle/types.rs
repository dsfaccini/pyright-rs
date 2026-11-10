//! Rust side of the `types` dump stage. Format: `oracle/FORMAT.md`, "types stage".

use jsrt::{FxHashMap, P};

use crate::analyzer::analyzer_node_info::get_string_annotation;
use crate::analyzer::program::Program;
use crate::analyzer::type_printer::{FunctionReturnTypeCallback, print_type};
use crate::analyzer::types::{FunctionType, Type, UnknownType};
use crate::oracle::bind::node_ref;
use crate::oracle::format;
use crate::parser::parse_nodes::{ModuleNode, ParseNode};
use crate::parser::parse_tree_utils::get_child_nodes;

/// The fork's return type callback, `(functionType) => FunctionType.getEffectiveReturnType(functionType)
/// ?? UnknownType.create()`: the specialized, declared or already-inferred return type, so printing
/// never runs return type inference.
struct OracleReturnTypeCallback;

impl FunctionReturnTypeCallback for OracleReturnTypeCallback {
    fn call(&self, type_: FunctionType) -> P<Type> {
        FunctionType::get_effective_return_type(type_, true).unwrap_or_else(|| UnknownType::create(false).0)
    }
}

/// The `types` dump of one file: the fork's `dumpTypes` (`oracle/pyright-oracle.patch`). It walks
/// `root` in walk order with every string annotation (`AnalyzerNodeInfo.getStringAnnotation` on
/// the program's reader) and prints each node's entry in the evaluator's main type cache
/// (`oracleReadTypeCacheEntry`), never running inference. `printed` memoizes printed types by
/// identity across files, as the fork's `Map<Type, string>` does.
// A `P` key hashes by address, so interior mutability in `Type` cannot move it.
#[allow(clippy::mutable_key_type)]
pub fn dump_types(program: &Program, root: ModuleNode, printed: &mut FxHashMap<P<Type>, String>) -> String {
    let evaluator = program.evaluator().expect("the types stage runs with an evaluator");
    let reader = program.analyzer_node_info_reader();
    let print_type_flags = evaluator.evaluator_options.print_type_flags;
    let return_type_callback = P::new_eval(OracleReturnTypeCallback).to_dyn::<dyn FunctionReturnTypeCallback>(|c| c);

    let mut out = String::new();
    for node in walk_tree(root.0, &|n| get_string_annotation(n, reader)) {
        // `oracleReadTypeCacheEntry`: the main type cache, keyed by node id.
        let (type_, is_incomplete, flags) = {
            let type_cache = evaluator.type_cache.get();
            let cache = type_cache.borrow();
            let Some(cache_entry) = cache.get(&node.id.get()) else {
                continue;
            };
            let type_result = cache_entry.type_result;
            (type_result.type_.get(), type_result.is_incomplete.get().unwrap_or(false), cache_entry.flags)
        };
        let printed_type = printed
            .entry(type_)
            .or_insert_with(|| print_type(type_, print_type_flags, return_type_callback))
            .clone();

        out.push_str(&node_ref(&node));
        out.push(':');
        if let Some(flags) = flags {
            // JS bitwise operators yield a signed 32-bit integer.
            out.push_str(&format!(" flags={}", flags.0.cast_signed()));
        }
        if is_incomplete {
            out.push_str(" incomplete");
        }
        out.push(' ');
        out.push_str(&format::str(&printed_type));
        out.push('\n');
    }
    out
}

/// The fork's `walkTree`: pre-order from `root`, children in `getChildNodes` order with the given
/// string annotations, skipping `undefined` entries.
fn walk_tree(
    root: P<ParseNode>,
    get_string_annotation: crate::parser::parse_tree_utils::StringAnnotationProvider<'_>,
) -> Vec<P<ParseNode>> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        result.push(node);
        let children = get_child_nodes(node, Some(get_string_annotation));
        for child in children.iter().rev().flatten() {
            stack.push(*child);
        }
    }
    result
}
