//! Rust side of the `types` dump stage. Format: `oracle/FORMAT.md`, "types stage".

use jsrt::{FxHashMap, P};

use crate::analyzer::program::Program;
use crate::analyzer::types::Type;
use crate::parser::parse_nodes::ModuleNode;

/// The `types` dump of one file: the fork's `dumpTypes` (`oracle/pyright-oracle.patch`). It walks
/// `root` in walk order with every string annotation (`AnalyzerNodeInfo.getStringAnnotation` on
/// the program's reader) and prints each node's entry in the evaluator's main type cache
/// (`oracleReadTypeCacheEntry`), never running inference. `printed` memoizes printed types by
/// identity across files, as the fork's `Map<Type, string>` does.
// A `P` key hashes by address, so interior mutability in `Type` cannot move it.
#[allow(clippy::mutable_key_type)]
pub fn dump_types(program: &Program, root: ModuleNode, printed: &mut FxHashMap<P<Type>, String>) -> String {
    let _ = (program, root, printed);
    todo!()
}
