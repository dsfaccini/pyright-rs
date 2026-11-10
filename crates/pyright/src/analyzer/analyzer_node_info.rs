//! Port of `packages/pyright-internal/src/analyzer/analyzerNodeInfo.ts`.
//!
//! The `WeakMap`s keyed by parse tree keys and nodes become lookup-only hash
//! maps; nothing iterates them. Readers and writers are `&'static dyn` trait
//! objects: every store, session and context lives in the arena.

// `P` hashes by address, so its interior mutability never changes a key.
#![allow(clippy::mutable_key_type)]

use std::cell::{Cell, RefCell};

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
pub struct DunderAllInfo {
    pub names: Vec<String>,
    pub string_nodes: Vec<StringNode>,
    pub uses_unsupported_dunder_all_form: bool,
}

/// `WeakMap<StringListNode, ExpressionNode>`: one owner's discovered annotations.
pub type OwnerStringAnnotations = FxHashMap<StringListNode, ExpressionNode>;

// analyzer/analyzerNodeInfo.ts:40
#[derive(Default)]
pub struct AnalyzerNodeInfo {
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
    pub declaration: Cell<Option<P<Declaration>>>,

    // Control flow information for this node.
    pub flow_node: Cell<Option<P<FlowNode>>>,

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

// analyzer/analyzerNodeInfo.ts:82
pub trait AnalyzerNodeInfoReader {
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>>;
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>>;
}

// analyzer/analyzerNodeInfo.ts:107
pub trait AnalyzerNodeInfoWriter: AnalyzerNodeInfoReader {
    fn get_or_create(&self, node: P<ParseNode>) -> P<AnalyzerNodeInfo>;
    fn set_file_info(&self, root: ModuleNode, file_info: P<AnalyzerFileInfo>);
}

// analyzer/analyzerNodeInfo.ts:112
#[derive(Default)]
pub struct AnalyzerNodeInfoStore {
    _info_by_tree: RefCell<FxHashMap<ParseTreeKey, FxHashMap<P<ParseNode>, P<AnalyzerNodeInfo>>>>,
    _file_info_by_tree: RefCell<FxHashMap<ParseTreeKey, P<AnalyzerFileInfo>>>,
}

impl AnalyzerNodeInfoStore {
    pub fn new() -> P<AnalyzerNodeInfoStore> {
        P::new(AnalyzerNodeInfoStore::default())
    }
}

impl AnalyzerNodeInfoReader for AnalyzerNodeInfoStore {
    // analyzer/analyzerNodeInfo.ts:116
    fn get(&self, node: P<ParseNode>) -> Option<P<AnalyzerNodeInfo>> {
        self._info_by_tree
            .borrow()
            .get(&node.a)
            .and_then(|tree_info| tree_info.get(&node).copied())
    }

    // analyzer/analyzerNodeInfo.ts:120
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        self._file_info_by_tree.borrow().get(&node.a).copied()
    }
}

impl AnalyzerNodeInfoWriter for AnalyzerNodeInfoStore {
    // analyzer/analyzerNodeInfo.ts:124
    fn get_or_create(&self, node: P<ParseNode>) -> P<AnalyzerNodeInfo> {
        let mut info_by_tree = self._info_by_tree.borrow_mut();
        let tree_info = info_by_tree.entry(node.a).or_default();

        *tree_info
            .entry(node)
            .or_insert_with(|| P::new(AnalyzerNodeInfo::default()))
    }

    // analyzer/analyzerNodeInfo.ts:140
    fn set_file_info(&self, root: ModuleNode, file_info: P<AnalyzerFileInfo>) {
        self._file_info_by_tree.borrow_mut().insert(root.a, file_info);
    }
}

// analyzer/analyzerNodeInfo.ts:145
pub trait AnalyzerNodeInfoBindingSession: AnalyzerNodeInfoWriter {
    fn root(&self) -> ModuleNode;
}

// analyzer/analyzerNodeInfo.ts:149
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

// analyzer/analyzerNodeInfo.ts:163
#[derive(Default)]
struct AnalyzerNodeInfoContextLayer {
    stores: FxHashMap<ParseTreeKey, P<AnalyzerNodeInfoStore>>,
    removed_keys: FxHashSet<ParseTreeKey>,
}

