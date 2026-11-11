//! Port of `packages/pyright-internal/src/analyzer/analyzerNodeInfo.ts`.
//!
//! The `WeakMap`s keyed by parse tree keys and nodes become lookup-only hash
//! maps; nothing iterates them. Readers and writers are `&'static dyn` trait
//! objects: every store, session and context lives in the arena.

// `P` hashes by address, so its interior mutability never changes a key.
#![allow(clippy::mutable_key_type)]

use jsrt::cell::{Cell, RefCell};

use jsrt::{FxHashMap, FxHashSet, JsSet, P};

use super::analyzer_file_info::AnalyzerFileInfo;
use super::code_flow_types::{FlowFlags, FlowNode};
use super::declaration::Declaration;
use super::import_result::ImportResult;
use super::scope::Scope;
use crate::parser::parse_nodes::{
    ExpressionNode, ModuleNode, ParseNode, ParseTreeKey, StringListNode, StringNode,
    get_parse_tree_root, get_parser_string_annotation,
};
use crate::parser::string_annotation_info::StringAnnotationInfo;

// analyzer/analyzerNodeInfo.ts:34
#[derive(jsrt::Trace)]
pub struct DunderAllInfo {
    pub names: std::rc::Rc<[String]>,
    pub string_nodes: Vec<StringNode>,
    pub uses_unsupported_dunder_all_form: bool,
}

/// `WeakMap<StringListNode, ExpressionNode>`: one owner's discovered annotations.
pub type OwnerStringAnnotations = FxHashMap<StringListNode, ExpressionNode>;

// analyzer/analyzerNodeInfo.ts:40
/// `flowNode` stays inline: the binder sets it on most nodes, and on nine of ten
/// it is the only field set. The other fields live in `AnalyzerNodeInfoCold`,
/// allocated on the first write to any of them.
#[derive(Default)]
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfo {
    // Control flow information for this node.
    pub flow_node: Cell<Option<P<FlowNode>>>,

    cold: Cell<Option<P<AnalyzerNodeInfoCold>>>,
}
const _: () = assert!(size_of::<AnalyzerNodeInfo>() <= 16);

/// The fields of `AnalyzerNodeInfo` that few nodes set.
#[derive(Default)]
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfoCold {
    //---------------------------------------------------------------
    // Set as part of import resolution

    // Information about an import; used for import nodes only.
    pub import_info: Cell<Option<P<ImportResult>>>,

    //---------------------------------------------------------------
    // Set by Binder

    // Scope for nodes that introduce scopes: modules, functions,
    // classes, lambdas, and list comprehensions. A scope is used
    // to store symbol names and their associated types and declarations.
    pub scope: Cell<Option<P<Scope>>>,

    // Declaration (for functions and classes only).
    /// Set by the binder only: a program handle.
    #[trace(program)]
    pub declaration: Cell<Option<P<Declaration>>>,

    // Control flow information at the end of this node.
    pub after_flow_node: Cell<Option<P<FlowNode>>>,

    // Set of expressions used within an execution scope (module,
    // function or lambda) that requires code flow analysis.
    pub code_flow_expressions: Cell<Option<P<RefCell<JsSet<String>>>>>,

    // Number that represents the complexity of a function's code
    // flow graph.
    pub code_flow_complexity: Cell<Option<f64>>,

    // Statically evaluated value of an if statement's condition.
    pub static_condition_value: Cell<Option<bool>>,

    // List of __all__ symbols in the module.
    pub dunder_all_info: Cell<Option<P<DunderAllInfo>>>,

    // String annotations discovered after parsing, keyed by analyzer owner.
    pub string_annotations:
        RefCell<Option<FxHashMap<P<AnalyzerFileInfo>, OwnerStringAnnotations>>>,
}

impl AnalyzerNodeInfo {
    /// The rarely set fields, or `None` when none was written.
    pub fn cold(&self) -> Option<P<AnalyzerNodeInfoCold>> {
        self.cold.get()
    }

    /// The rarely set fields, allocated on the first call.
    pub fn cold_mut(&self) -> P<AnalyzerNodeInfoCold> {
        if let Some(cold) = self.cold.get() {
            return cold;
        }
        let cold = P::new(AnalyzerNodeInfoCold::default());
        self.cold.set(Some(cold));
        cold
    }
}

// analyzer/analyzerNodeInfo.ts:82
pub trait AnalyzerNodeInfoReader {
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>>;
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>>;
}

/// Readers are program objects (`AnalyzerNodeInfoContextImpl`); a `P` to one
/// never points into the evaluator heap.
unsafe impl jsrt::Trace for dyn AnalyzerNodeInfoReader {
    const HOLDS_EVAL: bool = false;
    const EVAL: bool = false;
    fn trace(&self, _: &mut jsrt::Tracer) {}
}

// analyzer/analyzerNodeInfo.ts:107
pub trait AnalyzerNodeInfoWriter: AnalyzerNodeInfoReader {
    fn get_or_create(&self, node: P<ParseNode>) -> P<AnalyzerNodeInfo>;
    fn set_file_info(&self, root: ModuleNode, file_info: P<AnalyzerFileInfo>);
}

