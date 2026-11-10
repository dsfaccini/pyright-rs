//! Port of `packages/pyright-internal/src/analyzer/cellChainIndex.ts`.
//!
//! Only the provider interface the binder takes. The CLI never binds notebook
//! cells, so it always passes no provider.

use crate::common::uri::Uri;
use crate::parser::parse_nodes::ModuleNode;

/** Read-only provider surface for consumers that only need later-cell lookups. */
// analyzer/cellChainIndex.ts:17
pub trait CellChainIndexProvider {
    fn get_later_module_nodes(&self, file_uri: Uri) -> Option<Vec<ModuleNode>>;
}