// analyzer/analyzerNodeInfo.ts:174
pub struct AnalyzerNodeInfoBindingSessionImpl {
    _store: P<AnalyzerNodeInfoStore>,
    _published: Cell<bool>,
    pub root: ModuleNode,
    _context: P<AnalyzerNodeInfoContextImpl>,
}

impl AnalyzerNodeInfoBindingSessionImpl {
    // analyzer/analyzerNodeInfo.ts:178
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

// analyzer/analyzerNodeInfo.ts:229
/// `_retiredStores`, the symbol-keyed property TypeScript hangs on a tree
/// key, is the `_retired_stores` map: tree key to this context's store.
pub struct AnalyzerNodeInfoContextImpl {
    _layers: RefCell<Vec<AnalyzerNodeInfoContextLayer>>,
    _disposed: Cell<bool>,
    _current_layer_reader: Cell<Option<&'static CurrentLayerReader>>,
    _retired_stores: RefCell<FxHashMap<ParseTreeKey, P<AnalyzerNodeInfoStore>>>,
    _self: Cell<Option<P<AnalyzerNodeInfoContextImpl>>>,
}

/// The `_currentLayerReader` object literal.
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

    fn _store_for(&self, key: ParseTreeKey) -> Option<P<AnalyzerNodeInfoStore>> {
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

    // analyzer/analyzerNodeInfo.ts:362
    fn _create_layer() -> AnalyzerNodeInfoContextLayer {
        AnalyzerNodeInfoContextLayer::default()
    }

    // analyzer/analyzerNodeInfo.ts:369
    fn _get_retired_store(&self, key: ParseTreeKey) -> Option<P<AnalyzerNodeInfoStore>> {
        self._retired_stores.borrow().get(&key).copied()
    }

    // analyzer/analyzerNodeInfo.ts:373
    fn _delete_retired_store(&self, key: ParseTreeKey) {
        self._retired_stores.borrow_mut().remove(&key);
    }

    // analyzer/analyzerNodeInfo.ts:377
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

    // analyzer/analyzerNodeInfo.ts:390
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

    // analyzer/analyzerNodeInfo.ts:403
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

    // analyzer/analyzerNodeInfo.ts:265
    fn get_file_info(&self, node: P<ParseNode>) -> Option<P<AnalyzerFileInfo>> {
        if self._disposed.get() {
            return None;
        }

        self._store_for(node.a)
            .and_then(|store| store.get_file_info(node))
    }
}

impl AnalyzerNodeInfoContext for AnalyzerNodeInfoContextImpl {
    // analyzer/analyzerNodeInfo.ts:289
    fn get_current_layer_reader(&self) -> &'static dyn AnalyzerNodeInfoReader {
        self._current_layer_reader.get().unwrap()
    }

    // analyzer/analyzerNodeInfo.ts:293
    fn begin_write(&self, root: ModuleNode) -> P<AnalyzerNodeInfoBindingSessionImpl> {
        self._throw_if_disposed();
        AnalyzerNodeInfoBindingSessionImpl::new(root, self.handle())
    }

    // analyzer/analyzerNodeInfo.ts:298
    fn publish(&self, session: P<AnalyzerNodeInfoBindingSessionImpl>) -> P<AnalyzerNodeInfoStore> {
        self._throw_if_disposed();

        let store = session.publish(self.handle());
        self.register_store(session.root, store);
        store
    }