// analyzer/analyzerNodeInfo.ts:112
#[derive(Default)]
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfoStore {
    /// `_infoByTree`, keyed by node id where it can be: no two nodes share a nonzero id
    /// (`get_next_node_id`), and a parser numbers its tree's nodes consecutively, starting
    /// with the module node. `_dense[i]` holds the info of the node with id `_dense_base + i`.
    _dense: RefCell<Vec<Option<P<AnalyzerNodeInfo>>>>,
    _dense_base: Cell<u32>,
    /// The nodes outside the dense range, keyed by node: those with id 0 (the parts of a
    /// decorator dummy class) and those far from the range, such as nodes parsed later from
    /// string annotations.
    _sparse: RefCell<FxHashMap<P<ParseNode>, P<AnalyzerNodeInfo>>>,
    _file_info_by_tree: RefCell<FxHashMap<ParseTreeKey, P<AnalyzerFileInfo>>>,
}

impl AnalyzerNodeInfoStore {
    pub fn new() -> P<AnalyzerNodeInfoStore> {
        P::new(AnalyzerNodeInfoStore::default())
    }

    /// The `_dense` index of `id`, when the id can live there.
    fn _dense_index(&self, id: u32) -> Option<usize> {
        if id == 0 || id < self._dense_base.get() {
            return None;
        }
        usize::try_from(id - self._dense_base.get()).ok()
    }
}

impl AnalyzerNodeInfoReader for AnalyzerNodeInfoStore {
    // analyzer/analyzerNodeInfo.ts:116
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        if let Some(index) = self._dense_index(node.id.get()) {
            if let Some(Some(info)) = self._dense.borrow().get(index) {
                return Some(*info);
            }
        }
        let sparse = self._sparse.borrow();
        if sparse.is_empty() {
            return None;
        }
        sparse.get(&node).copied()
    }

    // analyzer/analyzerNodeInfo.ts:120
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        self._file_info_by_tree.borrow().get(&node.a).copied()
    }
}

impl AnalyzerNodeInfoWriter for AnalyzerNodeInfoStore {
    // analyzer/analyzerNodeInfo.ts:124
    fn get_or_create(&self, node: P<ParseNode>) -> P<AnalyzerNodeInfo> {
        if let Some(info) = self.get(node) {
            return info;
        }
        let info = P::new(AnalyzerNodeInfo::default());
        let id = node.id.get();
        let mut dense = self._dense.borrow_mut();
        if dense.is_empty() && id != 0 {
            self._dense_base.set(id);
        }
        // The range grows to cover ids near it; a far id would leave a long run of empty slots.
        match self._dense_index(id) {
            Some(index) if index <= 2 * dense.len() + 1024 => {
                if index >= dense.len() {
                    dense.resize(index + 1, None);
                }
                dense[index] = Some(info);
            }
            _ => {
                self._sparse.borrow_mut().insert(node, info);
            }
        }
        info
    }

    // analyzer/analyzerNodeInfo.ts:139
    fn set_file_info(&self, root: ModuleNode, file_info: P<AnalyzerFileInfo>) {
        self._file_info_by_tree.borrow_mut().insert(root.a, file_info);
    }
}

// analyzer/analyzerNodeInfo.ts:144
pub trait AnalyzerNodeInfoBindingSession: AnalyzerNodeInfoWriter {
    fn root(&self) -> ModuleNode;
}

// analyzer/analyzerNodeInfo.ts:148
pub trait AnalyzerNodeInfoContext: AnalyzerNodeInfoReader {
    fn get_current_layer_reader(&self) -> &'static dyn AnalyzerNodeInfoReader;
    fn begin_write(&self, root: ModuleNode) -> P<AnalyzerNodeInfoBindingSessionImpl>;
    fn publish(&self, session: P<AnalyzerNodeInfoBindingSessionImpl>) -> P<AnalyzerNodeInfoStore>;
    fn register_store(&self, root: ModuleNode, store: P<AnalyzerNodeInfoStore>);
    fn remove(&self, root: ModuleNode);
    fn retain_removed_store(&self, root: ModuleNode);
    fn enter_overlay(&self);
    // Preserves an overlay-produced store for a tree that survives the overlay.
    fn promote_to_previous_layer(&self, root: ModuleNode);
    fn discard_overlay(&self);
    fn dispose(&self);
}

// analyzer/analyzerNodeInfo.ts:162
#[derive(Default)]
#[derive(jsrt::Trace)]
struct AnalyzerNodeInfoContextLayer {
    stores: FxHashMap<ParseTreeKey, P<AnalyzerNodeInfoStore>>,
    removed_keys: FxHashSet<ParseTreeKey>,
}

// analyzer/analyzerNodeInfo.ts:173
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfoBindingSessionImpl {
    _store: P<AnalyzerNodeInfoStore>,
    _published: Cell<bool>,
    pub root: ModuleNode,
    _context: P<AnalyzerNodeInfoContextImpl>,
}

impl AnalyzerNodeInfoBindingSessionImpl {
    // analyzer/analyzerNodeInfo.ts:177
    fn new(
        root: ModuleNode,
        context: P<AnalyzerNodeInfoContextImpl>,
    ) -> P<AnalyzerNodeInfoBindingSessionImpl> {
        P::new(AnalyzerNodeInfoBindingSessionImpl {
            _store: AnalyzerNodeInfoStore::new(),
            _published: Cell::new(false),
            root,
            _context: context,
        })
    }

    // analyzer/analyzerNodeInfo.ts:205
    fn publish(&self, context: P<AnalyzerNodeInfoContextImpl>) -> P<AnalyzerNodeInfoStore> {
        if context != self._context {
            panic!("Cannot publish analyzer information to a different context");
        }

        if self._published.get() {
            panic!("Analyzer information binding session was already published");
        }

        self._published.set(true);
        self._store
    }

    // analyzer/analyzerNodeInfo.ts:218
    fn _verify_writable(&self, node: P<ParseNode>) {
        if self._published.get() {
            panic!("Cannot write analyzer information after publication");
        }

        if node.a != self.root.a {
            panic!("Cannot write analyzer information for a foreign parse tree");
        }
    }
}

impl AnalyzerNodeInfoReader for AnalyzerNodeInfoBindingSessionImpl {
    // analyzer/analyzerNodeInfo.ts:181
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        if node.a == self.root.a {
            return (*self._store).get(node);
        }

        (*self._context).get(node)
    }

    // analyzer/analyzerNodeInfo.ts:189
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        if node.a == self.root.a {
            return self._store.get_file_info(node);
        }

        self._context.get_file_info(node)
    }
}

impl AnalyzerNodeInfoWriter for AnalyzerNodeInfoBindingSessionImpl {
    // analyzer/analyzerNodeInfo.ts:197
    fn get_or_create(&self, node: P<ParseNode>) -> P<AnalyzerNodeInfo> {
        self._verify_writable(node);
        self._store.get_or_create(node)
    }

    // analyzer/analyzerNodeInfo.ts:202
    fn set_file_info(&self, root: ModuleNode, file_info: P<AnalyzerFileInfo>) {
        self._verify_writable(root.node());
        self._store.set_file_info(root, file_info);
    }
}

impl AnalyzerNodeInfoBindingSession for AnalyzerNodeInfoBindingSessionImpl {
    fn root(&self) -> ModuleNode {
        self.root
    }
}

/// `PYRIGHT_RS_NODE_INFO_STORE_CACHE`: `on` (the default) lets `_store_for` reuse its last
/// answer, `off` looks every store up, and `check` looks up again on every reuse and panics
/// when the answers differ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StoreCacheMode {
    Off,
    On,
    Check,
}

fn store_cache_mode() -> StoreCacheMode {
    static MODE: std::sync::OnceLock<StoreCacheMode> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("PYRIGHT_RS_NODE_INFO_STORE_CACHE").as_deref() {
        Err(_) | Ok("") | Ok("on") => StoreCacheMode::On,
        Ok("off") => StoreCacheMode::Off,
        Ok("check") => StoreCacheMode::Check,
        Ok(other) => panic!("PYRIGHT_RS_NODE_INFO_STORE_CACHE must be on, off or check, not {other:?}"),
    })
}

// analyzer/analyzerNodeInfo.ts:229
/// `_retiredStores`, the symbol-keyed property TypeScript hangs on a tree
/// key, is the `_retired_stores` map: tree key to this context's store.
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfoContextImpl {
    _layers: RefCell<Vec<AnalyzerNodeInfoContextLayer>>,
    /// The last answer of `_store_for`, which pyright lacks: consecutive reads mostly ask about
    /// one tree. Every write to `_layers` or `_retired_stores` clears it (`store_cache_mode`).
    _last_store: Cell<Option<(ParseTreeKey, Option<P<AnalyzerNodeInfoStore>>)>>,
    _disposed: Cell<bool>,
    _current_layer_reader: Cell<Option<&'static CurrentLayerReader>>,
    _retired_stores: RefCell<FxHashMap<ParseTreeKey, P<AnalyzerNodeInfoStore>>>,
    _self: Cell<Option<P<AnalyzerNodeInfoContextImpl>>>,
}

/// The `_currentLayerReader` object literal.
#[derive(jsrt::Trace)]
pub struct CurrentLayerReader {
    context: P<AnalyzerNodeInfoContextImpl>,
}

impl AnalyzerNodeInfoReader for CurrentLayerReader {
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        self.context._get_from_current_layer(node)
    }

    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        self.context._get_file_info_from_current_layer(node)
    }
}

impl AnalyzerNodeInfoContextImpl {
    pub fn new() -> P<AnalyzerNodeInfoContextImpl> {
        let context = P::new(AnalyzerNodeInfoContextImpl {
            _layers: RefCell::new(vec![AnalyzerNodeInfoContextImpl::_create_layer()]),
            _last_store: Cell::new(None),
            _disposed: Cell::new(false),
            _current_layer_reader: Cell::new(None),
            _retired_stores: RefCell::new(FxHashMap::default()),
            _self: Cell::new(None),
        });
        context._self.set(Some(context));
        context
            ._current_layer_reader
            .set(Some(P::new(CurrentLayerReader { context }).get()));
        context
    }

    fn handle(&self) -> P<AnalyzerNodeInfoContextImpl> {
        self._self.get().unwrap()
    }

    /// `_look_up_store`, reusing the last answer when `store_cache_mode` allows.
    fn _store_for(&self, key: ParseTreeKey) -> Option<P<AnalyzerNodeInfoStore>> {
        let mode = store_cache_mode();
        if mode != StoreCacheMode::Off {
            if let Some((cached_key, store)) = self._last_store.get() {
                if cached_key == key {
                    if mode == StoreCacheMode::Check {
                        assert_eq!(store, self._look_up_store(key), "the cached analyzer info store is stale");
                    }
                    return store;
                }
            }
        }
        let store = self._look_up_store(key);
        if mode != StoreCacheMode::Off {
            self._last_store.set(Some((key, store)));
        }
        store
    }