    // analyzer/analyzerNodeInfo.ts:309
    fn register_store(&self, root: ModuleNode, store: P<AnalyzerNodeInfoStore>) {
        self._throw_if_disposed();
        {
            let mut layers = self._layers.borrow_mut();
            let layer = layers.last_mut().unwrap();
            layer.removed_keys.remove(&root.a);
            layer.stores.insert(root.a, store);
        }
        self._delete_retired_store(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:317
    fn remove(&self, root: ModuleNode) {
        self._throw_if_disposed();
        {
            let mut layers = self._layers.borrow_mut();
            let layer = layers.last_mut().unwrap();
            layer.stores.remove(&root.a);
            layer.removed_keys.insert(root.a);
        }
        self._delete_retired_store(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:325
    fn retain_removed_store(&self, root: ModuleNode) {
        self._throw_if_disposed();
        let mut layers = self._layers.borrow_mut();
        let layer = layers.last_mut().unwrap();
        if let Some(store) = layer.stores.get(&root.a).copied() {
            self._retired_stores.borrow_mut().insert(root.a, store);
        }
        layer.stores.remove(&root.a);
        layer.removed_keys.insert(root.a);
    }

    // analyzer/analyzerNodeInfo.ts:338
    fn enter_overlay(&self) {
        self._throw_if_disposed();
        self._layers
            .borrow_mut()
            .push(AnalyzerNodeInfoContextImpl::_create_layer());
    }

    // analyzer/analyzerNodeInfo.ts:343
    fn promote_to_previous_layer(&self, root: ModuleNode) {
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

    // analyzer/analyzerNodeInfo.ts:350
    fn discard_overlay(&self) {
        self._throw_if_disposed();
        let mut layers = self._layers.borrow_mut();
        if layers.len() == 1 {
            panic!("Cannot discard the base analyzer information context layer");
        }

        layers.pop();
    }

    // analyzer/analyzerNodeInfo.ts:357
    fn dispose(&self) {
        self._layers.borrow_mut().clear();
        self._disposed.set(true);
    }
}

// analyzer/analyzerNodeInfo.ts:410
/// `ModuleNode | ClassNode | FunctionNode | LambdaNode | ComprehensionNode`.
pub type ScopedNode = P<ParseNode>;

// analyzer/analyzerNodeInfo.ts:412
#[derive(Clone, Copy)]
pub struct AnalyzerNodeInfoAccessor {
    _reader: &'static dyn AnalyzerNodeInfoReader,
    _writer: Option<&'static dyn AnalyzerNodeInfoWriter>,
}

impl AnalyzerNodeInfoAccessor {
    // analyzer/analyzerNodeInfo.ts:413
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

    // analyzer/analyzerNodeInfo.ts:419
    pub fn get_import_info(&self, node: impl Into<P<ParseNode>>) -> Option<P<ImportResult>> {
        get_import_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:423
    pub fn set_import_info(&self, node: impl Into<P<ParseNode>>, import_info: P<ImportResult>) {
        set_import_info(node, import_info, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:427
    pub fn get_scope(&self, node: impl Into<P<ParseNode>>) -> Option<P<Scope>> {
        get_scope(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:431
    pub fn set_scope(&self, node: impl Into<P<ParseNode>>, scope: P<Scope>) {
        set_scope(node, scope, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:435
    pub fn get_declaration(&self, node: impl Into<P<ParseNode>>) -> Option<P<Declaration>> {
        get_declaration(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:439
    pub fn set_declaration(&self, node: impl Into<P<ParseNode>>, declaration: P<Declaration>) {
        set_declaration(node, declaration, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:443
    pub fn get_static_condition_value(&self, node: impl Into<P<ParseNode>>) -> Option<bool> {
        get_static_condition_value(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:447
    pub fn set_static_condition_value(&self, node: impl Into<P<ParseNode>>, value: Option<bool>) {
        set_static_condition_value(node, value, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:451
    pub fn get_flow_node(&self, node: impl Into<P<ParseNode>>) -> Option<P<FlowNode>> {
        get_flow_node(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:455
    pub fn set_flow_node(&self, node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>) {
        set_flow_node(node, flow_node, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:459
    pub fn get_after_flow_node(&self, node: impl Into<P<ParseNode>>) -> Option<P<FlowNode>> {
        get_after_flow_node(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:463
    pub fn set_after_flow_node(&self, node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>) {
        set_after_flow_node(node, flow_node, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:467
    pub fn get_file_info_if_available(&self, node: impl Into<P<ParseNode>>) -> Option<P<AnalyzerFileInfo>> {
        get_file_info_if_available(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:471
    pub fn get_file_info(&self, node: P<ParseNode>) -> P<AnalyzerFileInfo> {
        get_file_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:475
    pub fn set_file_info(&self, node: ModuleNode, file_info: P<AnalyzerFileInfo>) {
        set_file_info(node, file_info, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:479
    pub fn get_code_flow_expressions(&self, node: impl Into<P<ParseNode>>) -> Option<P<RefCell<JsSet<String>>>> {
        get_code_flow_expressions(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:483
    pub fn set_code_flow_expressions(&self, node: impl Into<P<ParseNode>>, expressions: P<RefCell<JsSet<String>>>) {
        set_code_flow_expressions(node, expressions, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:487
    pub fn get_code_flow_complexity(&self, node: impl Into<P<ParseNode>>) -> f64 {
        get_code_flow_complexity(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:491
    pub fn set_code_flow_complexity(&self, node: impl Into<P<ParseNode>>, complexity: f64) {
        set_code_flow_complexity(node, complexity, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:495
    pub fn get_dunder_all_info(&self, node: impl Into<P<ParseNode>>) -> Option<P<DunderAllInfo>> {
        get_dunder_all_info(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:499
    pub fn set_dunder_all_info(&self, node: impl Into<P<ParseNode>>, names: Option<P<DunderAllInfo>>) {
        set_dunder_all_info(node, names, self._get_writer());
    }

    // analyzer/analyzerNodeInfo.ts:503
    pub fn get_string_annotation(&self, node: StringListNode) -> Option<ExpressionNode> {
        get_string_annotation(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:507
    pub fn set_string_annotation(
        &self,
        node: StringListNode,
        annotation: ExpressionNode,
        nested_annotations: &StringAnnotationInfo,
    ) {
        set_string_annotation(node, annotation, nested_annotations, self._reader);
    }

    // analyzer/analyzerNodeInfo.ts:511
    pub fn is_code_unreachable(&self, node: impl Into<P<ParseNode>>) -> bool {
        is_code_unreachable(node, self._reader)
    }

    // analyzer/analyzerNodeInfo.ts:515
    fn _get_writer(&self) -> &'static dyn AnalyzerNodeInfoWriter {
        match self._writer {
            Some(writer) => writer,
            None => panic!("Analyzer node information accessor is read-only"),
        }
    }
}

// analyzer/analyzerNodeInfo.ts:528
pub fn create_analyzer_node_info_accessor(
    reader: &'static dyn AnalyzerNodeInfoReader,
    writer: Option<&'static dyn AnalyzerNodeInfoWriter>,
) -> AnalyzerNodeInfoAccessor {
    AnalyzerNodeInfoAccessor::new(reader, writer)
}

// analyzer/analyzerNodeInfo.ts:532
pub fn get_import_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<ImportResult>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.import_info.get())
}

// analyzer/analyzerNodeInfo.ts:537
pub fn set_import_info(node: impl Into<P<ParseNode>>, import_info: P<ImportResult>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.import_info.set(Some(import_info));
}

// analyzer/analyzerNodeInfo.ts:542
pub fn get_scope(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<Scope>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.scope.get())
}

// analyzer/analyzerNodeInfo.ts:547
pub fn set_scope(node: impl Into<P<ParseNode>>, scope: P<Scope>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.scope.set(Some(scope));
}

// analyzer/analyzerNodeInfo.ts:552
pub fn get_declaration(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<Declaration>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.declaration.get())
}

// analyzer/analyzerNodeInfo.ts:557
pub fn set_declaration(node: impl Into<P<ParseNode>>, decl: P<Declaration>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.declaration.set(Some(decl));
}

// analyzer/analyzerNodeInfo.ts:562
pub fn get_static_condition_value(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<bool> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.static_condition_value.get())
}

// analyzer/analyzerNodeInfo.ts:567
pub fn set_static_condition_value(
    node: impl Into<P<ParseNode>>,
    value: Option<bool>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.static_condition_value.set(value);
}

// analyzer/analyzerNodeInfo.ts:572
pub fn get_flow_node(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<FlowNode>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.flow_node.get())
}

// analyzer/analyzerNodeInfo.ts:577
pub fn set_flow_node(node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.flow_node.set(Some(flow_node));
}

// analyzer/analyzerNodeInfo.ts:582
pub fn get_after_flow_node(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<FlowNode>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.after_flow_node.get())
}

// analyzer/analyzerNodeInfo.ts:587
pub fn set_after_flow_node(node: impl Into<P<ParseNode>>, flow_node: P<FlowNode>, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.after_flow_node.set(Some(flow_node));
}

// analyzer/analyzerNodeInfo.ts:592
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
    let string_annotations = root_info.string_annotations.borrow();
    string_annotations
        .as_ref()
        .and_then(|annotations| annotations.get(&file_info))
        .and_then(|owner_annotations| owner_annotations.get(&node).copied())
}

// analyzer/analyzerNodeInfo.ts:617
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

    let mut string_annotations = root_info.string_annotations.borrow_mut();
    let owner_annotations = string_annotations
        .get_or_insert_with(FxHashMap::default)
        .entry(file_info)
        .or_default();

    owner_annotations.insert(node, annotation);
    nested_annotations.for_each(|nested_annotation, nested_node| {
        owner_annotations.insert(nested_node, nested_annotation);
    });
}

// analyzer/analyzerNodeInfo.ts:641
pub fn get_file_info_if_available(
    node: impl Into<P<ParseNode>>,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<P<AnalyzerFileInfo>> {
    let node = node.into();
    reader.get_file_info(node)
}

// analyzer/analyzerNodeInfo.ts:645
pub fn get_file_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> P<AnalyzerFileInfo> {
    let node = node.into();
    get_file_info_if_available(node, reader).unwrap()
}

// analyzer/analyzerNodeInfo.ts:649
pub fn set_file_info(node: ModuleNode, file_info: P<AnalyzerFileInfo>, writer: &dyn AnalyzerNodeInfoWriter) {
    writer.set_file_info(node, file_info);
}

// analyzer/analyzerNodeInfo.ts:653
pub fn get_code_flow_expressions(
    node: impl Into<P<ParseNode>>,
    reader: &dyn AnalyzerNodeInfoReader,
) -> Option<P<RefCell<JsSet<String>>>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.code_flow_expressions.get())
}

// analyzer/analyzerNodeInfo.ts:661
pub fn set_code_flow_expressions(
    node: impl Into<P<ParseNode>>,
    expressions: P<RefCell<JsSet<String>>>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.code_flow_expressions.set(Some(expressions));
}

// analyzer/analyzerNodeInfo.ts:670
pub fn get_code_flow_complexity(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> f64 {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.code_flow_complexity.get())
        .unwrap_or(0.0)
}

// analyzer/analyzerNodeInfo.ts:675
pub fn set_code_flow_complexity(node: impl Into<P<ParseNode>>, complexity: f64, writer: &dyn AnalyzerNodeInfoWriter) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.code_flow_complexity.set(Some(complexity));
}

// analyzer/analyzerNodeInfo.ts:680
pub fn get_dunder_all_info(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> Option<P<DunderAllInfo>> {
    let node = node.into();
    let info = reader.get(node);
    info.and_then(|info| info.dunder_all_info.get())
}

// analyzer/analyzerNodeInfo.ts:685
pub fn set_dunder_all_info(
    node: impl Into<P<ParseNode>>,
    names: Option<P<DunderAllInfo>>,
    writer: &dyn AnalyzerNodeInfoWriter,
) {
    let node = node.into();
    let info = writer.get_or_create(node);
    info.dunder_all_info.set(names);
}

// analyzer/analyzerNodeInfo.ts:690
pub fn is_code_unreachable(node: impl Into<P<ParseNode>>, reader: &dyn AnalyzerNodeInfoReader) -> bool {
    let node = node.into();
    let mut cur_node = Some(node);

    // Walk up the parse tree until we find a node with
    // an associated flow node.
    while let Some(node) = cur_node {
        let flow_node = get_flow_node(node, reader);
        if let Some(flow_node) = flow_node {
            return flow_node
                .flags
                .get()
                .any(FlowFlags::UnreachableStaticCondition | FlowFlags::UnreachableStructural);
        }
        cur_node = node.parent.get();
    }

    false
}