    /// The layer walk that `get` and `getFileInfo` share (analyzerNodeInfo.ts:237, 266).
    fn _look_up_store(&self, key: ParseTreeKey) -> Option<P<AnalyzerNodeInfoStore>> {
        let layers = self._layers.borrow();

        // Fast path: no edit-mode overlay is active (the steady-state batch/check
        // case always has just the base layer). Skip the layer-walk loop and the
        // removedKeys tombstone check, but include metadata retained by a parse
        // tree that remains reachable after its source file drops it.
        if layers.len() == 1 {
            return layers[0]
                .stores
                .get(&key)
                .copied()
                .or_else(|| self._get_retired_store(key));
        }

        for layer in layers.iter().rev() {
            if layer.removed_keys.contains(&key) {
                return self._get_retired_store(key);
            }

            if let Some(store) = layer.stores.get(&key) {
                return Some(*store);
            }
        }

        self._get_retired_store(key)
    }

    // analyzer/analyzerNodeInfo.ts:377
    fn _create_layer() -> AnalyzerNodeInfoContextLayer {
        AnalyzerNodeInfoContextLayer::default()
    }

    // analyzer/analyzerNodeInfo.ts:384
    fn _get_retired_store(&self, key: ParseTreeKey) -> Option<P<AnalyzerNodeInfoStore>> {
        self._retired_stores.borrow().get(&key).copied()
    }

    // analyzer/analyzerNodeInfo.ts:388
    fn _delete_retired_store(&self, key: ParseTreeKey) {
        self._last_store.set(None);
        self._retired_stores.borrow_mut().remove(&key);
    }

    // analyzer/analyzerNodeInfo.ts:392
    fn _get_from_current_layer(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        if self._disposed.get() {
            return None;
        }

        let store = {
            let layers = self._layers.borrow();
            let layer = layers.last().unwrap();
            if layer.removed_keys.contains(&node.a) {
                return None;
            }
            layer.stores.get(&node.a).copied()
        };

        store.and_then(|store| (*store).get(node))
    }

    // analyzer/analyzerNodeInfo.ts:405
    fn _get_file_info_from_current_layer(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        if self._disposed.get() {
            return None;
        }

        let store = {
            let layers = self._layers.borrow();
            let layer = layers.last().unwrap();
            if layer.removed_keys.contains(&node.a) {
                return None;
            }
            layer.stores.get(&node.a).copied()
        };

        store.and_then(|store| store.get_file_info(node))
    }

    // analyzer/analyzerNodeInfo.ts:418
    fn _throw_if_disposed(&self) {
        if self._disposed.get() {
            panic!("Analyzer node information context is disposed");
        }
    }
}

impl AnalyzerNodeInfoReader for AnalyzerNodeInfoContextImpl {
    // analyzer/analyzerNodeInfo.ts:237
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        if self._disposed.get() {
            return None;
        }

        self._store_for(node.a).and_then(|store| (*store).get(node))
    }

    // analyzer/analyzerNodeInfo.ts:266
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        if self._disposed.get() {
            return None;
        }

        self._store_for(node.a)
            .and_then(|store| store.get_file_info(node))
    }
}

impl AnalyzerNodeInfoContext for AnalyzerNodeInfoContextImpl {
    // analyzer/analyzerNodeInfo.ts:291
    fn get_current_layer_reader(&self) -> &'static dyn AnalyzerNodeInfoReader {
        self._current_layer_reader.get().unwrap()
    }

    // analyzer/analyzerNodeInfo.ts:295
    fn begin_write(&self, root: ModuleNode) -> P<AnalyzerNodeInfoBindingSessionImpl> {
        self._throw_if_disposed();
        AnalyzerNodeInfoBindingSessionImpl::new(root, self.handle())
    }

    // analyzer/analyzerNodeInfo.ts:300
    fn publish(&self, session: P<AnalyzerNodeInfoBindingSessionImpl>) -> P<AnalyzerNodeInfoStore> {
        self._throw_if_disposed();

        let store = session.publish(self.handle());
        self.register_store(session.root, store);
        store
    }

    // analyzer/analyzerNodeInfo.ts:311
    fn register_store(&self, root: ModuleNode, store: P<AnalyzerNodeInfoStore>) {
        self._last_store.set(None);
        self._throw_if_disposed();
        {
            let mut layers = self._layers.borrow_mut();
            let layer = layers.last_mut().unwrap();
            layer.removed_keys.remove(&root.a);
            layer.stores.insert(root.a, store);
        }
        self._delete_retired_store(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:319
    fn remove(&self, root: ModuleNode) {
        self._last_store.set(None);
        self._throw_if_disposed();
        {
            let mut layers = self._layers.borrow_mut();
            let layer = layers.last_mut().unwrap();
            layer.stores.remove(&root.a);
            layer.removed_keys.insert(root.a);
        }
        self._delete_retired_store(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:327
    fn retain_removed_store(&self, root: ModuleNode) {
        self._last_store.set(None);
        self._throw_if_disposed();
        let mut layers = self._layers.borrow_mut();
        let layer = layers.last_mut().unwrap();
        if let Some(store) = layer.stores.get(&root.a).copied() {
            self._retired_stores.borrow_mut().insert(root.a, store);
        }
        layer.stores.remove(&root.a);
        layer.removed_keys.insert(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:340
    fn enter_overlay(&self) {
        self._last_store.set(None);
        self._throw_if_disposed();
        self._layers
            .borrow_mut()
            .push(AnalyzerNodeInfoContextImpl::_create_layer());
    }

    // analyzer/analyzerNodeInfo.ts:345
    fn promote_to_previous_layer(&self, root: ModuleNode) {
        self._last_store.set(None);
        self._throw_if_disposed();
        let mut layers = self._layers.borrow_mut();
        if layers.len() < 2 {
            return;
        }

        let top_index = layers.len() - 1;
        let Some(store) = layers[top_index].stores.get(&root.a).copied() else {
            return;
        };

        let previous = &mut layers[top_index - 1];
        previous.removed_keys.remove(&root.a);
        previous.stores.insert(root.a, store);
        layers[top_index].stores.remove(&root.a);
    }

    // analyzer/analyzerNodeInfo.ts:363
    fn discard_overlay(&self) {
        self._last_store.set(None);
        self._throw_if_disposed();
        let mut layers = self._layers.borrow_mut();
        if layers.len() == 1 {
            panic!("Cannot discard the base analyzer information context layer");
        }

        layers.pop();
    }

    // analyzer/analyzerNodeInfo.ts:372
    fn dispose(&self) {
        self._last_store.set(None);
        self._layers.borrow_mut().clear();
        self._disposed.set(true);
    }
}

// analyzer/analyzerNodeInfo.ts:425
/// `ModuleNode | ClassNode | FunctionNode | LambdaNode | ComprehensionNode`.
pub type ScopedNode = P<ParseNode>;

// analyzer/analyzerNodeInfo.ts:427
#[derive(Clone, Copy)]
#[derive(jsrt::Trace)]
pub struct AnalyzerNodeInfoAccessor {
    _reader: &'static dyn AnalyzerNodeInfoReader,
    _writer: Option<&'static dyn AnalyzerNodeInfoWriter>,
}

impl AnalyzerNodeInfoAccessor {
    // analyzer/analyzerNodeInfo.ts:428
    pub fn new(
        reader: &'static dyn AnalyzerNodeInfoReader,
        writer: Option<&'static dyn AnalyzerNodeInfoWriter>,
    ) -> AnalyzerNodeInfoAccessor {
        AnalyzerNodeInfoAccessor {
            _reader: reader,
            _writer: writer,
        }
    }

    pub fn reader(&self) -> &'static dyn AnalyzerNodeInfoReader {
        self._reader
    }

    // analyzer/analyzerNodeInfo.ts:434
    pub fn get_import_info(&self, node: impl Into<P<ParseNode>>) -> Option<P<ImportResult>> {
        get_import_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:438
    pub fn set_import_info(&self, node: impl Into<P<ParseNode>>, import_info: P<ImportResult>) {
        set_import_info(node, import_info, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:442
    pub fn get_scope(&self, node: impl Into<P<ParseNode>>) -> Option<P<Scope>> {
        get_scope(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:446
    pub fn set_scope(&self, node: impl Into<P<ParseNode>>, scope: P<Scope>) {
        set_scope(node, scope, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:450
    pub fn get_declaration(&self, node: impl Into<P<ParseNode>>) -> Option<P<Declaration>> {
        get_declaration(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:454
    pub fn set_declaration(&self, node: impl Into<P<ParseNode>>, declaration: P<Declaration>) {
        set_declaration(node, declaration, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:458
    pub fn get_static_condition_value(&self, node: impl Into<P<ParseNode>>) -> Option<bool> {
        get_static_condition_value(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:462
    pub fn set_static_condition_value(&self, node: impl Into<P<ParseNode>>, value: Option<bool>) {
        set_static_condition_value(node, value, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:466
    pub fn get_flow_node(&self, node: impl Into<P<ParseNode>>) -> Option<P<FlowNode>> {
        get_flow_node(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:470
    pub fn set_flow_node(&self, node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>) {
        set_flow_node(node, flow_node, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:474
    pub fn get_after_flow_node(&self, node: impl Into<P<ParseNode>>) -> Option<P<FlowNode>> {
        get_after_flow_node(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:478
    pub fn set_after_flow_node(&self, node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>) {
        set_after_flow_node(node, flow_node, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:482
    pub fn get_file_info_if_available(&self, node: impl Into<P<ParseNode>>) -> Option<P<AnalyzerFileInfo>> {
        get_file_info_if_available(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:486
    pub fn get_file_info(&self, node: P<ParseNode>) -> P<AnalyzerFileInfo> {
        get_file_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:490
    pub fn set_file_info(&self, node: ModuleNode, file_info: P<AnalyzerFileInfo>) {
        set_file_info(node, file_info, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:494
    pub fn get_code_flow_expressions(&self, node: impl Into<P<ParseNode>>) -> Option<P<RefCell<JsSet<String>>>> {
        get_code_flow_expressions(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:498
    pub fn set_code_flow_expressions(&self, node: impl Into<P<ParseNode>>, expressions: P<RefCell<JsSet<String>>>) {
        set_code_flow_expressions(node, expressions, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:502
    pub fn get_code_flow_complexity(&self, node: impl Into<P<ParseNode>>) -> f64 {
        get_code_flow_complexity(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:506
    pub fn set_code_flow_complexity(&self, node: impl Into<P<ParseNode>>, complexity: f64) {
        set_code_flow_complexity(node, complexity, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:510
    pub fn get_dunder_all_info(&self, node: impl Into<P<ParseNode>>) -> Option<P<DunderAllInfo>> {
        get_dunder_all_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:514
    pub fn set_dunder_all_info(&self, node: impl Into<P<ParseNode>>, names: Option<P<DunderAllInfo>>) {
        set_dunder_all_info(node, names, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:518
    pub fn get_string_annotation(&self, node: StringListNode) -> Option<ExpressionNode> {
        get_string_annotation(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:522
    pub fn set_string_annotation(
        &self,
        node: StringListNode,
        annotation: ExpressionNode,
        nested_annotations: &StringAnnotationInfo,
    ) {
        set_string_annotation(node, annotation, nested_annotations, self._reader);
    }

    // analyzer/analyzerNodeInfo.ts:526
    pub fn is_code_unreachable(&self, node: impl Into<P<ParseNode>>) -> bool {
        is_code_unreachable(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:530
    fn _get_writer(&self) -> &'static dyn AnalyzerNodeInfoWriter {
        match self._writer {
            Some(writer) => writer,
            None => panic!("Analyzer node information accessor is read-only"),
        }
    }
}

// analyzer/analyzerNodeInfo.ts:543
pub fn create_analyzer_node_info_accessor(
    reader: &'static dyn AnalyzerNodeInfoReader,
    writer: Option<&'static dyn AnalyzerNodeInfoWriter>,
) -> AnalyzerNodeInfoAccessor {
    AnalyzerNodeInfoAccessor::new(reader, writer)
}

// analyzer/analyzerNodeInfo.ts:547
pub fn get_import_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<ImportResult>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.import_info.get())
}

// analyzer/analyzerNodeInfo.ts:552
pub fn set_import_info(node: impl Into<P<ParseNode>>, import_info: P<ImportResult>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().import_info.set(Some(import_info));
}

// analyzer/analyzerNodeInfo.ts:557
pub fn get_scope(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<Scope>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.scope.get())
}

// analyzer/analyzerNodeInfo.ts:562
pub fn set_scope(node: impl Into<P<ParseNode>>, scope: P<Scope>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().scope.set(Some(scope));
}

// analyzer/analyzerNodeInfo.ts:567
pub fn get_declaration(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<Declaration>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.declaration.get())
}

// analyzer/analyzerNodeInfo.ts:572
pub fn set_declaration(node: impl Into<P<ParseNode>>, decl: P<Declaration>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().declaration.set(Some(decl));
}

// analyzer/analyzerNodeInfo.ts:577
pub fn get_static_condition_value(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<bool> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.static_condition_value.get())
}

// analyzer/analyzerNodeInfo.ts:582
pub fn set_static_condition_value(
    node: impl Into<P<ParseNode>>,
    value: Option<bool>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().static_condition_value.set(value);
}

// analyzer/analyzerNodeInfo.ts:587
pub fn get_flow_node(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<FlowNode>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.flow_node.get())
}

// analyzer/analyzerNodeInfo.ts:592
pub fn set_flow_node(node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.flow_node.set(Some(flow_node));
}

// analyzer/analyzerNodeInfo.ts:597
pub fn get_after_flow_node(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<FlowNode>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.after_flow_node.get())
}

// analyzer/analyzerNodeInfo.ts:602
pub fn set_after_flow_node(node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().after_flow_node.set(Some(flow_node));
}

// analyzer/analyzerNodeInfo.ts:607
pub fn get_string_annotation(
    node: StringListNode,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<ExpressionNode> {
    // Precedence and tier asymmetry (do not reorder):
    //   Tier 1 - parser-derived quoted annotations (e.g. `x: "Data"`) are established by the
    //            grammar during parsing. They are owner-independent and live on the shared parse
    //            tree's owner key, so they are visible to every Program that reuses the tree.
    //   Tier 2 - semantically-discovered annotations (e.g. `cast("Data", v)`, alias-dependent
    //            forward refs) are owner-specific and stored per-`fileInfo` under the root node.
    // Parser annotations always win: an ordinary string is only consulted against the tier-2
    // store when no tier-1 annotation exists. Merging or reordering these lookups would break
    // cross-owner isolation (two Programs sharing a root could see each other's semantic results).
    let parser_annotation = get_parser_string_annotation(node);
    if parser_annotation.is_some() {
        return parser_annotation;
    }

    let root = get_parse_tree_root(&node)?;

    let file_info = reader.get_file_info(node.node())?;
    let root_info = reader.get(root.node())?;
    let cold = root_info.cold()?;
    let string_annotations = cold.string_annotations.borrow();
    string_annotations
        .as_ref()
        .and_then(|annotations| annotations.get(&file_info))
        .and_then(|owner_annotations| owner_annotations.get(&node).copied())
}

// analyzer/analyzerNodeInfo.ts:631
pub fn set_string_annotation(
    node: StringListNode,
    annotation: ExpressionNode,
    nested_annotations: &StringAnnotationInfo,
    reader: &dyn AnalyzerNodeInfoReader,
) {
    let root = get_parse_tree_root(&node);
    let file_info = reader.get_file_info(node.node());
    let root_info = root.and_then(|root| reader.get(root.node()));
    let (Some(_root), Some(file_info), Some(root_info)) = (root, file_info, root_info) else {
        panic!("String annotations require a bound parse tree");
    };

    let cold = root_info.cold_mut();
    let mut string_annotations = cold.string_annotations.borrow_mut();
    let owner_annotations = string_annotations
        .get_or_insert_with(FxHashMap::default)
        .entry(file_info)
        .or_default();

    owner_annotations.insert(node, annotation);
    nested_annotations.for_each(|nested_annotation, nested_node| {
        owner_annotations.insert(nested_node, nested_annotation);
    });
}

// analyzer/analyzerNodeInfo.ts:657
pub fn get_file_info_if_available(
    node: impl Into<P<ParseNode>>,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<P<AnalyzerFileInfo>> {
    let node = node.into();
    reader.get_file_info(node)
}

// analyzer/analyzerNodeInfo.ts:661
pub fn get_file_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> P<AnalyzerFileInfo> {
    let node = node.into();
    get_file_info_if_available(node, reader).unwrap()
}

// analyzer/analyzerNodeInfo.ts:665
pub fn set_file_info(node: ModuleNode, file_info: P<AnalyzerFileInfo>, writer: &dyn AnalyzerNodeInfoWriter) {
    writer.set_file_info(node, file_info);
}

// analyzer/analyzerNodeInfo.ts:669
pub fn get_code_flow_expressions(
    node: impl Into<P<ParseNode>>,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<P<RefCell<JsSet<String>>>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.code_flow_expressions.get())
}

// analyzer/analyzerNodeInfo.ts:677
pub fn set_code_flow_expressions(
    node: impl Into<P<ParseNode>>,
    expressions: P<RefCell<JsSet<String>>>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().code_flow_expressions.set(Some(expressions));
}

// analyzer/analyzerNodeInfo.ts:686
pub fn get_code_flow_complexity(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> f64 {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.code_flow_complexity.get())
        .unwrap_or(0.0)
}

// analyzer/analyzerNodeInfo.ts:691
pub fn set_code_flow_complexity(node: impl Into<P<ParseNode>>, complexity: f64, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().code_flow_complexity.set(Some(complexity));
}

// analyzer/analyzerNodeInfo.ts:696
pub fn get_dunder_all_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<DunderAllInfo>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.cold()).and_then(|cold| cold.dunder_all_info.get())
}

// analyzer/analyzerNodeInfo.ts:701
pub fn set_dunder_all_info(
    node: impl Into<P<ParseNode>>,
    names: Option<P<DunderAllInfo>>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.cold_mut().dunder_all_info.set(names);
}

// analyzer/analyzerNodeInfo.ts:706
pub fn is_code_unreachable(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> bool {
    let node = node.into();
    let mut cur_node = Some(node);

    // Walk up the parse tree until we find a node with
    // an associated flow node.
    while let Some(node) = cur_node {
        let flow_node = get_flow_node(node, reader);
        if let Some(flow_node) = flow_node {
            return flow_node.flags().any(FlowFlags::UnreachableStaticCondition | FlowFlags::UnreachableStructural);
        }
        cur_node = node.parent.get();
    }

    false
}

// Port of `tests/analyzerNodeInfo.test.ts`. The parser is not needed: every
// test uses only the module node, which `ModuleNode::create` builds directly.
#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use jsrt::{JsMap, JsSet, P};

    use super::*;
    use crate::analyzer::source_file::IPythonMode;
    use crate::common::config_options::{ExecutionEnvironment, get_standard_diagnostic_rule_set};
    use crate::common::diagnostic_sink::TextRangeDiagnosticSink;
    use crate::common::text_range::TextRange;
    use crate::common::text_range_collection::TextRangeCollection;
    use crate::common::uri::Uri;
    use crate::parser::parse_nodes::ModuleNode;

    fn parse_module(code: &str) -> ModuleNode {
        ModuleNode::create(&TextRange::create(0, code.encode_utf16().count() as i32))
    }

    /// The `{} as AnalyzerFileInfo` of the TypeScript tests: an object
    /// compared only by identity.
    fn file_info() -> P<AnalyzerFileInfo> {
        let lines: &'static TextRangeCollection<TextRange> = P::new(TextRangeCollection::new(Vec::new())).get();
        let rule_set = get_standard_diagnostic_rule_set();
        P::new(AnalyzerFileInfo {
            import_lookup: Rc::new(|_, _| None),
            future_imports: JsSet::default(),
            builtins_scope: None,
            diagnostic_sink: TextRangeDiagnosticSink::new(lines, None),
            execution_environment: P::new(ExecutionEnvironment::new(
                String::new(),
                Uri::empty(),
                &rule_set,
                None,
                None,
                None,
                false,
            )),
            diagnostic_rule_set: rule_set,
            lines,
            typing_symbol_aliases: JsMap::default(),
            defined_constants: JsMap::default(),
            file_id: String::new(),
            file_uri: Uri::empty(),
            module_name: Rc::default(),
            is_stub_file: false,
            is_typing_stub_file: false,
            is_typing_extensions_stub_file: false,
            is_typeshed_stub_file: false,
            is_built_in_stub_file: false,
            is_in_py_typed_package: false,
            ipython_mode: IPythonMode::None,
            accessed_symbol_set: Default::default(),
        })
    }

    fn create_store(root: ModuleNode, complexity: f64) -> P<AnalyzerNodeInfoStore> {
        let store = AnalyzerNodeInfoStore::new();
        store.get_or_create(root.node()).cold_mut().code_flow_complexity.set(Some(complexity));
        store
    }

    fn create_store_with_file_info(root: ModuleNode, file_info: P<AnalyzerFileInfo>) -> P<AnalyzerNodeInfoStore> {
        let store = AnalyzerNodeInfoStore::new();
        store.set_file_info(root, file_info);
        store
    }

    fn complexity(info: Option<P<AnalyzerNodeInfo>>) -> Option<f64> {
        info.and_then(|info| info.cold()).and_then(|cold| cold.code_flow_complexity.get())
    }

    #[test]
    fn store_keeps_node_information_independent_across_parse_trees() {
        let first_root = parse_module("first = 1");
        let second_root = parse_module("second = 2");
        let store = AnalyzerNodeInfoStore::new();

        store.get_or_create(first_root.node()).cold_mut().code_flow_complexity.set(Some(1.0));
        store.get_or_create(second_root.node()).cold_mut().code_flow_complexity.set(Some(2.0));

        assert_eq!(complexity((*store).get(first_root.node())), Some(1.0));
        assert_eq!(complexity((*store).get(second_root.node())), Some(2.0));
    }

    #[test]
    fn context_preserves_layered_lookup_behavior_across_overlay_enter_discard() {
        let root = parse_module("value = 1");
        let context = AnalyzerNodeInfoContextImpl::new();
        let base_store = create_store(root, 1.0);
        let overlay_store = create_store(root, 2.0);

        context.register_store(root, base_store);
        context.enter_overlay();
        assert_eq!(complexity((*context).get(root.node())), Some(1.0));

        context.register_store(root, overlay_store);
        assert_eq!(complexity((*context).get(root.node())), Some(2.0));

        context.discard_overlay();
        assert_eq!(complexity((*context).get(root.node())), Some(1.0));

        context.enter_overlay();
        context.remove(root);
        assert!((*context).get(root.node()).is_none());
        context.discard_overlay();
        assert_eq!(complexity((*context).get(root.node())), Some(1.0));
    }

    #[test]
    fn context_fast_path_returns_undefined_after_base_layer_remove_and_recovers_on_reregister() {
        let root = parse_module("value = 1");
        let context = AnalyzerNodeInfoContextImpl::new();

        // Only the base layer exists here, so these operations exercise the single-layer fast path.
        context.register_store(root, create_store(root, 1.0));
        assert_eq!(complexity((*context).get(root.node())), Some(1.0));

        context.remove(root);
        assert!((*context).get(root.node()).is_none());

        context.register_store(root, create_store(root, 3.0));
        assert_eq!(complexity((*context).get(root.node())), Some(3.0));
    }

    #[test]
    fn context_get_file_info_fast_path_handles_base_layer_register_remove_reregister() {
        let root = parse_module("value = 1");
        let context = AnalyzerNodeInfoContextImpl::new();

        // Only the base layer exists here, so these operations exercise the single-layer
        // fast path in getFileInfo (the removedKeys tombstone check is skipped).
        let first_file_info = file_info();
        context.register_store(root, create_store_with_file_info(root, first_file_info));
        assert_eq!(context.get_file_info(root.node()), Some(first_file_info));

        context.remove(root);
        assert_eq!(context.get_file_info(root.node()), None);

        let second_file_info = file_info();
        context.register_store(root, create_store_with_file_info(root, second_file_info));
        assert_eq!(context.get_file_info(root.node()), Some(second_file_info));
    }

    #[test]
    fn context_retains_stores_with_reachable_parse_trees() {
        let first_root = parse_module("first = 1");
        let second_root = parse_module("second = 2");
        let context = AnalyzerNodeInfoContextImpl::new();
        let first_file_info = file_info();
        let second_file_info = file_info();

        let first_store = create_store_with_file_info(first_root, first_file_info);
        first_store
            .get_or_create(first_root.node())
            .cold_mut()
            .code_flow_complexity
            .set(Some(1.0));
        context.register_store(first_root, first_store);
        context.retain_removed_store(first_root);
        assert_eq!(context.get_file_info(first_root.node()), Some(first_file_info));
        assert_eq!(complexity((*context).get(first_root.node())), Some(1.0));

        context.register_store(second_root, create_store_with_file_info(second_root, second_file_info));
        context.retain_removed_store(second_root);
        assert_eq!(context.get_file_info(first_root.node()), Some(first_file_info));
        assert_eq!(context.get_file_info(second_root.node()), Some(second_file_info));

        context.remove(second_root);
        assert_eq!(context.get_file_info(second_root.node()), None);
    }

    #[test]
    fn retired_stores_remain_isolated_between_contexts_sharing_a_parse_tree() {
        let root = parse_module("value = 1");
        let first_context = AnalyzerNodeInfoContextImpl::new();
        let second_context = AnalyzerNodeInfoContextImpl::new();
        let first_file_info = file_info();
        let second_file_info = file_info();

        first_context.register_store(root, create_store_with_file_info(root, first_file_info));
        second_context.register_store(root, create_store_with_file_info(root, second_file_info));
        first_context.retain_removed_store(root);
        second_context.retain_removed_store(root);

        assert_eq!(first_context.get_file_info(root.node()), Some(first_file_info));
        assert_eq!(second_context.get_file_info(root.node()), Some(second_file_info));
    }
}
