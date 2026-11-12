//! Port of `packages/pyright-internal/src/analyzer/binder.ts`.
//!
//! `Binder` and `YieldFinder`/`ReturnFinder` implement the `ParseTreeWalker`
//! trait. The TypeScript callbacks that run with `this` bound (scope bodies,
//! deferred bindings, tracked flow expressions) are `FnOnce(&mut Binder)`.

use jsrt::cell::{Cell, RefCell};

use jsrt::{Atom, JsMap, JsSet, OptionRefCell, P};

use super::analyzer_file_info::{AnalyzerFileInfo, ImportLookupKey};
use super::analyzer_node_info::{AnalyzerNodeInfoAccessor, DunderAllInfo};
use super::cell_chain_index::CellChainIndexProvider;
use super::code_flow_types::{
    CodeFlowReferenceExpressionNode, FlowAssignmentDetails, FlowBranchLabel,
    FlowBranchLabelDetails, FlowCallDetails, FlowConditionDetails, FlowExhaustedMatchDetails,
    FlowFlags, FlowLabel, FlowLabelDetails, FlowNarrowForPatternDetails, FlowNode,
    FlowPlain, FlowPostContextManagerLabel, FlowPostContextManagerLabelDetails,
    FlowPostFinallyDetails, FlowPreFinallyGate, FlowPreFinallyGateDetails,
    FlowVariableAnnotationDetails, FlowWildcardImportDetails, wildcard_import_reference_key,
    create_key_for_reference, get_unique_flow_node_id,
    is_code_flow_supported_for_reference,
};
use super::declaration::{
    NoDeclarationDetails,
    AliasDeclarationDetails, Declaration, DeclarationType,
    FunctionDeclarationDetails, IntrinsicDeclarationDetails, IntrinsicType,
    ModuleLoaderActions, ModuleLoaderActionsLike, ParamDeclarationDetails,
    TypeAliasDeclarationDetails, VariableDeclarationDetails, create_implicit_import_map,
    unresolved_module_marker,
};
use super::import_result::{ImplicitImport, ImportResult, ImportType};
use super::import_statement_utils::get_wildcard_import_names;
use super::parse_tree_utils as ParseTreeUtils;
use super::parse_tree_walker::{ParseTreeVisitor, ParseTreeWalker};
use super::scope::{
    ChainedModuleLevelLookupContext, NameBindingType, Scope, ScopeChainedModuleLevelLookup,
    ScopeType, SymbolWithScope,
};
use super::source_file::IPythonMode;
use super::static_expressions as StaticExpressions;
use super::symbol::{INDETERMINATE_SYMBOL_ID, Symbol, SymbolFlags};
use super::symbol_name_utils::{is_constant_name, is_private_name, is_private_or_protected_name};
use crate::common::config_options::DiagnosticLevel;
use crate::common::debug::{assert, fail};
use crate::common::diagnostic::{Diagnostic, DiagnosticAction};
use crate::common::diagnostic_rules::DiagnosticRule;
use crate::common::path_utils::strip_file_extension;
use crate::common::position_utils::convert_text_range_to_range;
use crate::common::text_range::{HasTextRange, Range, TextRange, get_empty_range};
use crate::common::uri::Uri;
use crate::localization::localize::LocMessage;
use crate::parser::parse_nodes::{
    ArgCategory, AssertNode, AssignmentExpressionNode, AssignmentNode, AugmentedAssignmentNode,
    AwaitNode, BinaryOperationNode, BreakNode, CallNode, ClassNode, ComprehensionNode,
    ContinueNode, DelNode, ExceptNode, ExpressionNode, ForNode, FunctionNode, GlobalNode, IfNode,
    ImportAsNode, ImportFromNode, IndexNode, LambdaNode, ListNode, MatchNode,
    MemberAccessNode, ModuleNameNode, ModuleNode, NameNode, NonlocalNode, ParseNode,
    ParseNodeDetails as D, ParseNodeType, PatternAsNode, PatternCaptureNode,
    PatternMappingExpandEntryNode, RaiseNode, ReturnNode, StatementNode, StringListNode,
    StringNode, SuiteNode, TernaryNode, TryNode, TypeAliasNode, TypeAnnotationNode,
    TypeParameterListNode, UnaryOperationNode, WhileNode, WithNode, YieldFromNode, YieldNode,
};
use crate::parser::tokenizer_types::{KeywordType, OperatorType};

// The binder reads only `class_scope` and `is_instance_member`.
// analyzer/binder.ts:138
#[allow(dead_code)]
struct MemberAccessInfo {
    class_node: ClassNode,
    method_node: FunctionNode,
    class_scope: P<Scope>,
    is_instance_member: bool,
}

// analyzer/binder.ts:145
struct DeferredBindingTask {
    scope: P<Scope>,
    code_flow_expressions: P<RefCell<JsSet<Atom>>>,
    callback: Box<dyn FnOnce(&mut Binder)>,
}

// analyzer/binder.ts:151
struct FinalInfo {
    is_final: bool,
    final_type_node: Option<ExpressionNode>,
}

// analyzer/binder.ts:156
struct ClassVarInfo {
    is_class_var: bool,
    class_var_type_node: Option<ExpressionNode>,
}

// analyzer/binder.ts:161
#[derive(Clone, Copy, Default)]
struct NarrowExprOptions {
    filter_for_never_narrowing: Option<bool>,
    is_complex_expression: Option<bool>,
    allow_discriminated_narrowing: Option<bool>,
}

/// A new, empty shared `Set<string>`.
fn new_string_set() -> P<RefCell<JsSet<Atom>>> {
    P::new(RefCell::new(JsSet::default()))
}

/// The `value` of a `StringNode`, as the `string` TypeScript reads.
fn string_value(node: StringNode) -> String {
    node.d().value.to_string_lossy()
}

/// `stringList.d.strings[0].d.value`: a `StringNode` or `FormatStringNode`.
fn first_string_value(node: StringListNode) -> String {
    let first = node.d().strings.borrow()[0];
    match first.d() {
        D::String(d) => d.value.to_string_lossy(),
        D::FormatString(d) => d.value.to_string_lossy(),
        _ => unreachable!(),
    }
}

/// `node.nodeType === StringList && node.d.strings.length === 1 &&
/// node.d.strings[0].nodeType === String`: the one string node.
fn single_string_node(node: P<ParseNode>) -> Option<StringNode> {
    let D::StringList(d) = node.d() else {
        return None;
    };
    let strings = d.strings.borrow();
    if strings.len() == 1 {
        StringNode::cast(strings[0])
    } else {
        None
    }
}

/// `node.d.value` of a `NameNode` held as a `P<ParseNode>`.
fn name_value(node: P<ParseNode>) -> Option<&'static str> {
    match node.d() {
        D::Name(d) => Some(d.value()),
        _ => None,
    }
}

/// `(expr is Name && expr.value === name) || (expr is TypeAnnotation &&
/// expr.valueExpr is Name && expr.valueExpr.value === name)`.
fn is_name_or_annotated_name(expr: ExpressionNode, name: &str) -> bool {
    match expr.d() {
        D::Name(d) => d.value() == name,
        D::TypeAnnotation(d) => name_value(d.value_expr.get()) == Some(name),
        _ => false,
    }
}

// analyzer/binder.ts:167
fn is_static_class_assignment_target(target: ExpressionNode) -> bool {
    match target.d() {
        D::Name(_) => true,

        D::TypeAnnotation(d) => is_static_class_assignment_target(d.value_expr.get()),

        D::Tuple(d) => d
            .items
            .borrow()
            .iter()
            .all(|item| is_static_class_assignment_target(*item)),
        D::List(d) => d
            .items
            .borrow()
            .iter()
            .all(|item| is_static_class_assignment_target(*item)),

        D::Unpack(d) => is_static_class_assignment_target(d.expr.get()),

        _ => false,
    }
}

// analyzer/binder.ts:187
fn get_called_built_in_name(
    scope: P<Scope>,
    expression: ExpressionNode,
    visited_symbols: &mut Vec<P<Symbol>>,
) -> Option<String> {
    if let D::Name(name) = expression.d() {
        let symbol_with_scope = scope.look_up_symbol_recursive(name.value(), None)?;
        if visited_symbols.contains(&symbol_with_scope.symbol) {
            return None;
        }
        visited_symbols.push(symbol_with_scope.symbol);

        if symbol_with_scope.scope.type_ == ScopeType::Builtin {
            return Some(name.value().to_string());
        }

        let declarations = symbol_with_scope.symbol.get_declarations();
        let declaration = declarations.last().copied();
        if let Some(declaration) = declaration {
            if let Some(var) = declaration.d().variable() {
                if let Some(source) = var.inferred_type_source.get() {
                    if source.node_type == ParseNodeType::Name
                        || source.node_type == ParseNodeType::MemberAccess
                    {
                        return get_called_built_in_name(scope, source, visited_symbols);
                    }
                }
            }

            if let Some(alias) = declaration.d().alias() {
                if declaration.module_name().as_str() == "builtins" {
                    if let Some(symbol_name) = alias.symbol_name.borrow().as_ref() {
                        if !symbol_name.is_empty() {
                            return Some(symbol_name.clone());
                        }
                    }
                }
            }
        }
    } else if let D::MemberAccess(member_access) = expression.d() {
        if let Some(left_name) = name_value(member_access.left_expr.get()) {
            let symbol_with_scope = scope.look_up_symbol_recursive(left_name, None);
            let declarations = symbol_with_scope
                .map(|s| s.symbol.get_declarations())
                .unwrap_or_default();
            let declaration = declarations.last().copied();
            if let Some(declaration) = declaration {
                if let Some(alias) = declaration.d().alias() {
                    if declaration.module_name().as_str() == "builtins"
                        && alias.symbol_name.borrow().as_ref().is_none_or(|s| s.is_empty())
                    {
                        return Some(member_access.member.get().d().value().to_string());
                    }
                }
            }
        }
    }

    None
}

// analyzer/binder.ts:239
fn does_call_expose_class_namespace(scope: P<Scope>, node: CallNode) -> bool {
    let built_in_name = get_called_built_in_name(scope, node.d().left_expr.get(), &mut Vec::new());
    let arg_count = node.d().args.borrow().len();
    built_in_name.as_deref() == Some("exec")
        || (built_in_name.as_deref() == Some("eval") && arg_count == 1)
        || ((built_in_name.as_deref() == Some("locals") || built_in_name.as_deref() == Some("vars"))
            && arg_count == 0)
}

// analyzer/binder.ts:252
// For each flow node within an execution context, we'll add a small
// amount to the complexity factor. Without this, the complexity
// calculation fails to take into account large numbers of non-cyclical
// flow nodes. This number is somewhat arbitrary and is tuned empirically.
const FLOW_NODE_COMPLEXITY_CONTRIBUTION: f64 = 0.025;

thread_local! {
    // analyzer/binder.ts:336
    // Flow node that is used for unreachable code.
    static UNREACHABLE_STATIC_CONDITION_FLOW_NODE: P<FlowNode> = FlowNode::create(
        FlowFlags::UnreachableStaticCondition,
        get_unique_flow_node_id(),
        FlowPlain,
    );

    // analyzer/binder.ts:341
    static UNREACHABLE_STRUCTURAL_FLOW_NODE: P<FlowNode> = FlowNode::create(
        FlowFlags::UnreachableStructural,
        get_unique_flow_node_id(),
        FlowPlain,
    );
}

/// `Binder._unreachableStaticConditionFlowNode`.
fn unreachable_static_condition_flow_node() -> P<FlowNode> {
    UNREACHABLE_STATIC_CONDITION_FLOW_NODE.with(|node| *node)
}

/// `Binder._unreachableStructuralFlowNode`.
fn unreachable_structural_flow_node() -> P<FlowNode> {
    UNREACHABLE_STRUCTURAL_FLOW_NODE.with(|node| *node)
}

// analyzer/binder.ts:1484
pub struct Binder {
    _file_info: P<AnalyzerFileInfo>,
    _node_info: AnalyzerNodeInfoAccessor,

    // A queue of deferred analysis operations.
    _deferred_binding_tasks: Vec<DeferredBindingTask>,

    // The current scope in effect.
    _current_scope: Option<P<Scope>>,

    // Current control-flow node.
    _current_flow_node: Option<P<FlowNode>>,

    // Current target function declaration, if currently binding
    // a function. This allows return and yield statements to be
    // added to the function declaration.
    _target_function_declaration: Option<P<Declaration>>,

    // Flow node label that is the target of a "break" statement.
    _current_break_target: Option<FlowLabel>,

    // Flow node label that is the target of a "continue" statement.
    _current_continue_target: Option<FlowLabel>,

    // Flow nodes used for if/else and while/else statements.
    _current_true_target: Option<FlowLabel>,
    _current_false_target: Option<FlowLabel>,

    // Flow nodes used within try blocks.
    _current_except_targets: Vec<FlowLabel>,

    // Flow nodes used within try/finally flows.
    _finally_targets: Vec<FlowLabel>,

    // Flow nodes used for return statements.
    _current_return_target: Option<FlowLabel>,

    // Set of expressions within the current execution scope
    // and require code flow analysis to resolve.
    _current_scope_code_flow_expressions: Option<P<RefCell<JsSet<Atom>>>>,

    // If we're actively binding a match statement, this is the current
    // match expression.
    _current_match_subj_expr: Option<ExpressionNode>,

    // Aliases of "typing" and "typing_extensions".
    _typing_import_aliases: Vec<String>,

    // Aliases of "sys".
    _sys_import_aliases: Vec<String>,

    // Aliases of "dataclasses".
    _dataclasses_import_aliases: Vec<String>,

    // Map of imports of specific symbols imported from "typing" and "typing_extensions"
    // and the names they alias to.
    _typing_symbol_aliases: JsMap<String, String>,

    // Map of imports of specific symbols imported from "dataclasses"
    // and the names they alias to.
    _dataclasses_symbol_aliases: JsMap<String, String>,

    // List of names statically assigned to __all__ symbol.
    _dunder_all_names: Option<Vec<String>>,

    // List of string nodes associated with the "__all__" symbol.
    _dunder_all_string_nodes: Vec<StringNode>,

    // One or more statements are manipulating __all__ in a manner that a
    // static analyzer doesn't understand.
    _uses_unsupported_dunder_all_form: bool,

    // Are we currently binding code located within an except block?
    _is_in_except_suite: bool,

    // Are we currently walking the type arguments to an Annotated type annotation?
    _is_in_annotated_annotation: bool,

    // A list of names assigned to __slots__ within a class.
    _dunder_slots_entries: Option<Vec<StringListNode>>,

    // Map of symbols at the module level that may be externally
    // hidden depending on whether they are listed in the __all__ list.
    _potential_hidden_symbols: JsMap<String, P<Symbol>>,

    // Map of symbols imported via wildcard import in a py.typed (non-stub)
    // module that should be treated as private if this module defines __all__
    // and the symbol is not listed there.
    _potential_wildcard_reexport_symbols: JsMap<String, P<Symbol>>,

    // Map of symbols at the module level that may be private depending
    // on whether they are listed in the __all__ list.
    _potential_private_symbols: JsMap<String, P<Symbol>>,

    // Estimates the overall complexity of the code flow graph for
    // the current function.
    _code_flow_complexity: f64,

    _module_symbol_only: Option<bool>,
    _cell_chain_index: Option<&'static dyn CellChainIndexProvider>,
}

impl Binder {
    // analyzer/binder.ts:4876
    pub fn new(
        file_info: P<AnalyzerFileInfo>,
        module_symbol_only: Option<bool>,
        cell_chain_index: Option<&'static dyn CellChainIndexProvider>,
        node_info: AnalyzerNodeInfoAccessor,
    ) -> Binder {
        // The class statics exist before any binder runs.
        unreachable_static_condition_flow_node();
        unreachable_structural_flow_node();

        Binder {
            _file_info: file_info,
            _node_info: node_info,
            _deferred_binding_tasks: Vec::new(),
            _current_scope: None,
            _current_flow_node: None,
            _target_function_declaration: None,
            _current_break_target: None,
            _current_continue_target: None,
            _current_true_target: None,
            _current_false_target: None,
            _current_except_targets: Vec::new(),
            _finally_targets: Vec::new(),
            _current_return_target: None,
            _current_scope_code_flow_expressions: None,
            _current_match_subj_expr: None,
            _typing_import_aliases: Vec::new(),
            _sys_import_aliases: Vec::new(),
            _dataclasses_import_aliases: Vec::new(),
            _typing_symbol_aliases: JsMap::default(),
            _dataclasses_symbol_aliases: JsMap::default(),
            _dunder_all_names: None,
            _dunder_all_string_nodes: Vec::new(),
            _uses_unsupported_dunder_all_form: false,
            _is_in_except_suite: false,
            _is_in_annotated_annotation: false,
            _dunder_slots_entries: None,
            _potential_hidden_symbols: JsMap::default(),
            _potential_wildcard_reexport_symbols: JsMap::default(),
            _potential_private_symbols: JsMap::default(),
            _code_flow_complexity: 0.0,
            _module_symbol_only: module_symbol_only,
            _cell_chain_index: cell_chain_index,
        }
    }

    /// `this._currentScope` (assigned before any use).
    fn cur_scope(&self) -> P<Scope> {
        self._current_scope.unwrap()
    }

    /// `this._currentFlowNode!`.
    fn cur_flow(&self) -> P<FlowNode> {
        self._current_flow_node.unwrap()
    }

    /// `this._currentScopeCodeFlowExpressions!`.
    fn cur_code_flow_expressions(&self) -> P<RefCell<JsSet<Atom>>> {
        self._current_scope_code_flow_expressions.unwrap()
    }

    /// `convertTextRangeToRange(range, this._fileInfo.lines)`.
    fn _convert_range(&self, range: &impl HasTextRange) -> Range {
        convert_text_range_to_range(&range.text_range(), self._file_info.lines)
    }

    // analyzer/binder.ts:375
    pub fn bind_module(&mut self, node: ModuleNode) {
        // We'll assume that if there is no builtins scope provided, we must be
        // binding the builtins module itself.
        let is_built_in_module = self._file_info.builtins_scope.is_none();
        let chained_module_level_scope_lookup = self._create_cell_chain_module_level_lookup();

        self._add_typing_import_aliases_from_builtins_scope();

        self._create_new_scope(
            if is_built_in_module {
                ScopeType::Builtin
            } else {
                ScopeType::Module
            },
            self._file_info.builtins_scope,
            /* proxyScope */ None,
            chained_module_level_scope_lookup,
            move |this| {
                this._node_info.set_scope(node, this.cur_scope());
                // `this._currentFlowNode!` is still undefined here, so TypeScript
                // stores undefined: the module's node info has no flow node.
                if let Some(flow_node) = this._current_flow_node {
                    this._node_info.set_flow_node(node, flow_node);
                }

                // Bind implicit names.
                // List taken from https://docs.python.org/3/reference/import.html#__name__
                this._add_implicit_symbol_to_current_scope("__name__", node.node(), "str", true);
                this._add_implicit_symbol_to_current_scope("__loader__", node.node(), "Any", true);
                this._add_implicit_symbol_to_current_scope("__package__", node.node(), "str | None", true);
                this._add_implicit_symbol_to_current_scope("__spec__", node.node(), "Any", true);
                this._add_implicit_symbol_to_current_scope(
                    "__path__",
                    node.node(),
                    "MutableSequence[str]",
                    true,
                );
                this._add_implicit_symbol_to_current_scope("__file__", node.node(), "str", true);
                this._add_implicit_symbol_to_current_scope("__cached__", node.node(), "str", true);
                this._add_implicit_symbol_to_current_scope(
                    "__annotations__",
                    node.node(),
                    "dict[str, Any]",
                    true,
                );
                this._add_implicit_symbol_to_current_scope("__dict__", node.node(), "dict[str, Any]", true);
                this._add_implicit_symbol_to_current_scope("__builtins__", node.node(), "Any", true);
                this._add_implicit_symbol_to_current_scope("__doc__", node.node(), "str | None", true);

                // Create a start node for the module.
                this._current_flow_node = Some(this._create_start_flow_node());

                let statements = node.d().statements.borrow().clone();
                this._walk_statements_and_report_unreachable(&statements);

                // Associate the code flow node at the end of the module with the module.
                this._node_info.set_after_flow_node(node, this.cur_flow());

                this._node_info
                    .set_code_flow_expressions(node, this.cur_code_flow_expressions());
                this._node_info
                    .set_code_flow_complexity(node, this._code_flow_complexity);
            },
        );

        // Perform all analysis that was deferred during the first pass.
        self._bind_deferred();

        // Use the __all__ list to determine whether any potential private symbols should be
        // made externally hidden or private. When __all__ uses an unsupported form (e.g.,
        // dynamic construction like __all__ = _components + [...]), we can't determine
        // membership statically; fall back to name-convention heuristics so that
        // underscore-prefixed names are still treated as private while normally-named
        // symbols avoid false positives.
        let uses_unsupported = self._uses_unsupported_dunder_all_form;
        let should_process = |name: &str| !uses_unsupported || is_private_or_protected_name(name);

        let in_dunder_all = |names: &Option<Vec<String>>, name: &str| {
            names.as_ref().is_some_and(|names| names.iter().any(|sym| sym == name))
        };

        for (name, symbol) in &self._potential_hidden_symbols {
            if should_process(name) && !in_dunder_all(&self._dunder_all_names, name) {
                if self._file_info.is_stub_file {
                    symbol.set_is_externally_hidden();
                } else {
                    symbol.set_private_py_typed_import();
                }
            }
        }

        // Wildcard imports are considered a re-export form, but if this module defines
        // __all__, that list determines the public interface and should restrict which
        // wildcard-imported symbols are exposed.
        if let Some(dunder_all_names) = &self._dunder_all_names {
            for (name, symbol) in &self._potential_wildcard_reexport_symbols {
                if should_process(name) && !dunder_all_names.iter().any(|sym| sym == name) {
                    symbol.set_private_py_typed_import();
                }
            }
        }

        // Single-underscore module-level names remain private even when __all__ uses an
        // unsupported/computed form. Since every entry in _potentialPrivateSymbols already
        // has an underscore prefix, shouldProcess always returns true here, preserving
        // the behavior of upstream pyright test `Private3`.
        for (name, symbol) in &self._potential_private_symbols {
            if should_process(name) && !in_dunder_all(&self._dunder_all_names, name) {
                symbol.set_is_private_member();
            }
        }

        if let Some(dunder_all_names) = &self._dunder_all_names {
            self._node_info.set_dunder_all_info(
                node,
                Some(P::new(DunderAllInfo {
                    names: dunder_all_names.as_slice().into(),
                    string_nodes: self._dunder_all_string_nodes.clone(),
                    uses_unsupported_dunder_all_form: self._uses_unsupported_dunder_all_form,
                })),
            );
        } else {
            self._node_info.set_dunder_all_info(node, /* names */ None);
        }

        // Set __all__ flags on the module symbols.
        let scope = self._node_info.get_scope(node);
        if let (Some(scope), Some(dunder_all_names)) = (scope, &self._dunder_all_names) {
            for name in dunder_all_names {
                if let Some(symbol) = scope.symbol_table.get(name) {
                    symbol.set_is_in_dunder_all();
                }
            }
        }
    }
}

impl ParseTreeVisitor<bool> for Binder {
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/binder.ts:481
    fn visit_module(&mut self, _node: ModuleNode) -> bool {
        // Tree walking should start with the children of
        // the node, so we should never get here.
        fail(Some("We should never get here"));
    }

    // analyzer/binder.ts:488
    fn visit_suite(&mut self, node: SuiteNode) -> bool {
        let statements = node.d().statements.borrow().clone();
        self._walk_statements_and_report_unreachable(&statements);
        false
    }

    // analyzer/binder.ts:493
    fn visit_module_name(&mut self, node: ModuleNameNode) -> bool {
        let import_result = self._node_info.get_import_info(node);
        assert(import_result.is_some(), None);
        let import_result = import_result.unwrap();

        if import_result.is_native_lib {
            return true;
        }

        if !import_result.is_import_found && !import_result.import_name.is_empty() {
            self._add_diagnostic(
                DiagnosticRule::reportMissingImports,
                LocMessage::import_resolve_failure().format(&[
                    ("importName", &import_result.import_name),
                    ("venv", &self._file_info.execution_environment.name),
                ]),
                &node.text_range(),
            );
            return true;
        }

        // See if a source file was found but it's not part of a py.typed
        // library and no type stub is found.
        let mut report_stub_missing = false;
        if !import_result.is_stub_file
            && import_result.import_type == ImportType::ThirdParty
            && import_result.py_typed_info.is_none()
        {
            report_stub_missing = true;

            // If the import is a namespace package, it's possible that all of
            // the targeted import symbols are py.typed submodules. In this case,
            // suppress the missing stub diagnostic.
            if import_result.is_namespace_package {
                if let Some(parent) = node.parent.get().and_then(ImportFromNode::cast) {
                    if parent.d().imports.borrow().iter().all(|import_as| {
                        let implicit_import = import_result
                            .filtered_implicit_imports
                            .as_ref()
                            .and_then(|imports| imports.get(import_as.d().name.get().d().value()));
                        implicit_import.is_some_and(|i| i.py_typed_info.is_some())
                    }) {
                        report_stub_missing = false;
                    }
                }
            }
        }

        if report_stub_missing {
            let diagnostic = self._add_diagnostic(
                DiagnosticRule::reportMissingTypeStubs,
                LocMessage::stub_file_missing()
                    .format(&[("importName", &import_result.import_name)]),
                &node.text_range(),
            );
            if let Some(diagnostic) = diagnostic {
                // Add a diagnostic action for resolving this diagnostic.
                let create_type_stub_action = DiagnosticAction {
                    action: "pyright.createtypestub".to_string(),
                    module_name: Some(import_result.import_name.clone()),
                };
                diagnostic.add_action(create_type_stub_action);
            }
        }

        true
    }

    // analyzer/binder.ts:557
    fn visit_class(&mut self, node: ClassNode) -> bool {
        let decorators: Vec<_> = node.d().decorators.borrow().iter().map(|d| Some(d.node())).collect();
        self.walk_multiple(&decorators);

        let class_declaration = Declaration::create(
            DeclarationType::Class,
            node.node(),
            self._file_info.file_uri,
            self._convert_range(&node.d().name.get()),
            self._file_info.module_name.clone(),
            self._is_in_except_suite,
            NoDeclarationDetails,
        );

        let symbol = self._bind_name_to_scope(self.cur_scope(), node.d().name.get(), None);
        if let Some(symbol) = symbol {
            symbol.add_declaration(class_declaration);
        }

        // Stash the declaration in the parse node for later access.
        self._node_info.set_declaration(node, class_declaration);

        let mut type_param_scope: Option<P<Scope>> = None;
        if let Some(type_params) = node.d().type_params.get() {
            self.walk(type_params.node());
            type_param_scope = self._node_info.get_scope(type_params);
        }

        let arguments: Vec<_> = node.d().arguments.borrow().iter().map(|a| Some(a.node())).collect();
        self.walk_multiple(&arguments);

        let parent_scope = type_param_scope.unwrap_or_else(|| self._get_non_class_parent_scope());
        self._create_new_scope(
            ScopeType::Class,
            Some(parent_scope),
            /* proxyScope */ None,
            /* chainedModuleLevelScopeLookup */ None,
            move |this| {
                this._node_info.set_scope(node, this.cur_scope());

                this._add_implicit_symbol_to_current_scope("__doc__", node.node(), "str | None", true);
                this._add_implicit_symbol_to_current_scope("__module__", node.node(), "str", true);

                this._dunder_slots_entries = None;
                if this._module_symbol_only != Some(true) {
                    // Analyze the suite.
                    this.walk(node.d().suite.get().node());
                }

                // `__qualname__` is exposed via the metaclass (`type`) rather than as a
                // class/instance attribute, unlike `__doc__`/`__module__`. We handle it
                // after walking the suite so we can tell whether the class body already
                // declared it.
                let existing_qualname = this.cur_scope().look_up_symbol("__qualname__");
                if let Some(existing_qualname) = existing_qualname {
                    // The class explicitly declares `__qualname__` (e.g. typeshed `type`,
                    // `function`, or a user `__qualname__ = "..."`). Keep that real
                    // declaration untouched so hover, go-to-definition, and completion all
                    // resolve to it. We must not append a synthetic empty-range Intrinsic
                    // declaration on top, because declaration-selecting consumers (e.g.
                    // `getLastTypedDeclarationForSymbol`) would otherwise resolve to the
                    // empty range instead of the real declaration. We still mark it as
                    // ignored for protocol matching, matching the implicit-dunder treatment.
                    existing_qualname.set_is_ignored_for_protocol_match();
                } else {
                    // The class does not declare `__qualname__`. Add it as a non-class
                    // member so it is name-resolvable within the class body (e.g.
                    // `print(__qualname__)`) but is not exposed as a class/instance
                    // attribute. Otherwise instance access (`instance.__qualname__`) would
                    // incorrectly resolve instead of reporting an attribute-access error.
                    this._add_implicit_symbol_to_current_scope(
                        "__qualname__",
                        node.node(),
                        "str",
                        /* isClassMember */ false,
                    );
                }

                if let Some(entries) = this._dunder_slots_entries.clone() {
                    this._add_slots_to_current_scope(&entries);
                }
                this._dunder_slots_entries = None;
            },
        );

        self._create_assignment_target_flow_nodes(
            node.d().name.get().node(),
            /* walkTargets */ false,
            /* unbound */ false,
        );

        false
    }

    // analyzer/binder.ts:638
    fn visit_function(&mut self, node: FunctionNode) -> bool {
        self._create_variable_annotation_flow_node();
        self._node_info.set_flow_node(node, self.cur_flow());

        let symbol = self._bind_name_to_scope(self.cur_scope(), node.d().name.get(), None);
        let containing_class_node = ParseTreeUtils::get_enclosing_class(node.node(), /* stopAtFunction */ true);
        let function_declaration = Declaration::create(
            DeclarationType::Function,
            node.node(),
            self._file_info.file_uri,
            self._convert_range(&node.d().name.get()),
            self._file_info.module_name.clone(),
            self._is_in_except_suite,
            FunctionDeclarationDetails {
                is_method: Cell::new(containing_class_node.is_some()),
                is_generator: Cell::new(false),
                return_statements: OptionRefCell::new(),
                yield_statements: OptionRefCell::new(),
                raise_statements: OptionRefCell::new(),
            },
        );

        if let Some(symbol) = symbol {
            symbol.add_declaration(function_declaration);
        }

        // Stash the declaration in the parse node for later access.
        self._node_info.set_declaration(node, function_declaration);

        // Walk the default values prior to the type parameters.
        let params = node.d().params.borrow().clone();
        for param in &params {
            if let Some(default_value) = param.d().default_value.get() {
                self.walk(default_value);
            }
        }

        let mut type_param_scope: Option<P<Scope>> = None;
        if let Some(type_params) = node.d().type_params.get() {
            self.walk(type_params.node());
            type_param_scope = self._node_info.get_scope(type_params);
        }

        let decorators: Vec<_> = node.d().decorators.borrow().iter().map(|d| Some(d.node())).collect();
        self.walk_multiple(&decorators);

        for param in &params {
            if let Some(annotation) = param.d().annotation.get() {
                self.walk(annotation);
            }

            if let Some(annotation_comment) = param.d().annotation_comment.get() {
                self.walk(annotation_comment);
            }
        }

        if let Some(return_annotation) = node.d().return_annotation.get() {
            self.walk(return_annotation);
        }

        if let Some(func_annotation_comment) = node.d().func_annotation_comment.get() {
            self.walk(func_annotation_comment.node());
        }

        // Don't walk the body of the function until we're done analyzing
        // the current scope.
        let parent_scope = type_param_scope.unwrap_or_else(|| self._get_non_class_parent_scope());
        self._create_new_scope(
            ScopeType::Function,
            Some(parent_scope),
            /* proxyScope */ None,
            /* chainedModuleLevelScopeLookup */ None,
            move |this| {
                this._node_info.set_scope(node, this.cur_scope());

                let enclosing_class = ParseTreeUtils::get_enclosing_class(node.node(), false);
                if enclosing_class.is_some() {
                    // Add the implicit "__class__" symbol described in PEP 3135.
                    this._add_implicit_symbol_to_current_scope("__class__", node.node(), "__class__", true);
                }

                this._defer_binding(Box::new(move |this: &mut Binder| {
                    // Create a start node for the function.
                    this._current_flow_node = Some(this._create_start_flow_node());
                    this._code_flow_complexity = 0.0;

                    let params = node.d().params.borrow().clone();
                    for param_node in params {
                        if let Some(param_name) = param_node.d().name.get() {
                            let symbol = this._bind_name_to_scope(this.cur_scope(), param_name, None);

                            if let Some(symbol) = symbol {
                                let param_declaration = Declaration::create(
                                    DeclarationType::Param,
                                    param_node.node(),
                                    this._file_info.file_uri,
                                    this._convert_range(&param_node),
                                    this._file_info.module_name.clone(),
                                    this._is_in_except_suite,
                                    ParamDeclarationDetails {
                                        inferred_name: OptionRefCell::new(),
                                        inferred_type_nodes: OptionRefCell::new(),
                                    },
                                );

                                symbol.add_declaration(param_declaration);
                                this._node_info.set_declaration(param_name, param_declaration);
                            }

                            this._create_flow_assignment(param_name.node(), false);
                        }
                    }

                    this._target_function_declaration = Some(function_declaration);
                    this._current_return_target = Some(this._create_branch_label(None).label());

                    // Walk the statements that make up the function.
                    this.walk(node.d().suite.get().node());

                    this._target_function_declaration = None;

                    // Associate the code flow node at the end of the suite with
                    // the suite.
                    this._node_info.set_after_flow_node(node.d().suite.get(), this.cur_flow());

                    // Compute the final return flow node and associate it with
                    // the function's parse node. If this node is unreachable, then
                    // the function never returns.
                    let return_target = this._current_return_target.unwrap();
                    this._add_antecedent(return_target, this.cur_flow());
                    let return_flow_node = this._finish_flow_label(return_target);

                    this._node_info.set_after_flow_node(node, return_flow_node);

                    this._node_info
                        .set_code_flow_expressions(node, this.cur_code_flow_expressions());
                    this._node_info
                        .set_code_flow_complexity(node, this._code_flow_complexity);
                }));
            },
        );

        self._create_assignment_target_flow_nodes(
            node.d().name.get().node(),
            /* walkTargets */ false,
            /* unbound */ false,
        );

        // We'll walk the child nodes in a deferred manner, so don't walk them now.
        false
    }

    // analyzer/binder.ts:770
    fn visit_lambda(&mut self, node: LambdaNode) -> bool {
        self._create_variable_annotation_flow_node();
        self._node_info.set_flow_node(node, self.cur_flow());

        // Analyze the parameter defaults in the context of the parent's scope
        // before we add any names from the function's scope.
        let params = node.d().params.borrow().clone();
        for param in &params {
            if let Some(default_value) = param.d().default_value.get() {
                self.walk(default_value);
            }
        }

        let parent_scope = self._get_non_class_parent_scope();
        self._create_new_scope(
            ScopeType::Function,
            Some(parent_scope),
            /* proxyScope */ None,
            /* chainedModuleLevelScopeLookup */ None,
            move |this| {
                this._node_info.set_scope(node, this.cur_scope());

                let enclosing_class = ParseTreeUtils::get_enclosing_class(node.node(), false);
                if enclosing_class.is_some() {
                    // Lambdas create the same implicit __class__ closure as named functions.
                    this._add_implicit_symbol_to_current_scope("__class__", node.node(), "__class__", true);
                }

                this._defer_binding(Box::new(move |this: &mut Binder| {
                    // Create a start node for the lambda.
                    this._current_flow_node = Some(this._create_start_flow_node());

                    let params = node.d().params.borrow().clone();
                    for param_node in params {
                        if let Some(param_name) = param_node.d().name.get() {
                            let symbol = this._bind_name_to_scope(this.cur_scope(), param_name, None);
                            if let Some(symbol) = symbol {
                                let param_declaration = Declaration::create(
                                    DeclarationType::Param,
                                    param_node.node(),
                                    this._file_info.file_uri,
                                    this._convert_range(&param_node),
                                    this._file_info.module_name.clone(),
                                    this._is_in_except_suite,
                                    ParamDeclarationDetails {
                                        inferred_name: OptionRefCell::new(),
                                        inferred_type_nodes: OptionRefCell::new(),
                                    },
                                );

                                symbol.add_declaration(param_declaration);
                                this._node_info.set_declaration(param_name, param_declaration);
                            }

                            this._create_flow_assignment(param_name.node(), false);
                            this.walk(param_name.node());
                            this._node_info.set_flow_node(param_node, this.cur_flow());
                        }
                    }

                    // Walk the expression that make up the lambda body.
                    this.walk(node.d().expr.get());

                    this._node_info
                        .set_code_flow_expressions(node, this.cur_code_flow_expressions());
                }));
            },
        );

        // We'll walk the child nodes in a deferred manner.
        false
    }

    // analyzer/binder.ts:835
    fn visit_call(&mut self, node: CallNode) -> bool {
        if self.cur_scope().type_ == ScopeType::Class
            && does_call_expose_class_namespace(self.cur_scope(), node)
        {
            self.cur_scope().has_potentially_dynamic_symbol_table.set(true);
        }

        self._disable_true_false_targets(move |this| {
            this.walk(node.d().left_expr.get());

            let sorted_args = ParseTreeUtils::get_args_by_runtime_order(node);

            for arg_node in sorted_args {
                if let Some(current_flow_node) = this._current_flow_node {
                    this._node_info.set_flow_node(arg_node, current_flow_node);
                }
                this.walk(arg_node.node());
            }
        });

        // Create a call flow node. We'll skip this if the call is part of
        // a decorator. We assume that decorators are not NoReturn functions.
        // There are libraries that make extensive use of unannotated decorators,
        // and this can lead to a performance issue when walking the control
        // flow graph if we need to evaluate every decorator.
        if !ParseTreeUtils::is_node_contained_within_node_type(node.node(), ParseNodeType::Decorator) {
            // Skip if we're in an 'Annotated' annotation because this creates
            // problems for "No Return" return type analysis when annotation
            // evaluation is deferred.
            if !self._is_in_annotated_annotation {
                self._create_call_flow_node(node);
            }
        }

        // Is this an manipulation of dunder all?
        let left_member_access = MemberAccessNode::cast(node.d().left_expr.get());
        if self.cur_scope().type_ == ScopeType::Module
            && left_member_access.is_some_and(|left| {
                name_value(left.d().left_expr.get()) == Some("__all__")
            })
        {
            let left = left_member_access.unwrap();
            let mut emit_dunder_all_warning = true;
            let member_name = left.d().member.get().d().value().to_string();
            let args = node.d().args.borrow().clone();

            // Is this a call to "__all__.extend()"?
            if member_name == "extend" && args.len() == 1 {
                let arg_expr = args[0].d().value_expr.get();

                // Is this a call to "__all__.extend([<list>])"?
                if let Some(list) = ListNode::cast(arg_expr) {
                    let items = list.d().items.borrow().clone();
                    if items.iter().all(|list_entry_node| {
                        if let Some(string_node) = single_string_node(*list_entry_node) {
                            if let Some(names) = self._dunder_all_names.as_mut() {
                                names.push(string_value(string_node));
                            }
                            self._dunder_all_string_nodes.push(string_node);
                            return true;
                        }

                        false
                    }) {
                        emit_dunder_all_warning = false;
                    }
                } else if let Some(member_access) = MemberAccessNode::cast(arg_expr).filter(|m| {
                    m.d().left_expr.get().node_type == ParseNodeType::Name
                        && m.d().member.get().d().value() == "__all__"
                }) {
                    // Is this a call to "__all__.extend(<mod>.__all__)"?
                    let names_to_add = self._get_dunder_all_names_from_import(
                        name_value(member_access.d().left_expr.get()).unwrap(),
                    );
                    if let Some(names_to_add) = names_to_add {
                        if !names_to_add.is_empty() {
                            for name in names_to_add {
                                if let Some(names) = self._dunder_all_names.as_mut() {
                                    names.push(name);
                                }
                            }
                        }
                    }
                    emit_dunder_all_warning = false;
                }
            } else if member_name == "remove" && args.len() == 1 {
                // Is this a call to "__all__.remove()"?
                let arg_expr = args[0].d().value_expr.get();
                if let Some(string_node) = single_string_node(arg_expr) {
                    if let Some(dunder_all_names) = &mut self._dunder_all_names {
                        let value = string_value(string_node);
                        dunder_all_names.retain(|name| *name != value);
                        self._dunder_all_string_nodes
                            .retain(|node| string_value(*node) != value);
                        emit_dunder_all_warning = false;
                    }
                }
            } else if member_name == "append" && args.len() == 1 {
                // Is this a call to "__all__.append()"?
                let arg_expr = args[0].d().value_expr.get();
                if let Some(string_node) = single_string_node(arg_expr) {
                    if let Some(names) = self._dunder_all_names.as_mut() {
                        names.push(string_value(string_node));
                    }
                    self._dunder_all_string_nodes.push(string_node);
                    emit_dunder_all_warning = false;
                }
            }

            if emit_dunder_all_warning {
                self._uses_unsupported_dunder_all_form = true;

                self._add_diagnostic(
                    DiagnosticRule::reportUnsupportedDunderAll,
                    LocMessage::unsupported_dunder_all_operation(),
                    &node.text_range(),
                );
            }
        }

        false
    }

    // analyzer/binder.ts:956
    fn visit_type_parameter_list(&mut self, node: TypeParameterListNode) -> bool {
        let type_param_scope = Scope::new(
            ScopeType::TypeParameter,
            Some(self._get_non_class_parent_scope()),
            Some(self.cur_scope()),
            None,
        );

        let params = node.d().params.borrow().clone();
        for param in &params {
            if let Some(bound_expr) = param.d().bound_expr.get() {
                self.walk(bound_expr);
            }
        }

        let mut type_params_seen: JsSet<String> = JsSet::default();

        for param in &params {
            let name = param.d().name.get();
            let symbol = type_param_scope.add_symbol(name.d().value(), SymbolFlags::None);
            let param_declaration = Declaration::create(
                DeclarationType::TypeParam,
                param.node(),
                self._file_info.file_uri,
                self._convert_range(&node),
                self._file_info.module_name.clone(),
                self._is_in_except_suite,
                NoDeclarationDetails,
            );

            symbol.add_declaration(param_declaration);
            self._node_info.set_declaration(name, param_declaration);

            if type_params_seen.contains(name.d().value()) {
                self._add_syntax_error(
                    LocMessage::type_parameter_existing_type_parameter()
                        .format(&[("name", name.d().value())]),
                    &name.text_range(),
                );
            } else {
                type_params_seen.insert(name.d().value().to_string());
            }
        }

        for param in &params {
            if let Some(default_expr) = param.d().default_expr.get() {
                self.walk(default_expr);
            }
        }

        self._node_info.set_scope(node, type_param_scope);

        false
    }

    // analyzer/binder.ts:1003
    fn visit_type_alias(&mut self, node: TypeAliasNode) -> bool {
        self._bind_name_to_scope(self.cur_scope(), node.d().name.get(), None);

        self.walk(node.d().name.get().node());

        let mut type_param_scope: Option<P<Scope>> = None;
        if let Some(type_params) = node.d().type_params.get() {
            self.walk(type_params.node());
            type_param_scope = self._node_info.get_scope(type_params);
        }

        let type_alias_declaration = Declaration::create(
            DeclarationType::TypeAlias,
            node.node(),
            self._file_info.file_uri,
            self._convert_range(&node.d().name.get()),
            self._file_info.module_name.clone(),
            self._is_in_except_suite,
            TypeAliasDeclarationDetails {
                doc_string: self._get_variable_doc_string(node.d().expr.get()).into(),
            },
        );

        let symbol = self._bind_name_to_scope(self.cur_scope(), node.d().name.get(), None);
        if let Some(symbol) = symbol {
            symbol.add_declaration(type_alias_declaration);
        }

        // Stash the declaration in the parse node for later access.
        self._node_info.set_declaration(node, type_alias_declaration);

        self._create_assignment_target_flow_nodes(
            node.d().name.get().node(),
            /* walkTargets */ true,
            /* unbound */ false,
        );

        let prev_scope = self._current_scope;
        self._current_scope = type_param_scope.or(self._current_scope);
        self.walk(node.d().expr.get());
        self._current_scope = prev_scope;

        false
    }

    // analyzer/binder.ts:1042
    fn visit_assignment(&mut self, node: AssignmentNode) -> bool {
        if self._handle_typing_stub_assignment_or_annotation(node.node()) {
            return false;
        }

        self._bind_possible_tuple_named_target(node.d().left_expr.get(), None);

        if let Some(annotation_comment) = node.d().annotation_comment.get() {
            self.walk(annotation_comment);
            self._add_type_declaration_for_variable(node.d().left_expr.get(), annotation_comment);
        }

        if let Some(chained_annotation_comment) = node.d().chained_annotation_comment.get() {
            self._add_diagnostic(
                DiagnosticRule::reportInvalidTypeForm,
                LocMessage::annotation_not_supported(),
                &chained_annotation_comment.text_range(),
            );
        }

        // If the assignment target base expression is potentially a
        // TypedDict, add the base expression to the flow expressions set
        // to accommodate TypedDict type narrowing.
        if let Some(target) = IndexNode::cast(node.d().left_expr.get()) {
            let items = target.d().items.borrow().clone();
            if items.len() == 1
                && !target.d().trailing_comma.get()
                && items[0].d().value_expr.get().node_type == ParseNodeType::StringList
            {
                if is_code_flow_supported_for_reference(target.d().left_expr.get()) {
                    let base_expr_reference_key = create_key_for_reference(target.d().left_expr.get());
                    self.cur_code_flow_expressions()
                        .borrow_mut()
                        .insert(base_expr_reference_key);
                }
            }
        }

        self.walk(node.d().right_expr.get());

        let mut is_possible_type_alias = true;
        if ParseTreeUtils::get_enclosing_function(node.node()).is_some() {
            // We will assume that type aliases are defined only at the module level
            // or as class variables, not as local variables within a function.
            is_possible_type_alias = false;
        } else if node.d().right_expr.get().node_type == ParseNodeType::Call
            && self._file_info.is_typing_stub_file
        {
            // Some special built-in types defined in typing.pyi use
            // assignments of the form List = _Alias(). We don't want to
            // treat these as type aliases.
            is_possible_type_alias = false;
        } else if ParseTreeUtils::is_within_loop(node.node()) {
            // Assume that it's not a type alias if it's within a loop.
            is_possible_type_alias = false;
        }

        self._add_inferred_type_assignment_for_variable(
            node.d().left_expr.get(),
            node.d().right_expr.get(),
            is_possible_type_alias,
        );

        // If we didn't create assignment target flow nodes above, do so now.
        self._create_assignment_target_flow_nodes(
            node.d().left_expr.get(),
            /* walkTargets */ true,
            /* unbound */ false,
        );

        // Is this an assignment to dunder all?
        // Process __all__ for both Module and Builtin scope types. The Builtin scope type
        // is used when binding without a builtins scope (e.g., in indexing scenarios where
        // typeshed may not be available). We still want to extract __all__ information
        // to properly filter wildcard imports.
        let scope_type = self.cur_scope().type_;
        if scope_type == ScopeType::Module || scope_type == ScopeType::Builtin {
            if is_name_or_annotated_name(node.d().left_expr.get(), "__all__") {
                let expr = node.d().right_expr.get();
                self._dunder_all_names = Some(Vec::new());
                let mut emit_dunder_all_warning = false;

                let items = match expr.d() {
                    D::List(d) => Some(d.items.borrow().clone()),
                    D::Tuple(d) => Some(d.items.borrow().clone()),
                    _ => None,
                };
                if let Some(items) = items {
                    for entry_node in items {
                        if let Some(string_node) = single_string_node(entry_node) {
                            self._dunder_all_names
                                .as_mut()
                                .unwrap()
                                .push(string_value(string_node));
                            self._dunder_all_string_nodes.push(string_node);
                        } else {
                            emit_dunder_all_warning = true;
                        }
                    }
                } else {
                    emit_dunder_all_warning = true;
                }

                if emit_dunder_all_warning {
                    self._uses_unsupported_dunder_all_form = true;

                    self._add_diagnostic(
                        DiagnosticRule::reportUnsupportedDunderAll,
                        LocMessage::unsupported_dunder_all_operation(),
                        &node.text_range(),
                    );
                }
            }
        }

        // Is this an assignment to dunder slots?
        if self.cur_scope().type_ == ScopeType::Class {
            if is_name_or_annotated_name(node.d().left_expr.get(), "__slots__") {
                let expr = node.d().right_expr.get();
                let mut entries: Vec<StringListNode> = Vec::new();
                let mut is_expression_understood = true;

                match expr.d() {
                    D::StringList(_) => {
                        entries.push(StringListNode(expr));
                    }
                    D::List(_) | D::Tuple(_) => {
                        let items = match expr.d() {
                            D::List(d) => d.items.borrow().clone(),
                            D::Tuple(d) => d.items.borrow().clone(),
                            _ => unreachable!(),
                        };
                        for entry_node in items {
                            if single_string_node(entry_node).is_some() {
                                entries.push(StringListNode(entry_node));
                            } else {
                                is_expression_understood = false;
                            }
                        }
                    }
                    D::Dictionary(d) => {
                        let items = d.items.borrow().clone();
                        for dictionary_entry_node in items {
                            let key_expr = match dictionary_entry_node.d() {
                                D::DictionaryKeyEntry(entry) => Some(entry.key_expr.get()),
                                _ => None,
                            };
                            if let Some(key_expr) = key_expr.filter(|k| single_string_node(*k).is_some()) {
                                entries.push(StringListNode(key_expr));
                            } else {
                                is_expression_understood = false;
                            }
                        }
                    }
                    _ => {
                        is_expression_understood = false;
                    }
                }

                self.cur_scope().set_has_non_empty_slots(
                    entries
                        .iter()
                        .any(|entry| first_string_value(*entry) != "__dict__"),
                );
                self._dunder_slots_entries = Some(entries);

                if !is_expression_understood {
                    self._dunder_slots_entries = None;
                }
            }
        }

        false
    }

    // analyzer/binder.ts:1228
    fn visit_assignment_expression(&mut self, node: AssignmentExpressionNode) -> bool {
        // Temporarily disable true/false targets in case this assignment
        // expression is located within an if/else conditional.
        self._disable_true_false_targets(move |this| {
            // Evaluate the operand expression.
            this.walk(node.d().right_expr.get());
        });

        let evaluation_node =
            ParseTreeUtils::get_evaluation_node_for_assignment_expression(node, self._node_info.reader());
        match evaluation_node {
            None => {
                self._add_syntax_error(LocMessage::assignment_expr_context(), &node.text_range());
                self.walk(node.d().name.get().node());
            }
            Some(evaluation_node) => {
                // Bind the name to the containing scope. This special logic is required
                // because of the behavior defined in PEP 572. Targets of assignment
                // expressions don't bind to a list comprehension's scope but instead
                // bind to its containing scope.
                let container_scope = self._node_info.get_scope(evaluation_node).unwrap();

                // If we're in a list comprehension (possibly nested), make sure that
                // local for targets don't collide with the target of the assignment
                // expression.
                let name = node.d().name.get();
                let mut cur_scope = self._current_scope;
                while let Some(scope) = cur_scope.filter(|scope| *scope != container_scope) {
                    let local_symbol = scope.look_up_symbol(name.d().value_atom());
                    if local_symbol.is_some() {
                        self._add_syntax_error(
                            LocMessage::assignment_expr_comprehension()
                                .format(&[("name", name.d().value())]),
                            &name.text_range(),
                        );
                        break;
                    }

                    cur_scope = scope.parent;
                }

                self._bind_name_to_scope(container_scope, name, None);
                self._add_inferred_type_assignment_for_variable(
                    name.node(),
                    node.d().right_expr.get(),
                    false,
                );
                self._create_assignment_target_flow_nodes(
                    name.node(),
                    /* walkTargets */ true,
                    /* unbound */ false,
                );
            }
        }

        false
    }

    // analyzer/binder.ts:1272
    fn visit_augmented_assignment(&mut self, node: AugmentedAssignmentNode) -> bool {
        self.walk(node.d().left_expr.get());
        self.walk(node.d().right_expr.get());

        self._bind_possible_tuple_named_target(node.d().dest_expr.get(), None);
        self._create_assignment_target_flow_nodes(
            node.d().dest_expr.get(),
            /* walkTargets */ false,
            /* unbound */ false,
        );

        self._add_inferred_type_assignment_for_variable(
            node.d().dest_expr.get(),
            node.d().right_expr.get(),
            false,
        );

        // Is this an assignment to dunder all of the form
        // __all__ += <expression>?
        if node.d().operator.get() == OperatorType::AddEqual
            && self.cur_scope().type_ == ScopeType::Module
            && name_value(node.d().left_expr.get()) == Some("__all__")
        {
            let expr = node.d().right_expr.get();
            let mut emit_dunder_all_warning = true;

            if let Some(list) = ListNode::cast(expr) {
                // Is this the form __all__ += ["a", "b"]?
                let items = list.d().items.borrow().clone();
                for list_entry_node in items {
                    if let Some(string_node) = single_string_node(list_entry_node) {
                        if let Some(names) = self._dunder_all_names.as_mut() {
                            names.push(string_value(string_node));
                        }
                        self._dunder_all_string_nodes.push(string_node);
                    }
                }
                emit_dunder_all_warning = false;
            } else if let Some(member_access) = MemberAccessNode::cast(expr).filter(|m| {
                m.d().left_expr.get().node_type == ParseNodeType::Name
                    && m.d().member.get().d().value() == "__all__"
            }) {
                // Is this using the form "__all__ += <mod>.__all__"?
                let names_to_add = self._get_dunder_all_names_from_import(
                    name_value(member_access.d().left_expr.get()).unwrap(),
                );
                if let Some(names_to_add) = names_to_add {
                    for name in names_to_add {
                        if let Some(names) = self._dunder_all_names.as_mut() {
                            names.push(name);
                        }
                    }

                    emit_dunder_all_warning = false;
                }
            }

            if emit_dunder_all_warning {
                self._uses_unsupported_dunder_all_form = true;

                self._add_diagnostic(
                    DiagnosticRule::reportUnsupportedDunderAll,
                    LocMessage::unsupported_dunder_all_operation(),
                    &node.text_range(),
                );
            }
        }

        false
    }

    // analyzer/binder.ts:1335
    fn visit_del(&mut self, node: DelNode) -> bool {
        let targets = node.d().targets.borrow().clone();
        for expr in targets {
            self._bind_possible_tuple_named_target(expr, None);
            self.walk(expr);
            self._create_assignment_target_flow_nodes(expr, /* walkTargets */ false, /* unbound */ true);
        }

        false
    }

    // analyzer/binder.ts:1345
    fn visit_type_annotation(&mut self, node: TypeAnnotationNode) -> bool {
        if self._handle_typing_stub_assignment_or_annotation(node.node()) {
            return false;
        }

        // If this is an annotated variable assignment within a class body,
        // we need to evaluate the type annotation first.
        let bind_variable_before_annotation_evaluation = node
            .parent
            .get()
            .is_some_and(|parent| parent.node_type == ParseNodeType::Assignment)
            && ParseTreeUtils::get_enclosing_class(node.node(), /* stopAtFunction */ true).is_some();

        if !bind_variable_before_annotation_evaluation {
            self.walk(node.d().annotation.get());
        }

        self._create_variable_annotation_flow_node();

        self._bind_possible_tuple_named_target(node.d().value_expr.get(), None);
        self._add_type_declaration_for_variable(node.d().value_expr.get(), node.d().annotation.get());

        if bind_variable_before_annotation_evaluation {
            self.walk(node.d().annotation.get());
        }

        // For type annotations that are not part of assignments (e.g. simple variable
        // annotations), we need to populate the reference map. Otherwise the type
        // analyzer's code flow engine won't run and detect cases where the variable
        // is unbound.
        let mut expression_list: Vec<CodeFlowReferenceExpressionNode> = Vec::new();
        if self._is_narrowing_expression(node.d().value_expr.get(), &mut expression_list, NarrowExprOptions::default()) {
            for expr in &expression_list {
                let reference_key = create_key_for_reference(*expr);
                self.cur_code_flow_expressions().borrow_mut().insert(reference_key);
            }
        }

        self.walk(node.d().value_expr.get());

        false
    }

    // analyzer/binder.ts:1386
    fn visit_for(&mut self, node: ForNode) -> bool {
        self._bind_possible_tuple_named_target(node.d().target_expr.get(), None);
        self._add_inferred_type_assignment_for_variable(node.d().target_expr.get(), node.node(), false);

        self.walk(node.d().iterable_expr.get());

        let pre_for_label = self._create_loop_label();
        let pre_else_label = self._create_branch_label(None).label();
        let post_for_label = self._create_branch_label(None).label();

        // Determine if this loop is guaranteed to execute at least once
        let is_guaranteed_to_execute = self._is_non_empty_list_or_tuple_literal(node.d().iterable_expr.get());

        self._add_antecedent(pre_for_label, self.cur_flow());
        self._current_flow_node = Some(pre_for_label.node());

        // Only add zero-iteration path for potentially-empty iterables
        if !is_guaranteed_to_execute {
            self._add_antecedent(pre_else_label, self.cur_flow());
        }

        let target_expressions = self._track_code_flow_expressions(move |this| {
            this._create_assignment_target_flow_nodes(
                node.d().target_expr.get(),
                /* walkTargets */ true,
                /* unbound */ false,
            );
        });

        // Record antecedent count before the loop body to detect continue back-edges.
        let pre_body_antecedent_count = pre_for_label.d().antecedents.borrow().len();

        self._bind_loop_statement(pre_for_label, post_for_label, move |this| {
            this.walk(node.d().for_suite.get().node());
            this._add_antecedent(pre_for_label, this.cur_flow());

            // Add any target expressions since they are modified in the loop.
            let values: Vec<Atom> = target_expressions.borrow().iter().copied().collect();
            for value in values {
                if let Some(expressions) = this._current_scope_code_flow_expressions {
                    expressions.borrow_mut().insert(value);
                }
            }
        });

        // For guaranteed loops, add post-body exit path to preElseLabel.
        // When _currentFlowNode is reachable (normal completion or conditional break),
        // use it directly — it carries the post-body type state.
        // When _currentFlowNode is unreachable (all paths end with break/continue/return/raise),
        // we must distinguish the cause:
        //   - All break: preElseLabel gets nothing. Python's else doesn't run after break,
        //     and break already sent the assigned-state to postForLabel.
        //   - All continue: preForLabel accumulated continue back-edges. Use it as an
        //     approximation for the loop-completion state feeding into else.
        //   - All return/raise: preElseLabel gets nothing. Post-loop is unreachable.
        //   - Mix with continue: if any continues occurred, use preForLabel for else path.
        if is_guaranteed_to_execute {
            if self
                .cur_flow()
                .flags()
                .any(FlowFlags::UnreachableStructural | FlowFlags::UnreachableStaticCondition)
            {
                // Check if any continue statements added back-edges to preForLabel.
                let has_continue_back_edges =
                    pre_for_label.d().antecedents.borrow().len() > pre_body_antecedent_count;

                if has_continue_back_edges {
                    // Some paths continued — use preForLabel (with accumulated continue state)
                    // as the else-clause antecedent.
                    let saved_flow_node = self.cur_flow();
                    self._current_flow_node = Some(pre_for_label.node());
                    self._add_antecedent(pre_else_label, pre_for_label.node());
                    self._current_flow_node = Some(saved_flow_node);
                }
                // Otherwise (all break / all return / all raise): preElseLabel gets no
                // antecedent. For break, the flow already reached postForLabel directly.
                // For return/raise, post-loop code is unreachable.
            } else {
                // Normal completion or conditional break — use current flow node.
                self._add_antecedent(pre_else_label, self.cur_flow());
            }
        }

        self._current_flow_node = Some(self._finish_flow_label(pre_else_label));
        if let Some(else_suite) = node.d().else_suite.get() {
            self.walk(else_suite.node());
        }
        self._add_antecedent(post_for_label, self.cur_flow());

        self._current_flow_node = Some(self._finish_flow_label(post_for_label));

        // Async for is not allowed outside of an async function
        // unless we're in ipython mode.
        if let Some(async_token) = node.d().async_token.get() {
            if self._file_info.ipython_mode == IPythonMode::None {
                let enclosing_function = ParseTreeUtils::get_enclosing_function(node.node());
                if enclosing_function.is_none_or(|f| !f.d().is_async.get()) {
                    self._add_syntax_error(LocMessage::async_not_in_async_function(), &async_token.text_range());
                }
            }
        }

        false
    }

    // analyzer/binder.ts:1480
    fn visit_continue(&mut self, _node: ContinueNode) -> bool {
        if let Some(target) = self._current_continue_target {
            self._add_antecedent(target, self.cur_flow());
        }
        self._current_flow_node = Some(unreachable_structural_flow_node());

        // Continue nodes don't have any children.
        false
    }

    // analyzer/binder.ts:1490
    fn visit_break(&mut self, _node: BreakNode) -> bool {
        if let Some(target) = self._current_break_target {
            self._add_antecedent(target, self.cur_flow());
        }
        self._current_flow_node = Some(unreachable_structural_flow_node());

        // Break nodes don't have any children.
        false
    }

    // analyzer/binder.ts:1500
    fn visit_return(&mut self, node: ReturnNode) -> bool {
        if let Some(decl) = self._target_function_declaration {
            let function = decl.d().function().unwrap();
            let mut return_statements = function.return_statements.borrow_mut();
            return_statements.get_or_insert_with(Vec::new).push(node);
        }

        if let Some(expr) = node.d().expr.get() {
            self._node_info.set_flow_node(expr, self.cur_flow());
            self.walk(expr);
        }

        self._node_info.set_flow_node(node, self.cur_flow());
        if let Some(target) = self._current_return_target {
            self._add_antecedent(target, self.cur_flow());
        }
        for target in self._finally_targets.clone() {
            self._add_antecedent(target, self.cur_flow());
        }
        self._current_flow_node = Some(unreachable_structural_flow_node());
        false
    }

    // analyzer/binder.ts:1524
    fn visit_yield(&mut self, node: YieldNode) -> bool {
        if self._is_in_comprehension(node.node(), /* ignoreOutermostIterable */ true) {
            self._add_syntax_error(LocMessage::yield_within_comprehension(), &node.text_range());
        }

        self._bind_yield(node.node());
        false
    }

    // analyzer/binder.ts:1533
    fn visit_yield_from(&mut self, node: YieldFromNode) -> bool {
        if self._is_in_comprehension(node.node(), /* ignoreOutermostIterable */ true) {
            self._add_syntax_error(LocMessage::yield_within_comprehension(), &node.text_range());
        }

        self._bind_yield(node.node());
        false
    }

    // analyzer/binder.ts:1542
    fn visit_member_access(&mut self, node: MemberAccessNode) -> bool {
        self.walk(node.d().left_expr.get());
        self._node_info.set_flow_node(node, self.cur_flow());
        false
    }

    // analyzer/binder.ts:1548
    fn visit_name(&mut self, node: NameNode) -> bool {
        self._node_info.set_flow_node(node, self.cur_flow());
        false
    }

    // analyzer/binder.ts:1553
    fn visit_index(&mut self, node: IndexNode) -> bool {
        self._node_info.set_flow_node(node, self.cur_flow());

        self.walk(node.d().left_expr.get());

        // If we're within an 'Annotated' type annotation, set the flag.
        let was_in_annotated_annotation = self._is_in_annotated_annotation;
        if self._is_typing_annotation(node.d().left_expr.get(), "Annotated") {
            self._is_in_annotated_annotation = true;
        }

        let items = node.d().items.borrow().clone();
        for arg_node in items {
            self.walk(arg_node.node());
        }

        self._is_in_annotated_annotation = was_in_annotated_annotation;

        false
    }

    // analyzer/binder.ts:1573
    fn visit_if(&mut self, node: IfNode) -> bool {
        let pre_if_flow_node = self.cur_flow();
        let then_label = self._create_branch_label(None).label();
        let else_label = self._create_branch_label(None).label();
        let post_if_label = self._create_branch_label(Some(pre_if_flow_node)).label();

        let affected = self._track_code_flow_expressions(move |this| {
            // Determine if the test condition is always true or always false. If so,
            // we can treat either the then or the else clause as unconditional.
            let const_expr_value = StaticExpressions::evaluate_static_bool_like_expression(
                node.d().test_expr.get(),
                &this._file_info.execution_environment,
                &this._file_info.defined_constants,
                Some(&this._typing_import_aliases),
                Some(&this._sys_import_aliases),
            );
            this._node_info.set_static_condition_value(node, const_expr_value);

            this._bind_conditional(node.d().test_expr.get(), then_label, else_label);

            // Handle the if clause.
            this._current_flow_node = Some(if const_expr_value == Some(false) {
                unreachable_static_condition_flow_node()
            } else {
                this._finish_flow_label(then_label)
            });
            this.walk(node.d().if_suite.get().node());
            this._add_antecedent(post_if_label, this.cur_flow());

            // Now handle the else clause if it's present. If there
            // are chained "else if" statements, they'll be handled
            // recursively here.
            this._current_flow_node = Some(if const_expr_value == Some(true) {
                unreachable_static_condition_flow_node()
            } else {
                this._finish_flow_label(else_label)
            });
            if let Some(else_suite) = node.d().else_suite.get() {
                this.walk(else_suite);
            } else {
                this._bind_never_condition(node.d().test_expr.get(), post_if_label, /* isPositiveTest */ false);
            }
            this._add_antecedent(post_if_label, this.cur_flow());
            this._current_flow_node = Some(this._finish_flow_label(post_if_label));
        });
        post_if_label.d().affected_expressions.set(Some(affected));

        false
    }

    // analyzer/binder.ts:1618
    fn visit_while(&mut self, node: WhileNode) -> bool {
        let then_label = self._create_branch_label(None).label();
        let else_label = self._create_branch_label(None).label();
        let post_while_label = self._create_branch_label(None).label();

        // Determine if the test condition is always true or always false. If so,
        // we can treat either the while or the else clause as unconditional.
        let const_expr_value = StaticExpressions::evaluate_static_bool_like_expression(
            node.d().test_expr.get(),
            &self._file_info.execution_environment,
            &self._file_info.defined_constants,
            Some(&self._typing_import_aliases),
            Some(&self._sys_import_aliases),
        );

        let pre_loop_label = self._create_loop_label();
        self._add_antecedent(pre_loop_label, self.cur_flow());
        self._current_flow_node = Some(pre_loop_label.node());

        self._bind_conditional(node.d().test_expr.get(), then_label, else_label);

        // Handle the while clause.
        self._current_flow_node = Some(if const_expr_value == Some(false) {
            unreachable_static_condition_flow_node()
        } else {
            self._finish_flow_label(then_label)
        });
        self._bind_loop_statement(pre_loop_label, post_while_label, move |this| {
            this.walk(node.d().while_suite.get().node());
        });
        self._add_antecedent(pre_loop_label, self.cur_flow());

        self._current_flow_node = Some(if const_expr_value == Some(true) {
            unreachable_static_condition_flow_node()
        } else {
            self._finish_flow_label(else_label)
        });
        if let Some(else_suite) = node.d().else_suite.get() {
            self.walk(else_suite.node());
        }
        self._add_antecedent(post_while_label, self.cur_flow());
        self._current_flow_node = Some(self._finish_flow_label(post_while_label));
        false
    }

    // analyzer/binder.ts:1657
    fn visit_assert(&mut self, node: AssertNode) -> bool {
        let assert_true_label = self._create_branch_label(None).label();
        let assert_false_label = self._create_branch_label(None).label();

        self._bind_conditional(node.d().test_expr.get(), assert_true_label, assert_false_label);

        if let Some(exception_expr) = node.d().exception_expr.get() {
            self._current_flow_node = Some(self._finish_flow_label(assert_false_label));
            self.walk(exception_expr);
        }

        self._current_flow_node = Some(self._finish_flow_label(assert_true_label));
        false
    }

    // analyzer/binder.ts:1672
    fn visit_except(&mut self, node: ExceptNode) -> bool {
        if let Some(type_expr) = node.d().type_expr.get() {
            self.walk(type_expr);
        }

        if let Some(name) = node.d().name.get() {
            self.walk(name.node());
            let symbol = self._bind_name_to_scope(self.cur_scope(), name, None);
            self._create_assignment_target_flow_nodes(name.node(), /* walkTargets */ true, /* unbound */ false);

            if let Some(symbol) = symbol {
                let details = VariableDeclarationDetails::default();
                details.is_constant.set(Some(is_constant_name(name.d().value())));
                details.inferred_type_source.set(Some(node.node()));
                details
                    .is_explicit_binding
                    .set(Some(self.cur_scope().get_binding_type(name.d().value()).is_some()));
                let declaration = Declaration::create(
                    DeclarationType::Variable,
                    name.node(),
                    self._file_info.file_uri,
                    self._convert_range(&name),
                    self._file_info.module_name.clone(),
                    self._is_in_except_suite,
                    details,
                );
                symbol.add_declaration(declaration);
            }
        }

        let was_in_except_suite = self._is_in_except_suite;
        self._is_in_except_suite = true;
        self.walk(node.d().except_suite.get().node());
        self._is_in_except_suite = was_in_except_suite;

        if let Some(name) = node.d().name.get() {
            // The exception name is implicitly unbound at the end of
            // the except block.
            self._create_flow_assignment(name.node(), /* unbound */ true);
        }

        false
    }

    // analyzer/binder.ts:1712
    fn visit_raise(&mut self, node: RaiseNode) -> bool {
        if let Some(current_flow_node) = self._current_flow_node {
            self._add_except_targets(current_flow_node);
        }

        if let Some(decl) = self._target_function_declaration {
            let function = decl.d().function().unwrap();
            let mut raise_statements = function.raise_statements.borrow_mut();
            raise_statements.get_or_insert_with(Vec::new).push(node);
        }

        if let Some(expr) = node.d().expr.get() {
            self.walk(expr);
        }
        if let Some(from_expr) = node.d().from_expr.get() {
            self.walk(from_expr);
        }

        for target in self._finally_targets.clone() {
            self._add_antecedent(target, self.cur_flow());
        }

        self._current_flow_node = Some(unreachable_structural_flow_node());
        false
    }

    // analyzer/binder.ts:1739
    fn visit_try(&mut self, node: TryNode) -> bool {
        // The try/except/else/finally statement is tricky to model using static code
        // flow rules because the finally clause is executed regardless of whether an
        // exception is raised or a return statement is executed. Code within the finally
        // clause needs to be reachable always, and we conservatively assume that any
        // statement within the try block can generate an exception, so we assume that its
        // antecedent is the pre-try flow. We implement this with a "gate" node in the
        // control flow graph. If analysis starts within the finally clause, the gate is
        // opened, and all raise/return statements within try/except/else blocks are
        // considered antecedents. If analysis starts outside (after) the finally clause,
        // the gate is closed, and only paths that don't hit a raise/return statement
        // in try/except/else blocks are considered.
        //
        //
        //                               1. PostElse
        //                                    ^
        //                                    |
        // 3. TryExceptElseReturnOrExcept     |
        //       ^                            |
        //       |                            |     2. PostExcept (for each except)
        //       |                            |            ^
        // 4. ReturnOrRaiseLabel              |            |
        //       ^                            |            |
        //       |                            |   |---------
        // 5. PreFinallyGate                  |   |
        //       ^                            |   |
        //       |------------------          |   |
        //                         |          |   |
        //                        6. PreFinallyLabel
        //                                ^
        //                         (finally block)
        //                                ^
        //                        7. PostFinally
        //                                ^    (only if isAfterElseAndExceptsReachable)
        //                         (after finally)

        // Create one flow label for every except clause.
        let pre_try_flow_node = self.cur_flow();
        let except_clauses = node.d().except_clauses.borrow().clone();
        let mut cur_except_targets: Vec<FlowLabel> = except_clauses
            .iter()
            .map(|_| self._create_branch_label(None).label())
            .collect();
        let pre_finally_label = self._create_branch_label(Some(pre_try_flow_node)).label();
        let mut is_after_else_and_excepts_reachable = false;

        // Create a label for all of the return or raise labels that are
        // encountered within the try/except/else blocks. This conditionally
        // connects the return/raise statement to the finally clause.
        let pre_finally_return_or_raise_label = self._create_branch_label(Some(pre_try_flow_node)).label();

        let pre_finally_gate = FlowPreFinallyGate(FlowNode::create(
            FlowFlags::PreFinallyGate,
            self._get_unique_flow_node_id(),
            FlowPreFinallyGateDetails {
                antecedent: pre_finally_return_or_raise_label.node(),
            },
        ));

        let affected = self._track_code_flow_expressions(|this| {
            if node.d().finally_suite.get().is_some() {
                this._add_antecedent(pre_finally_label, pre_finally_gate.node());
            }

            // Add the finally target as an exception target unless there is
            // a "bare" except clause that accepts all exception types.
            let has_bare_except_clause = except_clauses
                .iter()
                .any(|except| except.d().type_expr.get().is_none());
            if !has_bare_except_clause {
                cur_except_targets.push(pre_finally_return_or_raise_label);
            }

            // An exception may be generated before the first flow node
            // added by the try block, so all of the exception targets
            // must have the pre-try flow node as an antecedent.
            for except_label in &cur_except_targets {
                this._add_antecedent(*except_label, this.cur_flow());
            }

            // We don't perfectly handle nested finally clauses, which are not
            // possible to model fully within a static analyzer, but we do handle
            // a single level of finally statements, and we handle most cases
            // involving nesting. Returns or raises within the try/except/raise
            // block will execute the finally target(s).
            if node.d().finally_suite.get().is_some() {
                this._finally_targets.push(pre_finally_return_or_raise_label);
            }

            // Handle the try block.
            this._use_except_targets(cur_except_targets.clone(), |this| {
                this.walk(node.d().try_suite.get().node());
            });

            // Handle the else block, which is executed only if
            // execution falls through the try block.
            if let Some(else_suite) = node.d().else_suite.get() {
                this.walk(else_suite.node());
            }
            this._add_antecedent(pre_finally_label, this.cur_flow());
            if !this._is_code_unreachable() {
                is_after_else_and_excepts_reachable = true;
            }

            // Handle the except blocks.
            for (index, except_node) in except_clauses.iter().enumerate() {
                this._current_flow_node = Some(this._finish_flow_label(cur_except_targets[index]));
                this.walk(except_node.node());
                this._add_antecedent(pre_finally_label, this.cur_flow());
                if !this._is_code_unreachable() {
                    is_after_else_and_excepts_reachable = true;
                }
            }

            if node.d().finally_suite.get().is_some() {
                this._finally_targets.pop();
            }

            // Handle the finally block.
            this._current_flow_node = Some(this._finish_flow_label(pre_finally_label));
        });
        pre_finally_label.d().affected_expressions.set(Some(affected));

        if let Some(finally_suite) = node.d().finally_suite.get() {
            self.walk(finally_suite.node());

            // Add a post-finally node at the end. If we traverse this node,
            // we'll set the "ignore" flag in the pre-finally node.
            let post_finally_node = FlowNode::create(
                FlowFlags::PostFinally,
                self._get_unique_flow_node_id(),
                FlowPostFinallyDetails {
                    finally_node: finally_suite,
                    antecedent: self.cur_flow(),
                    pre_finally_gate,
                },
            );
            self._current_flow_node = Some(if is_after_else_and_excepts_reachable {
                post_finally_node
            } else {
                unreachable_structural_flow_node()
            });
        }

        false
    }

    // analyzer/binder.ts:1873
    fn visit_await(&mut self, node: AwaitNode) -> bool {
        // Make sure this is within an async lambda or function.
        let exec_scope_node = ParseTreeUtils::get_execution_scope_node(node.node(), self._node_info.reader());
        let is_async_function = FunctionNode::cast(exec_scope_node).is_some_and(|f| f.d().is_async.get());
        if !is_async_function {
            if self._file_info.ipython_mode != IPythonMode::None
                && exec_scope_node.node_type == ParseNodeType::Module
            {
                // Top level await is allowed in ipython mode.
                return true;
            }

            let parent = node.parent.get();
            let grandparent_type = parent.and_then(|p| p.parent.get()).map(|gp| gp.node_type);
            let is_in_generator = parent.is_some_and(|p| p.node_type == ParseNodeType::Comprehension)
                && grandparent_type != Some(ParseNodeType::List)
                && grandparent_type != Some(ParseNodeType::Set)
                && grandparent_type != Some(ParseNodeType::Dictionary);

            // Allow if it's within a generator expression. Execution of
            // generator expressions is deferred and therefore can be
            // run within the context of an async function later.
            if !is_in_generator {
                self._add_syntax_error(
                    LocMessage::await_not_in_async(),
                    &node.d().await_token.get().text_range(),
                );
            }
        }

        true
    }

    // analyzer/binder.ts:1899
    fn visit_global(&mut self, node: GlobalNode) -> bool {
        let global_scope = self.cur_scope().get_global_scope().scope;

        let targets = node.d().targets.borrow().clone();
        for name in targets {
            let name_value = name.d().value();

            // Is the binding inconsistent?
            if self.cur_scope().get_binding_type(name_value) == Some(NameBindingType::Nonlocal) {
                self._add_syntax_error(
                    LocMessage::non_local_redefinition().format(&[("name", name_value)]),
                    &name.text_range(),
                );
            }

            let value_with_scope = self.cur_scope().look_up_symbol_recursive(name_value, None);

            // Was the name already assigned within this scope before it was declared global?
            if value_with_scope.is_some_and(|v| v.scope == self.cur_scope()) {
                self._add_syntax_error(
                    LocMessage::global_reassignment().format(&[("name", name_value)]),
                    &name.text_range(),
                );
            }

            // Add it to the global scope if it's not already added.
            self._bind_name_to_scope(global_scope, name, None);

            if self.cur_scope() != global_scope {
                self.cur_scope().set_binding_type(name_value, NameBindingType::Global);
            }
        }

        true
    }

    // analyzer/binder.ts:1928
    fn visit_nonlocal(&mut self, node: NonlocalNode) -> bool {
        let global_scope = self.cur_scope().get_global_scope().scope;

        if self.cur_scope() == global_scope {
            self._add_syntax_error(LocMessage::non_local_in_module(), &node.text_range());
        } else {
            let targets = node.d().targets.borrow().clone();
            for name in targets {
                let name_value = name.d().value();

                // Is the binding inconsistent?
                if self.cur_scope().get_binding_type(name_value) == Some(NameBindingType::Global) {
                    self._add_syntax_error(
                        LocMessage::global_redefinition().format(&[("name", name_value)]),
                        &name.text_range(),
                    );
                }

                let value_with_scope = self.cur_scope().look_up_symbol_recursive(name_value, None);

                // Was the name already assigned within this scope before it was declared nonlocal?
                if value_with_scope.is_some_and(|v| v.scope == self.cur_scope()) {
                    self._add_syntax_error(
                        LocMessage::non_local_reassignment().format(&[("name", name_value)]),
                        &name.text_range(),
                    );
                } else if value_with_scope.is_none_or(|v| v.scope == global_scope) {
                    self._add_syntax_error(
                        LocMessage::non_local_no_binding().format(&[("name", name_value)]),
                        &name.text_range(),
                    );
                }

                if value_with_scope.is_some() {
                    self.cur_scope().set_binding_type(name_value, NameBindingType::Nonlocal);
                }
            }
        }

        true
    }

    // analyzer/binder.ts:1960
    fn visit_import_as(&mut self, node: ImportAsNode) -> bool {
        let module = node.d().module.get();
        let name_parts = module.d().name_parts.borrow().clone();
        if !name_parts.is_empty() {
            let first_name_part_value = name_parts[0].d().value().to_string();

            let symbol_name: String;
            let symbol_name_node: NameNode;
            if let Some(alias) = node.d().alias.get() {
                // The symbol name is defined by the alias.
                symbol_name = alias.d().value().to_string();
                symbol_name_node = alias;
            } else {
                // There was no alias, so we need to use the first element of
                // the name parts as the symbol.
                symbol_name = first_name_part_value.clone();
                symbol_name_node = name_parts[0];
            }

            let symbol = self._bind_name_to_scope(self.cur_scope(), symbol_name_node, None);
            if let Some(symbol) = symbol {
                let scope_type = self.cur_scope().type_;
                if (scope_type == ScopeType::Module || scope_type == ScopeType::Builtin)
                    && node.d().alias.get().is_none_or(|alias| {
                        name_parts.len() != 1 || name_parts[0].d().value() != alias.d().value()
                    })
                {
                    if self._file_info.is_stub_file || self._file_info.is_in_py_typed_package {
                        // PEP 484 indicates that imported symbols should not be
                        // considered "reexported" from a type stub file unless
                        // they are imported using the "as" form and the aliased
                        // name is entirely redundant.
                        self._potential_hidden_symbols.insert(symbol_name.clone(), symbol);
                    }
                }
            }

            let import_info = self._node_info.get_import_info(module);
            assert(import_info.is_some(), None);
            let import_info = import_info.unwrap();

            if let Some(symbol) = symbol {
                self._create_alias_declaration_for_multipart_import_name(
                    node.node(),
                    node.d().alias.get(),
                    Some(import_info),
                    symbol,
                );
            }

            self._create_flow_assignment(
                node.d().alias.get().unwrap_or(name_parts[0]).node(),
                false,
            );

            if name_parts.len() == 1 {
                let alias_value = node
                    .d()
                    .alias
                    .get()
                    .map(|alias| alias.d().value().to_string())
                    .unwrap_or_else(|| first_name_part_value.clone());
                if first_name_part_value == "typing" || first_name_part_value == "typing_extensions" {
                    self._typing_import_aliases.push(alias_value);
                } else if first_name_part_value == "sys" {
                    self._sys_import_aliases.push(alias_value);
                } else if first_name_part_value == "dataclasses" {
                    self._dataclasses_import_aliases.push(alias_value);
                }
            }
        }

        true
    }

    // analyzer/binder.ts:2017
    fn visit_import_from(&mut self, node: ImportFromNode) -> bool {
        let typing_symbols_of_interest = ["Final", "ClassVar", "Annotated"];
        let dataclasses_symbols_of_interest = ["InitVar", "KW_ONLY"];
        let module = node.d().module.get();
        let import_info = self._node_info.get_import_info(module);

        self._node_info.set_flow_node(node, self.cur_flow());

        let mut resolved_path = Uri::empty();
        if let Some(import_info) = import_info {
            if import_info.is_import_found && !import_info.is_native_lib {
                resolved_path = *import_info.resolved_uris.last().unwrap();
            }
        }

        // If this file is a module __init__.py(i), relative imports of submodules
        // using the syntax "from .x import y" introduce a symbol x into the
        // module namespace. We do this first (before adding the individual imported
        // symbols below) in case one of the imported symbols is the same name as the
        // submodule. In that case, we want to the symbol to appear later in the
        // declaration list because it should "win" when resolving the alias.
        let file_name = strip_file_extension(self._file_info.file_uri.file_name(), false);
        let name_parts = module.d().name_parts.borrow().clone();
        let is_module_init_file =
            file_name == "__init__" && module.d().leading_dots.get() == 1 && name_parts.len() == 1;

        let mut is_typing_import = false;
        let mut is_dataclasses_import = false;

        if name_parts.len() == 1 {
            let first_name_part_value = name_parts[0].d().value();
            if first_name_part_value == "typing" || first_name_part_value == "typing_extensions" {
                is_typing_import = true;
            }

            if first_name_part_value == "dataclasses" {
                is_dataclasses_import = true;
            }
        }

        if node.d().is_wildcard_import.get() {
            if ParseTreeUtils::get_enclosing_class(node.node(), false).is_some()
                || ParseTreeUtils::get_enclosing_function(node.node()).is_some()
            {
                self._add_syntax_error(LocMessage::wildcard_in_function(), &node.text_range());
            }

            if let Some(import_info) = import_info {
                let mut names: Vec<String> = Vec::new();

                // Note that this scope uses a wildcard import, so we cannot shortcut
                // any code flow checks. All expressions are potentially in play.
                if let Some(expressions) = self._current_scope_code_flow_expressions {
                    expressions
                        .borrow_mut()
                        .insert(wildcard_import_reference_key());
                }

                let lookup_info = (self._file_info.import_lookup)(ImportLookupKey::Uri(resolved_path), None);
                if let Some(lookup_info) = lookup_info {
                    let wildcard_names = get_wildcard_import_names(&lookup_info);

                    if is_module_init_file {
                        // If the symbol is going to be immediately replaced with a same-named
                        // imported symbol, skip this.
                        let is_immediately_replaced = wildcard_names
                            .iter()
                            .any(|name| *name == name_parts[0].d().value());

                        if !is_immediately_replaced {
                            self._add_implicit_from_import(node, Some(import_info));
                        }
                    }

                    for name in &wildcard_names {
                        let local_symbol = self._bind_name_value_to_scope(self.cur_scope(), Atom::new(name), None);

                        if let Some(local_symbol) = local_symbol {
                            let imported_symbol = lookup_info.symbol_table.get(name);

                            let scope_type = self.cur_scope().type_;
                            if (scope_type == ScopeType::Module || scope_type == ScopeType::Builtin)
                                && self._file_info.is_in_py_typed_package
                                && !self._file_info.is_stub_file
                            {
                                // Wildcard imports are considered a re-export form. If this module
                                // defines __all__, it determines the public interface, so we may
                                // need to treat wildcard-imported names as private unless listed.
                                self._potential_wildcard_reexport_symbols
                                    .insert(name.clone(), local_symbol);
                            }

                            // Is the symbol in the target module's symbol table? If so,
                            // alias it.
                            if let Some(imported_symbol) = imported_symbol {
                                if self._add_wildcard_imported_module_alias(node, local_symbol, imported_symbol) {
                                    names.push(name.clone());
                                } else {
                                    let details = AliasDeclarationDetails::new(false, true);
                                    *details.symbol_name.borrow_mut() = Some(name.clone());
                                    let alias_decl = Declaration::create(
                                        DeclarationType::Alias,
                                        node.node(),
                                        resolved_path,
                                        get_empty_range(), // Range is unknown for wildcard name import.
                                        self._file_info.module_name.clone(),
                                        self._is_in_except_suite,
                                        details,
                                    );
                                    local_symbol.add_declaration(alias_decl);
                                    names.push(name.clone());
                                }
                            } else {
                                // The symbol wasn't in the target module's symbol table. It's probably
                                // an implicitly-imported submodule referenced by __all__.
                                if let Some(filtered_implicit_imports) = &import_info.filtered_implicit_imports {
                                    let implicit_import = filtered_implicit_imports.get(name);

                                    if let Some(implicit_import) = implicit_import {
                                        let submodule_fallback = Declaration::create(
                                            DeclarationType::Alias,
                                            node.node(),
                                            implicit_import.uri,
                                            get_empty_range(),
                                            self._file_info.module_name.clone(),
                                            self._is_in_except_suite,
                                            AliasDeclarationDetails::new(false, true),
                                        );

                                        let details = AliasDeclarationDetails::new(false, true);
                                        *details.symbol_name.borrow_mut() = Some(name.clone());
                                        details.submodule_fallback.set(Some(submodule_fallback));
                                        let alias_decl = Declaration::create(
                                            DeclarationType::Alias,
                                            node.node(),
                                            resolved_path,
                                            get_empty_range(),
                                            self._file_info.module_name.clone(),
                                            self._is_in_except_suite,
                                            details,
                                        );

                                        local_symbol.add_declaration(alias_decl);
                                        names.push(name.clone());
                                    }
                                }
                            }

                            if is_typing_import {
                                local_symbol.set_typing_symbol_alias(name);
                            }
                        }
                    }
                }

                self._create_flow_wildcard_import(node, names);

                if is_typing_import {
                    for s in typing_symbols_of_interest {
                        self._typing_symbol_aliases.insert(s.to_string(), s.to_string());
                    }
                }

                if is_dataclasses_import {
                    for s in dataclasses_symbols_of_interest {
                        self._dataclasses_symbol_aliases.insert(s.to_string(), s.to_string());
                    }
                }
            }
        } else {
            if is_module_init_file {
                self._add_implicit_from_import(node, import_info);
            }

            let imports = node.d().imports.borrow().clone();
            for import_symbol_node in imports {
                let imported_name = import_symbol_node.d().name.get().d().value().to_string();
                let name_node = import_symbol_node
                    .d()
                    .alias
                    .get()
                    .unwrap_or(import_symbol_node.d().name.get());

                self._node_info.set_flow_node(import_symbol_node, self.cur_flow());

                let symbol = self._bind_name_to_scope(self.cur_scope(), name_node, None);

                if let Some(symbol) = symbol {
                    // All import statements of the form `from . import x` treat x
                    // as an externally-visible (not hidden) symbol.
                    if !name_parts.is_empty() {
                        let scope_type = self.cur_scope().type_;
                        if scope_type == ScopeType::Module || scope_type == ScopeType::Builtin {
                            if import_symbol_node
                                .d()
                                .alias
                                .get()
                                .is_none_or(|alias| alias.d().value() != import_symbol_node.d().name.get().d().value())
                            {
                                if self._file_info.is_stub_file || self._file_info.is_in_py_typed_package {
                                    // PEP 484 indicates that imported symbols should not be
                                    // considered "reexported" from a type stub file unless
                                    // they are imported using the "as" form using a redundant form.
                                    // Py.typed packages follow the same rule as PEP 484.
                                    self._potential_hidden_symbols
                                        .insert(name_node.d().value().to_string(), symbol);
                                }
                            }
                        }
                    }

                    // Is the import referring to an implicitly-imported module?
                    let mut implicit_import: Option<&ImplicitImport> = None;
                    if let Some(import_info) = import_info {
                        if let Some(filtered_implicit_imports) = &import_info.get().filtered_implicit_imports {
                            implicit_import = filtered_implicit_imports.get(&imported_name);
                        }
                    }

                    let is_lazy = if node.d().is_lazy.get() == Some(true) {
                        Some(true)
                    } else {
                        None
                    };

                    let mut submodule_fallback: Option<P<Declaration>> = None;
                    let mut load_symbols_from_path = true;
                    if let Some(implicit_import) = implicit_import {
                        let fallback_details = AliasDeclarationDetails::new(false, true);
                        fallback_details.is_lazy.set(is_lazy);
                        submodule_fallback = Some(Declaration::create(
                            DeclarationType::Alias,
                            import_symbol_node.node(),
                            implicit_import.uri,
                            get_empty_range(),
                            self._format_module_name(module).into(),
                            self._is_in_except_suite,
                            fallback_details,
                        ));

                        // Handle the case where this is an __init__.py file and the imported
                        // module name refers to itself. The most common situation where this occurs
                        // is with a "from . import X" form, but it can also occur with
                        // an absolute import (e.g. "from A.B.C import X"). In this case, we want to
                        // always resolve to the submodule rather than the resolved path.
                        if file_name == "__init__" {
                            if module.d().leading_dots.get() == 1 && name_parts.is_empty() {
                                load_symbols_from_path = false;
                            } else if resolved_path.equals(Some(self._file_info.file_uri)) {
                                load_symbols_from_path = false;
                            }
                        }
                    }

                    let details = AliasDeclarationDetails::new(
                        import_symbol_node.d().alias.get().is_some(),
                        load_symbols_from_path,
                    );
                    *details.symbol_name.borrow_mut() = Some(imported_name.clone());
                    details.submodule_fallback.set(submodule_fallback);
                    details.is_native_lib.set(import_info.map(|info| info.is_native_lib));
                    details.is_lazy.set(is_lazy);
                    let alias_decl = Declaration::create(
                        DeclarationType::Alias,
                        import_symbol_node.node(),
                        resolved_path,
                        self._convert_range(&name_node),
                        self._format_module_name(module).into(),
                        self._is_in_except_suite,
                        details,
                    );

                    symbol.add_declaration(alias_decl);
                    self._create_flow_assignment(name_node.node(), false);

                    if is_typing_import {
                        if typing_symbols_of_interest
                            .iter()
                            .any(|s| *s == import_symbol_node.d().name.get().d().value())
                        {
                            self._typing_symbol_aliases.insert(
                                name_node.d().value().to_string(),
                                import_symbol_node.d().name.get().d().value().to_string(),
                            );

                            if is_typing_import {
                                symbol.set_typing_symbol_alias(name_node.d().value());
                            }
                        }
                    }

                    if is_dataclasses_import {
                        if dataclasses_symbols_of_interest
                            .iter()
                            .any(|s| *s == import_symbol_node.d().name.get().d().value())
                        {
                            self._dataclasses_symbol_aliases.insert(
                                name_node.d().value().to_string(),
                                import_symbol_node.d().name.get().d().value().to_string(),
                            );
                        }
                    }
                }
            }
        }

        true
    }

    // analyzer/binder.ts:2288
    fn visit_with(&mut self, node: WithNode) -> bool {
        let with_items = node.d().with_items.borrow().clone();
        for item in &with_items {
            self.walk(item.d().expr.get());
            if let Some(target) = item.d().target.get() {
                self._bind_possible_tuple_named_target(target, None);
                self._add_inferred_type_assignment_for_variable(target, item.node(), false);
                self._create_assignment_target_flow_nodes(target, /* walkTargets */ true, /* unbound */ false);
            }
        }

        // We need to treat the "with" body as though it is wrapped in a try/except
        // block because some context managers catch and suppress exceptions.
        // We'll make use of a special "context manager label" which acts like
        // a regular branch label in most respects except that it is disabled
        // if none of the context managers support exception suppression. We won't
        // be able to determine whether any context managers support exception
        // processing until the type evaluation phase.
        //
        //  (pre with suite)
        //         ^
        //         |<--------------------|
        //    (with suite)<--------------|
        //         ^                     |
        //         |    ContextManagerSwallowExceptionTarget
        //         |                     ^
        //         |          PostContextManagerLabel
        //         |                     ^
        //         |---------------------|
        //         |
        //   (after with)
        //
        // In addition to the ContextManagerSwallowExceptionTarget, we'll create
        // a second target called ContextManagerForwardExceptionTarget that forwards
        // exceptions to existing exception targets if they exist.

        let is_async = node.d().is_async.get() == Some(true);
        let context_manager_swallow_exception_target = self._create_context_manager_label(
            with_items.iter().map(|item| item.d().expr.get()).collect(),
            is_async,
            /* blockIfSwallowsExceptions */ false,
        );
        self._add_antecedent(context_manager_swallow_exception_target.label(), self.cur_flow());

        let context_manager_forward_exception_target = self._create_context_manager_label(
            with_items.iter().map(|item| item.d().expr.get()).collect(),
            is_async,
            /* blockIfSwallowsExceptions */ true,
        );
        for exception_target in self._current_except_targets.clone() {
            self._add_antecedent(exception_target, context_manager_forward_exception_target.node());
        }

        let pre_with_suite_node = self.cur_flow();
        let post_context_manager_label = self._create_branch_label(Some(pre_with_suite_node)).label();
        self._add_antecedent(post_context_manager_label, context_manager_swallow_exception_target.node());

        let affected = self._track_code_flow_expressions(move |this| {
            this._use_except_targets(
                vec![
                    context_manager_swallow_exception_target.label(),
                    context_manager_forward_exception_target.label(),
                ],
                move |this| {
                    this.walk(node.d().suite.get().node());
                },
            );

            this._add_antecedent(post_context_manager_label, this.cur_flow());
            this._current_flow_node = Some(post_context_manager_label.node());

            // Model the call to `__exit__` as a potential exception generator.
            if !this._is_code_unreachable() {
                this._add_except_targets(this.cur_flow());
            }

            if let Some(async_token) = node.d().async_token.get() {
                if this._file_info.ipython_mode == IPythonMode::None {
                    // Top level async with is allowed in ipython mode.
                    let enclosing_function = ParseTreeUtils::get_enclosing_function(node.node());
                    if enclosing_function.is_none_or(|f| !f.d().is_async.get()) {
                        this._add_syntax_error(LocMessage::async_not_in_async_function(), &async_token.text_range());
                    }
                }
            }
        });
        post_context_manager_label.d().affected_expressions.set(Some(affected));

        false
    }

    // analyzer/binder.ts:2368
    fn visit_ternary(&mut self, node: TernaryNode) -> bool {
        let pre_ternary_flow_node = self.cur_flow();
        let true_label = self._create_branch_label(None).label();
        let false_label = self._create_branch_label(None).label();
        let post_expression_label = self._create_branch_label(Some(pre_ternary_flow_node)).label();

        let affected = self._track_code_flow_expressions(move |this| {
            // Handle the test expression.
            this._bind_conditional(node.d().test_expr.get(), true_label, false_label);

            // Handle the "true" portion (the "if" expression).
            this._current_flow_node = Some(this._finish_flow_label(true_label));
            this.walk(node.d().if_expr.get());
            this._add_antecedent(post_expression_label, this.cur_flow());

            // Handle the "false" portion (the "else" expression).
            this._current_flow_node = Some(this._finish_flow_label(false_label));
            this.walk(node.d().else_expr.get());
            this._add_antecedent(post_expression_label, this.cur_flow());

            this._current_flow_node = Some(this._finish_flow_label(post_expression_label));
        });
        post_expression_label.d().affected_expressions.set(Some(affected));

        false
    }

    // analyzer/binder.ts:2394
    fn visit_unary_operation(&mut self, node: UnaryOperationNode) -> bool {
        if node.d().operator.get() == OperatorType::Not
            && self._current_false_target.is_some()
            && self._current_true_target.is_some()
        {
            // Swap the existing true/false targets.
            self._bind_conditional(
                node.d().expr.get(),
                self._current_false_target.unwrap(),
                self._current_true_target.unwrap(),
            );
        } else {
            // Temporarily set the true/false targets to undefined because
            // this unary operation is not part of a chain of logical expressions
            // (AND/OR/NOT subexpressions).
            self._disable_true_false_targets(move |this| {
                // Evaluate the operand expression.
                this.walk(node.d().expr.get());
            });
        }

        false
    }

    // analyzer/binder.ts:2411
    fn visit_binary_operation(&mut self, node: BinaryOperationNode) -> bool {
        let operator = node.d().operator.get();
        if operator == OperatorType::And || operator == OperatorType::Or {
            let mut true_target = self._current_true_target;
            let mut false_target = self._current_false_target;
            let mut post_right_label: Option<FlowLabel> = None;

            if true_target.is_none() || false_target.is_none() {
                let label = self._create_branch_label(None).label();
                post_right_label = Some(label);
                true_target = Some(label);
                false_target = Some(label);
            }
            let true_target = true_target.unwrap();
            let false_target = false_target.unwrap();

            let pre_right_label = self._create_branch_label(None).label();
            if operator == OperatorType::And {
                self._bind_conditional(node.d().left_expr.get(), pre_right_label, false_target);
            } else {
                self._bind_conditional(node.d().left_expr.get(), true_target, pre_right_label);
            }
            self._current_flow_node = Some(self._finish_flow_label(pre_right_label));
            self._bind_conditional(node.d().right_expr.get(), true_target, false_target);
            if let Some(post_right_label) = post_right_label {
                self._current_flow_node = Some(self._finish_flow_label(post_right_label));
            }
        } else {
            // Temporarily set the true/false targets to undefined because
            // this binary operation is not part of a chain of logical expressions
            // (AND/OR/NOT subexpressions).
            self._disable_true_false_targets(move |this| {
                this.walk(node.d().left_expr.get());
                this.walk(node.d().right_expr.get());
            });
        }

        false
    }

    // analyzer/binder.ts:2446
    fn visit_comprehension(&mut self, node: ComprehensionNode) -> bool {
        let enclosing_function = ParseTreeUtils::get_enclosing_function(node.node());

        // The first iterable is executed outside of the comprehension scope.
        let for_if_nodes = node.d().for_if_nodes.borrow().clone();
        if let Some(first) = for_if_nodes.first() {
            if let D::ComprehensionFor(d) = first.d() {
                self.walk(d.iterable_expr.get());
            }
        }

        let parent_scope = self._get_non_class_parent_scope();
        self._create_new_scope(
            ScopeType::Comprehension,
            Some(parent_scope),
            /* proxyScope */ None,
            /* chainedModuleLevelScopeLookup */ None,
            move |this| {
                this._node_info.set_scope(node, this.cur_scope());

                let false_label = this._create_branch_label(None).label();

                // We'll walk the forIfNodes list twice. The first time we'll
                // bind targets of for statements. The second time we'll walk
                // expressions and create the control flow graph.
                for compr in &for_if_nodes {
                    let mut added_symbols: JsMap<String, P<Symbol>> = JsMap::default();
                    if let D::ComprehensionFor(d) = compr.d() {
                        this._bind_possible_tuple_named_target(d.target_expr.get(), Some(&mut added_symbols));
                        this._add_inferred_type_assignment_for_variable(d.target_expr.get(), *compr, false);

                        // Async for is not allowed outside of an async function
                        // unless we're in ipython mode.
                        if let Some(async_token) = d.async_token.get() {
                            if this._file_info.ipython_mode == IPythonMode::None {
                                if enclosing_function.is_none_or(|f| !f.d().is_async.get()) {
                                    // Allow if it's within a generator expression. Execution of
                                    // generator expressions is deferred and therefore can be
                                    // run within the context of an async function later.
                                    let parent_type = node.parent.get().map(|p| p.node_type);
                                    if parent_type == Some(ParseNodeType::List)
                                        || parent_type == Some(ParseNodeType::Set)
                                        || parent_type == Some(ParseNodeType::Dictionary)
                                    {
                                        this._add_syntax_error(
                                            LocMessage::async_not_in_async_function(),
                                            &async_token.text_range(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                for (i, compr) in for_if_nodes.iter().enumerate() {
                    match compr.d() {
                        D::ComprehensionFor(d) => {
                            // We already walked the first iterable expression above,
                            // so skip it here.
                            if i != 0 {
                                this.walk(d.iterable_expr.get());
                            }

                            this._create_assignment_target_flow_nodes(
                                d.target_expr.get(),
                                /* walkTargets */ true,
                                /* unbound */ false,
                            );
                        }
                        D::ComprehensionIf(d) => {
                            let true_label = this._create_branch_label(None).label();
                            this._bind_conditional(d.test_expr.get(), true_label, false_label);
                            this._current_flow_node = Some(this._finish_flow_label(true_label));
                        }
                        _ => unreachable!(),
                    }
                }

                this.walk(node.d().expr.get());
                this._add_antecedent(false_label, this.cur_flow());
                this._current_flow_node = Some(this._finish_flow_label(false_label));
            },
        );

        false
    }

    // analyzer/binder.ts:2523
    fn visit_match(&mut self, node: MatchNode) -> bool {
        // Evaluate the subject expression.
        self.walk(node.d().expr.get());

        let mut expression_list: Vec<CodeFlowReferenceExpressionNode> = Vec::new();
        let mut is_subject_narrowable =
            self._is_narrowing_expression(node.d().expr.get(), &mut expression_list, NarrowExprOptions::default());

        // We also support narrowing of individual tuple entries found within a
        // match subject expression, so add those here as well.
        if let D::Tuple(tuple) = node.d().expr.get().d() {
            let items = tuple.items.borrow().clone();
            for item_expr in items {
                if self._is_narrowing_expression(item_expr, &mut expression_list, NarrowExprOptions::default()) {
                    is_subject_narrowable = true;
                }
            }
        }

        if is_subject_narrowable {
            for expr in &expression_list {
                let reference_key = create_key_for_reference(*expr);
                self.cur_code_flow_expressions().borrow_mut().insert(reference_key);
            }
        }

        let post_match_label = self._create_branch_label(None).label();
        let mut found_irrefutable_case = false;

        // Model the match statement as a series of if/elif clauses
        // each of which tests for the specified pattern (and optionally
        // for the guard condition).
        let cases = node.d().cases.borrow().clone();
        for case_statement in cases {
            let post_case_label = self._create_branch_label(None).label();
            let pre_guard_label = self._create_branch_label(None).label();
            let pre_suite_label = self._create_branch_label(None).label();

            // Evaluate the pattern.
            self._add_antecedent(pre_guard_label, self.cur_flow());

            if !case_statement.d().is_irrefutable.get() {
                self._add_antecedent(post_case_label, self.cur_flow());
            } else if case_statement.d().guard_expr.get().is_none() {
                found_irrefutable_case = true;
            }

            self._current_flow_node = Some(self._finish_flow_label(pre_guard_label));

            // Note the active match subject expression prior to binding
            // the pattern. If the pattern involves any targets that overwrite
            // the subject expression, this will be set to undefined.
            self._current_match_subj_expr = Some(node.d().expr.get());

            // Bind the pattern.
            self.walk(case_statement.d().pattern.get());

            // If the pattern involves targets that overwrite the subject
            // expression, skip creating a flow node for narrowing the subject.
            if self._current_match_subj_expr.is_some() {
                self._create_flow_narrow_for_pattern(node.d().expr.get(), case_statement.node());
                self._current_match_subj_expr = None;
            }

            // Apply the guard expression.
            if let Some(guard_expr) = case_statement.d().guard_expr.get() {
                self._bind_conditional(guard_expr, pre_suite_label, post_case_label);
            } else {
                self._add_antecedent(pre_suite_label, self.cur_flow());
            }

            self._current_flow_node = Some(self._finish_flow_label(pre_suite_label));

            // Bind the body of the case statement.
            self.walk(case_statement.d().suite.get().node());
            self._add_antecedent(post_match_label, self.cur_flow());

            self._current_flow_node = Some(self._finish_flow_label(post_case_label));
        }

        // Add a final narrowing step for the subject expression for the entire
        // match statement. This will compute the narrowed type if no case
        // statements are matched.
        if is_subject_narrowable {
            self._create_flow_narrow_for_pattern(node.d().expr.get(), node.node());
        }

        // Create an "implied else" to conditionally gate code flow based on
        // whether the narrowed type of the subject expression is Never at this point.
        if !found_irrefutable_case {
            self._create_flow_exhausted_match(node);
        }

        self._add_antecedent(post_match_label, self.cur_flow());
        self._current_flow_node = Some(self._finish_flow_label(post_match_label));

        false
    }

    // analyzer/binder.ts:2619
    fn visit_pattern_as(&mut self, node: PatternAsNode) -> bool {
        let post_or_label = self._create_branch_label(None).label();

        let or_patterns = node.d().or_patterns.borrow().clone();
        for or_pattern in or_patterns {
            self.walk(or_pattern);
            self._add_antecedent(post_or_label, self.cur_flow());
        }

        self._current_flow_node = Some(self._finish_flow_label(post_or_label));

        if let Some(target) = node.d().target.get() {
            self.walk(target.node());
            let symbol = self._bind_name_to_scope(self.cur_scope(), target, None);
            self._create_assignment_target_flow_nodes(target.node(), /* walkTargets */ false, /* unbound */ false);

            if let Some(symbol) = symbol {
                let details = VariableDeclarationDetails::default();
                details.is_constant.set(Some(is_constant_name(target.d().value())));
                details.inferred_type_source.set(Some(node.node()));
                details
                    .is_explicit_binding
                    .set(Some(self.cur_scope().get_binding_type(target.d().value()).is_some()));
                let declaration = Declaration::create(
                    DeclarationType::Variable,
                    target.node(),
                    self._file_info.file_uri,
                    self._convert_range(&target),
                    self._file_info.module_name.clone(),
                    self._is_in_except_suite,
                    details,
                );
                symbol.add_declaration(declaration);
            }
        }

        false
    }

    // analyzer/binder.ts:2653
    fn visit_pattern_capture(&mut self, node: PatternCaptureNode) -> bool {
        if !node.d().is_wildcard.get() {
            self._add_pattern_capture_target(node.d().target.get());
        }

        true
    }

    // analyzer/binder.ts:2661
    fn visit_pattern_mapping_expand_entry(&mut self, node: PatternMappingExpandEntryNode) -> bool {
        if node.d().target.get().d().value() != "_" {
            self._add_pattern_capture_target(node.d().target.get());
        }

        true
    }
}

impl ParseTreeWalker for Binder {}

impl Binder {
    // analyzer/binder.ts:2672
    // Helper method to determine if an expression is a non-empty list or tuple literal.
    // This is a syntactic check, not a semantic one, so it's very fast.
    // Guards against starred expressions ([*empty_list]) and comprehensions ([v for v in []]).
    fn _is_non_empty_list_or_tuple_literal(&self, expr: ExpressionNode) -> bool {
        if let D::List(d) = expr.d() {
            let items = d.items.borrow();
            return items.len() > 0
                && items.iter().all(|item| {
                    item.node_type != ParseNodeType::Unpack && item.node_type != ParseNodeType::Comprehension
                });
        }
        if let D::Tuple(d) = expr.d() {
            let items = d.items.borrow();
            return items.len() > 0 && items.iter().all(|item| item.node_type != ParseNodeType::Unpack);
        }
        false
    }

    // analyzer/binder.ts:2687
    fn _add_typing_import_aliases_from_builtins_scope(&mut self) {
        let Some(builtins_scope) = self._file_info.builtins_scope else {
            return;
        };

        let symbol_table = builtins_scope.symbol_table;
        symbol_table.for_each(|symbol, name| {
            let typing_import_alias = symbol.get_typing_symbol_alias();
            if let Some(typing_import_alias) = typing_import_alias {
                if !typing_import_alias.is_empty() && !symbol.is_externally_hidden() {
                    self._typing_symbol_aliases.insert(name.to_string(), typing_import_alias);
                }
            }
        });
    }

    // analyzer/binder.ts:2701
    fn _format_module_name(&self, node: ModuleNameNode) -> String {
        ".".repeat(node.d().leading_dots.get() as usize)
            + &node
                .d()
                .name_parts
                .borrow()
                .iter()
                .map(|part| part.d().value())
                .collect::<Vec<_>>()
                .join(".")
    }

    // analyzer/binder.ts:2705
    fn _get_non_class_parent_scope(&self) -> P<Scope> {
        // We may not be able to use the current scope if it's a class scope.
        // Walk up until we find a non-class scope instead.
        let mut parent_scope = self.cur_scope();
        while parent_scope.type_ == ScopeType::Class {
            parent_scope = parent_scope.parent.unwrap();
        }

        parent_scope
    }

    // analyzer/binder.ts:2716
    fn _add_slots_to_current_scope(&mut self, slot_name_nodes: &[StringListNode]) {
        assert(self.cur_scope().type_ == ScopeType::Class, None);

        let mut slots_contains_dict = false;

        for slot_name_node in slot_name_nodes {
            let slot_name = first_string_value(*slot_name_node);

            if slot_name == "__dict__" {
                slots_contains_dict = true;
                continue;
            }

            let mut symbol = self.cur_scope().look_up_symbol(&slot_name);
            if let Some(symbol) = symbol {
                symbol.set_is_slots_member();
            } else {
                let new_symbol = self.cur_scope().add_symbol(
                    &slot_name,
                    SymbolFlags::InitiallyUnbound
                        | SymbolFlags::ClassMember
                        | SymbolFlags::InstanceMember
                        | SymbolFlags::SlotsMember,
                );
                let honor_private_naming =
                    self._file_info.diagnostic_rule_set.report_private_usage != DiagnosticLevel::None;
                if is_private_or_protected_name(&slot_name) && honor_private_naming {
                    new_symbol.set_is_private_member();
                }
                symbol = Some(new_symbol);
            }

            let details = VariableDeclarationDetails::default();
            details.is_constant.set(Some(is_constant_name(&slot_name)));
            details.is_defined_by_slots.set(Some(true));
            details
                .is_explicit_binding
                .set(Some(self.cur_scope().get_binding_type(&slot_name).is_some()));
            let declaration = Declaration::create(
                DeclarationType::Variable,
                slot_name_node.node(),
                self._file_info.file_uri,
                self._convert_range(slot_name_node),
                self._file_info.module_name.clone(),
                self._is_in_except_suite,
                details,
            );
            symbol.unwrap().add_declaration(declaration);
        }

        if !slots_contains_dict {
            self.cur_scope().set_slots_names(
                slot_name_nodes
                    .iter()
                    .map(|node| first_string_value(*node))
                    .collect(),
            );
        }
    }

    // analyzer/binder.ts:2765
    fn _is_in_comprehension(&self, node: P<ParseNode>, ignore_outermost_iterable: bool) -> bool {
        let mut cur_node = Some(node);
        let mut prev_node: Option<P<ParseNode>> = None;
        let mut prev_prev_node: Option<P<ParseNode>> = None;

        while let Some(cur) = cur_node {
            if let D::Comprehension(d) = cur.d() {
                let for_if_nodes = d.for_if_nodes.borrow();
                if ignore_outermost_iterable && for_if_nodes.len() > 0 {
                    let outermost_compr = for_if_nodes[0];
                    if prev_node == Some(outermost_compr) {
                        if let D::ComprehensionFor(f) = outermost_compr.d() {
                            if prev_prev_node == Some(f.iterable_expr.get()) {
                                return false;
                            }
                        }
                    }
                }

                return true;
            }

            prev_prev_node = prev_node;
            prev_node = cur_node;
            cur_node = cur.parent.get();
        }
        false
    }

    // analyzer/binder.ts:2791
    fn _add_pattern_capture_target(&mut self, target: NameNode) {
        let symbol = self._bind_name_to_scope(self.cur_scope(), target, None);
        self._create_assignment_target_flow_nodes(target.node(), /* walkTargets */ false, /* unbound */ false);

        // See if the target overwrites all or a portion of the subject expression.
        if let Some(subj) = self._current_match_subj_expr {
            if ParseTreeUtils::is_matching_expression(target.node(), subj, None)
                || ParseTreeUtils::is_partial_matching_expression(target.node(), subj)
            {
                self._current_match_subj_expr = None;
            }
        }

        if let Some(symbol) = symbol {
            let details = VariableDeclarationDetails::default();
            details.is_constant.set(Some(is_constant_name(target.d().value())));
            details.inferred_type_source.set(target.parent.get());
            details
                .is_explicit_binding
                .set(Some(self.cur_scope().get_binding_type(target.d().value()).is_some()));
            let declaration = Declaration::create(
                DeclarationType::Variable,
                target.node(),
                self._file_info.file_uri,
                self._convert_range(&target),
                self._file_info.module_name.clone(),
                self._is_in_except_suite,
                details,
            );
            symbol.add_declaration(declaration);
        }
    }

    // analyzer/binder.ts:2821
    fn _use_except_targets(&mut self, targets: Vec<FlowLabel>, callback: impl FnOnce(&mut Binder)) {
        let prev_except_targets = std::mem::replace(&mut self._current_except_targets, targets);
        callback(self);
        self._current_except_targets = prev_except_targets;
    }

    // analyzer/binder.ts:2830
    // Attempts to resolve the module name, import it, and return
    // its __all__ symbols.
    fn _get_dunder_all_names_from_import(&self, var_name: &str) -> Option<Vec<String>> {
        let var_symbol = self.cur_scope().look_up_symbol(var_name)?;

        // There should be only one declaration for the variable.
        let alias_decl = var_symbol
            .get_declarations()
            .iter().copied()
            .find(|decl| decl.type_ == DeclarationType::Alias);
        let fallback = alias_decl.and_then(|decl| decl.d().alias().unwrap().submodule_fallback.get());
        let resolved_uri = if let Some(alias_decl) = alias_decl.filter(|decl| {
            !decl.uri.get().is_empty() && decl.d().alias().unwrap().load_symbols_from_path.get()
        }) {
            Some(alias_decl.uri.get())
        } else {
            fallback
                .filter(|fallback| {
                    !fallback.uri.get().is_empty() && fallback.d().alias().unwrap().load_symbols_from_path.get()
                })
                .map(|fallback| fallback.uri.get())
        };
        let resolved_uri = resolved_uri?;

        let lookup_info = (self._file_info.import_lookup)(ImportLookupKey::Uri(resolved_uri), None);
        if let Some(names) = lookup_info.as_ref().and_then(|info| info.dunder_all_names.as_deref()) {
            return Some(names.to_vec());
        }

        if let Some(fallback) = fallback.filter(|fallback| !fallback.uri.get().is_empty()) {
            let lookup_info = (self._file_info.import_lookup)(ImportLookupKey::Uri(fallback.uri.get()), None);
            return lookup_info.and_then(|info| info.dunder_all_names).map(|names| names.to_vec());
        }

        None
    }

    // analyzer/binder.ts:2865
    fn _add_implicit_from_import(&mut self, node: ImportFromNode, import_info: Option<P<ImportResult>>) {
        let first_name_part = node.d().module.get().d().name_parts.borrow()[0];
        let symbol_name = first_name_part.d().value_atom();
        let symbol = self._bind_name_value_to_scope(self.cur_scope(), symbol_name, None);
        if let Some(symbol) = symbol {
            self._create_alias_declaration_for_multipart_import_name(
                node.node(),
                /* importAlias */ None,
                import_info,
                symbol,
            );
        }

        self._create_flow_assignment(first_name_part.node(), false);
    }

    // analyzer/binder.ts:2875
    /// `node` is an `ImportAsNode | ImportFromNode`.
    fn _create_alias_declaration_for_multipart_import_name(
        &mut self,
        node: P<ParseNode>,
        import_alias: Option<NameNode>,
        import_info: Option<P<ImportResult>>,
        symbol: P<Symbol>,
    ) {
        let module = match node.d() {
            D::ImportAs(d) => d.module.get(),
            D::ImportFrom(d) => d.module.get(),
            _ => unreachable!(),
        };
        let name_parts = module.d().name_parts.borrow().clone();
        let first_name_part_value = name_parts[0].d().value().to_string();

        self._node_info.set_flow_node(node, self.cur_flow());

        let is_resolved = import_info
            .is_some_and(|info| info.is_import_found && !info.is_native_lib && !info.resolved_uris.is_empty());

        let mut uri_of_first_submodule: Option<Uri> = None;
        if is_resolved {
            uri_of_first_submodule = Some(import_info.unwrap().resolved_uris[0]);
        }

        // See if there's already a matching alias declaration for this import.
        // if so, we'll update it rather than creating a new one. This is required
        // to handle cases where multiple import statements target the same
        // starting symbol such as "import a.b.c" and "import a.d". In this case,
        // we'll build a single declaration that describes the combined actions
        // of both import statements, thus reflecting the behavior of the
        // python module loader.
        let existing_decl = symbol.get_declarations().iter().copied().find(|decl| {
            decl.type_ == DeclarationType::Alias
                && decl.d().alias().unwrap().first_name_part.borrow().as_deref() == Some(first_name_part_value.as_str())
                && uri_of_first_submodule.is_none_or(|uri| uri.equals(Some(decl.uri.get())))
        });
        let new_decl: P<Declaration>;
        let uri_of_last_submodule: Uri = if is_resolved {
            *import_info.unwrap().resolved_uris.last().unwrap()
        } else {
            unresolved_module_marker()
        };

        // Determine whether this import was declared with the "lazy" keyword (PEP 810).
        let is_lazy = if node.node_type == ParseNodeType::ImportAs {
            node.parent
                .get()
                .is_some_and(|parent| matches!(parent.d(), D::Import(d) if d.is_lazy.get() == Some(true)))
        } else {
            matches!(node.d(), D::ImportFrom(d) if d.is_lazy.get() == Some(true))
        };

        if let Some(existing_decl) = existing_decl {
            new_decl = existing_decl;

            // Reconcile laziness: if any eager import path exists for this symbol,
            // the declaration is not lazy (PEP 810).
            if !is_lazy {
                new_decl.d().alias().unwrap().is_lazy.set(None);
            }
        } else if is_resolved {
            let details = AliasDeclarationDetails::new(import_alias.is_some(), false);
            *details.first_name_part.borrow_mut() = Some(first_name_part_value.clone());
            details.is_lazy.set(if is_lazy { Some(true) } else { None });
            new_decl = Declaration::create(
                DeclarationType::Alias,
                node,
                uri_of_last_submodule,
                get_empty_range(),
                if import_alias.is_some() {
                    self._format_module_name(module)
                } else {
                    ".".repeat(module.d().leading_dots.get() as usize) + &first_name_part_value
                }
                .into(),
                self._is_in_except_suite,
                details,
            );
        } else {
            // If we couldn't resolve the import, create a dummy declaration with a
            // bogus path so it gets an unknown type (rather than an unbound type) at
            // analysis time.
            let details = AliasDeclarationDetails::new(import_alias.is_some(), true);
            *details.first_name_part.borrow_mut() = Some(if import_alias.is_some() {
                self._format_module_name(module)
            } else {
                ".".repeat(module.d().leading_dots.get() as usize) + &first_name_part_value
            });
            details.is_unresolved.set(Some(true));
            details.is_lazy.set(if is_lazy { Some(true) } else { None });
            new_decl = Declaration::create(
                DeclarationType::Alias,
                node,
                uri_of_last_submodule,
                get_empty_range(),
                import_info.map(|info| info.import_name.clone()).unwrap_or_default().into(),
                self._is_in_except_suite,
                details,
            );
        }
        let new_alias = new_decl.d().alias().unwrap();

        // See if there is import info for this part of the path. This allows us
        // to implicitly import all of the modules in a multi-part module name.
        let implicit_import_info = self._node_info.get_import_info(name_parts[0]);
        if let Some(implicit_import_info) = implicit_import_info.filter(|info| !info.resolved_uris.is_empty()) {
            new_decl.uri.set(implicit_import_info.resolved_uris[0]);
            new_alias.load_symbols_from_path.set(true);
            self._add_implicit_imports_to_loader_actions(&implicit_import_info, &new_decl);
        }

        // Add the implicit imports for this module if it's the last
        // name part we're resolving.
        if import_alias.is_some() || name_parts.len() == 1 {
            new_decl.uri.set(uri_of_last_submodule);
            new_alias.load_symbols_from_path.set(true);
            new_alias.is_unresolved.set(Some(false));

            if let Some(import_info) = import_info {
                self._add_implicit_imports_to_loader_actions(&import_info, &new_decl);
            }
        } else {
            // Fill in the remaining name parts.
            let mut cur_loader_actions: &dyn ModuleLoaderActionsLike = &new_decl;

            for i in 1..name_parts.len() {
                let name_part_value = name_parts[i].d().value().to_string();

                // Is there an existing loader action for this name?
                let mut loader_actions = cur_loader_actions
                    .implicit_imports()
                    .get()
                    .and_then(|imports| imports.borrow().get(&name_part_value).copied());
                if loader_actions.is_none() {
                    let loader_action_path = match import_info {
                        Some(info) if i < info.resolved_uris.len() => info.resolved_uris[i],
                        _ => unresolved_module_marker(),
                    };

                    // Allocate a new loader action.
                    let new_loader_actions = ModuleLoaderActions::create(loader_action_path, false);
                    new_loader_actions
                        .implicit_imports
                        .set(Some(create_implicit_import_map()));
                    new_loader_actions.is_unresolved.set(Some(!is_resolved));
                    if cur_loader_actions.implicit_imports().get().is_none() {
                        cur_loader_actions
                            .implicit_imports()
                            .set(Some(create_implicit_import_map()));
                    }
                    cur_loader_actions
                        .implicit_imports()
                        .get()
                        .unwrap()
                        .borrow_mut()
                        .insert(name_part_value.clone(), new_loader_actions);
                    loader_actions = Some(new_loader_actions);
                }
                let loader_actions = loader_actions.unwrap();

                if i == name_parts.len() - 1 {
                    // If this is the last name part we're resolving, add in the
                    // implicit imports as well.
                    if let Some(import_info) = import_info.filter(|info| i < info.resolved_uris.len()) {
                        loader_actions.uri.set(import_info.resolved_uris[i]);
                        loader_actions.load_symbols_from_path.set(true);
                        self._add_implicit_imports_to_loader_actions(&import_info, loader_actions.get());
                    }
                } else {
                    // If this isn't the last name part we're resolving, see if there
                    // is import info for this part of the path. This allows us to implicitly
                    // import all of the modules in a multi-part module name (e.g. "import a.b.c"
                    // imports "a" and "a.b" and "a.b.c").
                    let implicit_import_info = self._node_info.get_import_info(name_parts[i]);
                    if let Some(implicit_import_info) =
                        implicit_import_info.filter(|info| !info.resolved_uris.is_empty())
                    {
                        loader_actions.uri.set(implicit_import_info.resolved_uris[i]);
                        loader_actions.load_symbols_from_path.set(true);
                        self._add_implicit_imports_to_loader_actions(&implicit_import_info, loader_actions.get());
                    }
                }

                cur_loader_actions = loader_actions.get();
            }
        }

        if existing_decl.is_none() {
            symbol.add_declaration(new_decl);
        }
    }

    // analyzer/binder.ts:3045
    fn _walk_statements_and_report_unreachable(&mut self, statements: &[StatementNode]) -> bool {
        let mut found_unreachable_statement = false;

        for statement in statements {
            self._node_info.set_flow_node(*statement, self.cur_flow());

            if !found_unreachable_statement {
                found_unreachable_statement = self._is_code_unreachable();
            }

            if !found_unreachable_statement {
                self.walk(*statement);
            } else {
                // If we're within a function, we need to look for unreachable yield
                // statements because they affect the behavior of the function (making
                // it a generator) even if they're never executed.
                if let Some(target) = self._target_function_declaration {
                    let function = target.d().function().unwrap();
                    if !function.is_generator.get() {
                        let mut yield_finder = YieldFinder::new();
                        if yield_finder.check_contains_yield(*statement) {
                            function.is_generator.set(true);
                        }
                    }
                }

                // In case there are any class or function statements within this
                // subtree, we need to create dummy scopes for them. The type analyzer
                // depends on scopes being present.
                if self._module_symbol_only != Some(true) {
                    let mut dummy_scope_generator = DummyScopeGenerator::new(self.cur_scope(), self._node_info);
                    dummy_scope_generator.walk(*statement);
                }
            }
        }

        false
    }

    // analyzer/binder.ts:3081
    fn _create_start_flow_node(&mut self) -> P<FlowNode> {
        FlowNode::create(FlowFlags::Start, self._get_unique_flow_node_id(), FlowPlain)
    }

    // analyzer/binder.ts:3089
    fn _create_branch_label(&mut self, pre_branch_antecedent: Option<P<FlowNode>>) -> FlowBranchLabel {
        FlowBranchLabel(FlowNode::create(
            FlowFlags::BranchLabel,
            self._get_unique_flow_node_id(),
            FlowBranchLabelDetails {
                label: FlowLabelDetails::new(),
                pre_branch_antecedent,
            },
        ))
    }

    // analyzer/binder.ts:3103
    // Create a flow node that narrows the type of the subject expression for
    // a specified case statement or the entire match statement (if the flow
    // falls through the bottom of all cases).
    /// `statement` is a `CaseNode | MatchNode`.
    fn _create_flow_narrow_for_pattern(&mut self, subject_expression: ExpressionNode, statement: P<ParseNode>) {
        let flow_node = FlowNode::create(
            FlowFlags::NarrowForPattern,
            self._get_unique_flow_node_id(),
            FlowNarrowForPatternDetails {
                subject_expression,
                statement,
                antecedent: self.cur_flow(),
            },
        );

        self._current_flow_node = Some(flow_node);
    }

    // analyzer/binder.ts:3115
    fn _create_context_manager_label(
        &mut self,
        expressions: Vec<ExpressionNode>,
        is_async: bool,
        block_if_swallows_exceptions: bool,
    ) -> FlowPostContextManagerLabel {
        FlowPostContextManagerLabel(FlowNode::create(
            FlowFlags::PostContextManager | FlowFlags::BranchLabel,
            self._get_unique_flow_node_id(),
            FlowPostContextManagerLabelDetails {
                label: FlowLabelDetails::new(),
                expressions,
                is_async,
                block_if_swallows_exceptions,
            },
        ))
    }

    // analyzer/binder.ts:3132
    fn _create_loop_label(&mut self) -> FlowLabel {
        FlowLabel(FlowNode::create(
            FlowFlags::LoopLabel,
            self._get_unique_flow_node_id(),
            FlowLabelDetails::new(),
        ))
    }

    // analyzer/binder.ts:3143
    fn _finish_flow_label(&mut self, node: FlowLabel) -> P<FlowNode> {
        let antecedents_len = node.d().antecedents.borrow().len();

        // If there were no antecedents, this is unreachable.
        if antecedents_len == 0 {
            return unreachable_structural_flow_node();
        }

        // If there was only one antecedent and this is a simple
        // branch label, there's no need for a label to exist.
        if antecedents_len == 1 && node.flags() == FlowFlags::BranchLabel {
            return node.d().antecedents.borrow()[0];
        }

        // The cyclomatic complexity is the number of edges minus the
        // number of nodes in the graph. Add n-1 where n is the number
        // of antecedents (edges) and 1 represents the label node.
        self._code_flow_complexity += (antecedents_len - 1) as f64;

        node.node()
    }

    // analyzer/binder.ts:3166
    // Creates a node that creates a "gate" that is closed (doesn't allow for code
    // flow) if the specified expression is never once it is narrowed (in either the
    // positive or negative case).
    fn _bind_never_condition(&mut self, node: ExpressionNode, target: FlowLabel, is_positive_test: bool) {
        let mut expression_list: Vec<CodeFlowReferenceExpressionNode> = Vec::new();

        let unary_not = match node.d() {
            D::UnaryOperation(d) if d.operator.get() == OperatorType::Not => Some(d.expr.get()),
            _ => None,
        };
        let and_or = match node.d() {
            D::BinaryOperation(d)
                if d.operator.get() == OperatorType::And || d.operator.get() == OperatorType::Or =>
            {
                Some(d)
            }
            _ => None,
        };

        if let Some(operand) = unary_not {
            self._bind_never_condition(operand, target, !is_positive_test);
        } else if let Some(d) = and_or {
            let mut is_and = d.operator.get() == OperatorType::And;
            if is_positive_test {
                is_and = !is_and;
            }

            if is_and {
                // In the And case, we need to gate the synthesized else clause if both
                // of the operands evaluate to never once they are narrowed.
                let saved_current_flow_node = self._current_flow_node;
                self._bind_never_condition(d.left_expr.get(), target, is_positive_test);
                self._current_flow_node = saved_current_flow_node;
                self._bind_never_condition(d.right_expr.get(), target, is_positive_test);
            } else {
                let initial_current_flow_node = self._current_flow_node;

                // In the Or case, we need to gate the synthesized else clause if either
                // of the operands evaluate to never.
                let after_label = self._create_branch_label(None).label();
                self._bind_never_condition(d.left_expr.get(), after_label, is_positive_test);

                // If the condition didn't result in any new flow nodes, we can skip
                // checking the other condition.
                if initial_current_flow_node != self._current_flow_node {
                    self._current_flow_node = Some(self._finish_flow_label(after_label));

                    let prev_current_node = self._current_flow_node;
                    self._bind_never_condition(d.right_expr.get(), target, is_positive_test);

                    // If the second condition resulted in no new control flow node, we can
                    // eliminate this entire subgraph.
                    if prev_current_node == self._current_flow_node {
                        self._current_flow_node = initial_current_flow_node;
                    }
                }
            }
        } else {
            // Limit only to expressions that contain a narrowable subexpression
            // that is a name. This avoids complexities with composite expressions like
            // member access or index expressions.
            if self._is_narrowing_expression(
                node,
                &mut expression_list,
                NarrowExprOptions {
                    filter_for_never_narrowing: Some(true),
                    ..Default::default()
                },
            ) {
                let has_name = expression_list
                    .iter()
                    .any(|expr| expr.node_type == ParseNodeType::Name);
                if has_name {
                    self._current_flow_node = Some(self._create_flow_conditional(
                        if is_positive_test {
                            FlowFlags::TrueNeverCondition
                        } else {
                            FlowFlags::FalseNeverCondition
                        },
                        self.cur_flow(),
                        node,
                    ));
                }
            }

            self._add_antecedent(target, self.cur_flow());
        }
    }

    // analyzer/binder.ts:3229
    fn _bind_conditional(&mut self, node: ExpressionNode, true_target: FlowLabel, false_target: FlowLabel) {
        self._set_true_false_targets(Some(true_target), Some(false_target), move |this| {
            this.walk(node);
        });

        if !self._is_logical_expression(node) {
            let true_condition = self._create_flow_conditional(FlowFlags::TrueCondition, self.cur_flow(), node);
            self._add_antecedent(true_target, true_condition);
            let false_condition = self._create_flow_conditional(FlowFlags::FalseCondition, self.cur_flow(), node);
            self._add_antecedent(false_target, false_condition);
        }
    }

    // analyzer/binder.ts:3246
    fn _disable_true_false_targets(&mut self, callback: impl FnOnce(&mut Binder)) {
        self._set_true_false_targets(/* trueTarget */ None, /* falseTarget */ None, callback);
    }

    // analyzer/binder.ts:3250
    fn _set_true_false_targets(
        &mut self,
        true_target: Option<FlowLabel>,
        false_target: Option<FlowLabel>,
        callback: impl FnOnce(&mut Binder),
    ) {
        let saved_true_target = self._current_true_target;
        let saved_false_target = self._current_false_target;
        self._current_true_target = true_target;
        self._current_false_target = false_target;

        callback(self);

        self._current_true_target = saved_true_target;
        self._current_false_target = saved_false_target;
    }

    // analyzer/binder.ts:3266
    fn _create_flow_conditional(
        &mut self,
        flags: FlowFlags,
        antecedent: P<FlowNode>,
        expression: ExpressionNode,
    ) -> P<FlowNode> {
        if antecedent
            .flags()
            .any(FlowFlags::UnreachableStructural | FlowFlags::UnreachableStaticCondition)
        {
            return antecedent;
        }
        let static_value = StaticExpressions::evaluate_static_bool_like_expression(
            expression,
            &self._file_info.execution_environment,
            &self._file_info.defined_constants,
            Some(&self._typing_import_aliases),
            Some(&self._sys_import_aliases),
        );
        if (static_value == Some(true) && flags.any(FlowFlags::FalseCondition))
            || (static_value == Some(false) && flags.any(FlowFlags::TrueCondition))
        {
            return unreachable_static_condition_flow_node();
        }

        let mut expression_list: Vec<CodeFlowReferenceExpressionNode> = Vec::new();
        if !self._is_narrowing_expression(
            expression,
            &mut expression_list,
            NarrowExprOptions {
                filter_for_never_narrowing: Some(
                    flags.any(FlowFlags::TrueNeverCondition | FlowFlags::FalseNeverCondition),
                ),
                ..Default::default()
            },
        ) {
            return antecedent;
        }

        for expr in &expression_list {
            let reference_key = create_key_for_reference(*expr);
            self.cur_code_flow_expressions().borrow_mut().insert(reference_key);
        }

        // Select the first name expression.
        let filtered_expr_list: Vec<P<ParseNode>> = expression_list
            .iter()
            .copied()
            .filter(|expr| expr.node_type == ParseNodeType::Name)
            .collect();

        let conditional_flow_node = FlowNode::create(
            flags,
            self._get_unique_flow_node_id(),
            FlowConditionDetails {
                reference: filtered_expr_list.first().map(|expr| NameNode(*expr)),
                expression,
                antecedent,
            },
        );

        self._add_except_targets(conditional_flow_node);

        conditional_flow_node
    }

    // analyzer/binder.ts:3315
    // Indicates whether the expression is a NOT, AND or OR expression.
    fn _is_logical_expression(&self, expression: ExpressionNode) -> bool {
        match expression.d() {
            D::UnaryOperation(d) => d.operator.get() == OperatorType::Not,

            D::BinaryOperation(d) => {
                d.operator.get() == OperatorType::And || d.operator.get() == OperatorType::Or
            }

            _ => false,
        }
    }

    // Determines whether the specified expression can be used for conditional
    // type narrowing. The expression atoms (names, member accesses and index)
    // are provided as an output in the expressionList.
    // If filterForNeverNarrowing is true, we limit some types of narrowing
    // expressions for performance reasons.
    // The isComplexExpression parameter is used internally to determine whether
    // the call is an atom (name, member access, index - plus a "not" form of
    // these) or something more complex (binary operator, call, etc.).
    // analyzer/binder.ts:3337
    fn _is_narrowing_expression(
        &self,
        expression: ExpressionNode,
        expression_list: &mut Vec<CodeFlowReferenceExpressionNode>,
        options: NarrowExprOptions,
    ) -> bool {
        let complex = |allow_discriminated: Option<bool>| NarrowExprOptions {
            is_complex_expression: Some(true),
            allow_discriminated_narrowing: allow_discriminated.or(options.allow_discriminated_narrowing),
            ..options
        };

        match expression.d() {
            D::Name(_) | D::MemberAccess(_) | D::Index(_) => {
                if options.filter_for_never_narrowing == Some(true) {
                    // Never narrowing doesn't support member access or index
                    // expressions.
                    if expression.node_type != ParseNodeType::Name {
                        return false;
                    }

                    // Never narrowing doesn't support simple names (falsy
                    // or truthy narrowing) because it's too expensive and
                    // provides relatively little utility.
                    if options.is_complex_expression != Some(true) {
                        return false;
                    }
                }

                if is_code_flow_supported_for_reference(expression) {
                    expression_list.push(expression);

                    if options.filter_for_never_narrowing != Some(true) {
                        // If the expression is a member access expression, add its
                        // leftExpression to the expression list because that expression
                        // can be narrowed based on the attribute type.
                        if let D::MemberAccess(d) = expression.d() {
                            if options.allow_discriminated_narrowing == Some(true) {
                                if is_code_flow_supported_for_reference(d.left_expr.get()) {
                                    expression_list.push(d.left_expr.get());
                                }
                            }
                        }

                        // If the expression is an index expression with a supported
                        // subscript, add its baseExpression to the expression list because
                        // that expression can be narrowed.
                        if let D::Index(d) = expression.d() {
                            let items = d.items.borrow();
                            if items.len() == 1
                                && !d.trailing_comma.get()
                                && items[0].d().arg_category.get() == ArgCategory::Simple
                            {
                                if is_code_flow_supported_for_reference(d.left_expr.get()) {
                                    expression_list.push(d.left_expr.get());
                                }
                            }
                        }
                    }
                    return true;
                }

                false
            }

            D::AssignmentExpression(d) => {
                expression_list.push(d.name.get().node());
                self._is_narrowing_expression(d.right_expr.get(), expression_list, complex(None));
                true
            }

            D::BinaryOperation(d) => {
                let operator = d.operator.get();
                let is_or_is_not_operator = operator == OperatorType::Is || operator == OperatorType::IsNot;
                let equals_or_not_equals_operator =
                    operator == OperatorType::Equals || operator == OperatorType::NotEquals;
                let left_expr = d.left_expr.get();
                let right_expr = d.right_expr.get();

                if is_or_is_not_operator || equals_or_not_equals_operator {
                    // Look for "X is None", "X is not None", "X == None", "X != None".
                    // These are commonly-used patterns used in control flow.
                    if matches!(right_expr.d(), D::Constant(c) if c.const_type.get() == KeywordType::None) {
                        return self._is_narrowing_expression(left_expr, expression_list, complex(Some(true)));
                    }

                    // Look for "type(X) is Y" or "type(X) is not Y".
                    if is_or_is_not_operator {
                        if let D::Call(call) = left_expr.d() {
                            let args = call.args.borrow().clone();
                            if name_value(call.left_expr.get()) == Some("type")
                                && args.len() == 1
                                && args[0].d().arg_category.get() == ArgCategory::Simple
                            {
                                return self._is_narrowing_expression(
                                    args[0].d().value_expr.get(),
                                    expression_list,
                                    complex(None),
                                );
                            }
                        }
                    }

                    let is_left_narrowing =
                        self._is_narrowing_expression(left_expr, expression_list, complex(Some(true)));

                    // Look for "X is Y" or "X is not Y".
                    // Look for X == <literal> or X != <literal>
                    // Look for len(X) == <literal> or len(X) != <literal>
                    return is_left_narrowing;
                }

                // Look for len(X) < <literal>, len(X) <= <literal>, len(X) > <literal>, len(X) >= <literal>.
                if matches!(right_expr.d(), D::Number(n) if n.is_integer.get()) {
                    if operator == OperatorType::LessThan
                        || operator == OperatorType::LessThanOrEqual
                        || operator == OperatorType::GreaterThan
                        || operator == OperatorType::GreaterThanOrEqual
                    {
                        let is_left_narrowing =
                            self._is_narrowing_expression(left_expr, expression_list, complex(None));

                        return is_left_narrowing;
                    }
                }

                // Look for "<string> in Y" or "<string> not in Y".
                if operator == OperatorType::In || operator == OperatorType::NotIn {
                    if left_expr.node_type == ParseNodeType::StringList
                        && self._is_narrowing_expression(right_expr, expression_list, complex(None))
                    {
                        return true;
                    }
                }

                // Look for "X in Y" or "X not in Y".
                if operator == OperatorType::In || operator == OperatorType::NotIn {
                    let is_left_narrowable = self._is_narrowing_expression(left_expr, expression_list, complex(None));

                    let is_right_narrowable =
                        self._is_narrowing_expression(right_expr, expression_list, complex(None));

                    return is_left_narrowable || is_right_narrowable;
                }

                false
            }

            D::UnaryOperation(d) => {
                d.operator.get() == OperatorType::Not
                    && self._is_narrowing_expression(
                        d.expr.get(),
                        expression_list,
                        NarrowExprOptions {
                            is_complex_expression: Some(false),
                            ..options
                        },
                    )
            }

            D::AugmentedAssignment(d) => {
                self._is_narrowing_expression(d.right_expr.get(), expression_list, complex(None))
            }

            D::Call(d) => {
                let args = d.args.borrow().clone();
                let left_name = name_value(d.left_expr.get());
                if (left_name == Some("isinstance") || left_name == Some("issubclass")) && args.len() == 2 {
                    return self._is_narrowing_expression(args[0].d().value_expr.get(), expression_list, complex(None));
                }

                if left_name == Some("callable") && args.len() == 1 {
                    return self._is_narrowing_expression(args[0].d().value_expr.get(), expression_list, complex(None));
                }

                // Is this potentially a call to a user-defined type guard function?
                if args.len() >= 1 {
                    // Never narrowing doesn't support type guards because they do not
                    // offer negative narrowing.
                    if options.filter_for_never_narrowing == Some(true) {
                        return false;
                    }

                    return self._is_narrowing_expression(args[0].d().value_expr.get(), expression_list, complex(None));
                }

                false
            }

            _ => false,
        }
    }

    // analyzer/binder.ts:3559
    fn _create_assignment_target_flow_nodes(&mut self, target: ExpressionNode, walk_targets: bool, unbound: bool) {
        match target.d() {
            D::Name(_) | D::MemberAccess(_) => {
                self._create_flow_assignment(target, unbound);
                if walk_targets {
                    self.walk(target);
                }
            }

            D::Index(_) => {
                self._create_flow_assignment(target, unbound);
                if walk_targets {
                    self.walk(target);
                }
            }

            D::Tuple(d) => {
                let items = d.items.borrow().clone();
                for expr in items {
                    self._create_assignment_target_flow_nodes(expr, walk_targets, unbound);
                }
            }

            D::TypeAnnotation(d) => {
                self._create_assignment_target_flow_nodes(d.value_expr.get(), /* walkTargets */ false, unbound);
                if walk_targets {
                    self.walk(target);
                }
            }

            D::Unpack(d) => {
                self._create_assignment_target_flow_nodes(d.expr.get(), /* walkTargets */ false, unbound);
                if walk_targets {
                    self.walk(target);
                }
            }

            D::List(d) => {
                let items = d.items.borrow().clone();
                for entry in items {
                    self._create_assignment_target_flow_nodes(entry, walk_targets, unbound);
                }
            }

            _ => {
                if walk_targets {
                    self.walk(target);
                }
            }
        }
    }

    // analyzer/binder.ts:3616
    fn _create_call_flow_node(&mut self, node: CallNode) {
        if !self._is_code_unreachable() {
            self._add_except_targets(self.cur_flow());

            let flow_node = FlowNode::create(
                FlowFlags::Call,
                self._get_unique_flow_node_id(),
                FlowCallDetails {
                    node,
                    antecedent: self.cur_flow(),
                },
            );

            self._current_flow_node = Some(flow_node);
        }
    }

    // analyzer/binder.ts:3631
    fn _create_variable_annotation_flow_node(&mut self) {
        if !self._is_code_unreachable() {
            let flow_node = FlowNode::create(
                FlowFlags::VariableAnnotation,
                self._get_unique_flow_node_id(),
                FlowVariableAnnotationDetails {
                    antecedent: self.cur_flow(),
                },
            );

            self._current_flow_node = Some(flow_node);
        }
    }

    // analyzer/binder.ts:3643
    fn _create_flow_assignment(&mut self, node: CodeFlowReferenceExpressionNode, unbound: bool) {
        let mut target_symbol_id = INDETERMINATE_SYMBOL_ID;
        if let D::Name(name) = node.d() {
            let symbol_with_scope = self.cur_scope().look_up_symbol_recursive(name.value_atom(), None);
            assert(symbol_with_scope.is_some(), None);
            target_symbol_id = symbol_with_scope.unwrap().symbol.id;
        }

        let prev_flow_node = self.cur_flow();
        if !self._is_code_unreachable() && is_code_flow_supported_for_reference(node) {
            let mut flags = FlowFlags::Assignment;
            let id = self._get_unique_flow_node_id();

            let reference_key = create_key_for_reference(node);
            self.cur_code_flow_expressions().borrow_mut().insert(reference_key);

            if unbound {
                flags |= FlowFlags::Unbind;
            }

            let flow_node = FlowNode::create(
                flags,
                id,
                FlowAssignmentDetails {
                    node,
                    antecedent: self.cur_flow(),
                    target_symbol_id,
                },
            );

            // Assume that an assignment to a member access expression
            // can potentially generate an exception.
            if node.node_type == ParseNodeType::MemberAccess {
                self._add_except_targets(flow_node);
            }
            self._current_flow_node = Some(flow_node);
        }

        // If we're marking the node as unbound and there is already a flow node
        // associated with the node, don't replace it. This case applies for symbols
        // introduced in except clauses. If there is no use the previous flow node
        // associated, use the previous flow node (applies in the del case).
        // Otherwise, the node will be evaluated as unbound at this point in the flow.
        if !unbound || self._node_info.get_flow_node(node).is_none() {
            self._node_info
                .set_flow_node(node, if unbound { prev_flow_node } else { self.cur_flow() });
        }
    }

    // analyzer/binder.ts:3686
    fn _create_flow_wildcard_import(&mut self, node: ImportFromNode, names: Vec<String>) {
        if !self._is_code_unreachable() {
            let flow_node = FlowNode::create(
                FlowFlags::WildcardImport,
                self._get_unique_flow_node_id(),
                FlowWildcardImportDetails {
                    node,
                    names,
                    antecedent: self.cur_flow(),
                },
            );

            self._add_except_targets(flow_node);
            self._current_flow_node = Some(flow_node);
        }

        self._node_info.set_flow_node(node, self.cur_flow());
    }

    // analyzer/binder.ts:3703
    fn _create_flow_exhausted_match(&mut self, node: MatchNode) {
        if !self._is_code_unreachable() {
            let flow_node = FlowNode::create(
                FlowFlags::ExhaustedMatch,
                self._get_unique_flow_node_id(),
                FlowExhaustedMatchDetails {
                    node,
                    antecedent: self.cur_flow(),
                    subject_expression: node.d().expr.get(),
                },
            );

            self._current_flow_node = Some(flow_node);
        }

        self._node_info.set_after_flow_node(node, self.cur_flow());
    }

    // analyzer/binder.ts:3719
    fn _is_code_unreachable(&self) -> bool {
        self.cur_flow()
            .flags()
            .any(FlowFlags::UnreachableStaticCondition | FlowFlags::UnreachableStructural)
    }

    // analyzer/binder.ts:3726
    fn _add_except_targets(&mut self, flow_node: P<FlowNode>) {
        // If there are any except targets, then we're in a try block, and we
        // have to assume that an exception can be raised after every assignment.
        for label in self._current_except_targets.clone() {
            self._add_antecedent(label, flow_node);
        }
    }

    // analyzer/binder.ts:3736
    fn _track_code_flow_expressions(&mut self, callback: impl FnOnce(&mut Binder)) -> P<RefCell<JsSet<Atom>>> {
        let saved_expressions = self._current_scope_code_flow_expressions;
        self._current_scope_code_flow_expressions = Some(new_string_set());
        callback(self);

        let scoped_expressions = self.cur_code_flow_expressions();

        if let Some(saved_expressions) = saved_expressions {
            let values: Vec<Atom> = scoped_expressions.borrow().iter().copied().collect();
            let mut saved = saved_expressions.borrow_mut();
            for value in values {
                saved.insert(value);
            }
        }

        self._current_scope_code_flow_expressions = saved_expressions;

        scoped_expressions
    }

    // analyzer/binder.ts:3754
    fn _bind_loop_statement(
        &mut self,
        pre_loop_label: FlowLabel,
        post_loop_label: FlowLabel,
        callback: impl FnOnce(&mut Binder),
    ) {
        let saved_continue_target = self._current_continue_target;
        let saved_break_target = self._current_break_target;

        self._current_continue_target = Some(pre_loop_label);
        self._current_break_target = Some(post_loop_label);

        let affected = self._track_code_flow_expressions(callback);
        pre_loop_label.d().affected_expressions.set(Some(affected));

        self._current_continue_target = saved_continue_target;
        self._current_break_target = saved_break_target;
    }

    // analyzer/binder.ts:3767
    fn _add_antecedent(&mut self, label: FlowLabel, antecedent: P<FlowNode>) {
        if !self
            .cur_flow()
            .flags()
            .any(FlowFlags::UnreachableStructural | FlowFlags::UnreachableStaticCondition)
        {
            // Don't add the same antecedent twice.
            let mut antecedents = label.d().antecedents.borrow_mut();
            if !antecedents.iter().any(|existing| existing.id == antecedent.id) {
                antecedents.push(antecedent);
            }
        }
    }

    // analyzer/binder.ts:3778
    fn _bind_name_to_scope(
        &mut self,
        scope: P<Scope>,
        node: NameNode,
        added_symbols: Option<&mut JsMap<String, P<Symbol>>>,
    ) -> Option<P<Symbol>> {
        self._bind_name_value_to_scope(scope, node.d().value_atom(), added_symbols)
    }

    // analyzer/binder.ts:3782
    fn _bind_name_value_to_scope(
        &mut self,
        scope: P<Scope>,
        name: Atom,
        added_symbols: Option<&mut JsMap<String, P<Symbol>>>,
    ) -> Option<P<Symbol>> {
        // Is this name already bound to a scope other than the local one?
        let binding_type = self.cur_scope().get_binding_type(&name);

        if let Some(binding_type) = binding_type {
            let scope_to_use = if binding_type == NameBindingType::Nonlocal {
                self.cur_scope().parent.unwrap()
            } else {
                self.cur_scope().get_global_scope().scope
            };
            let symbol_with_scope = scope_to_use.look_up_symbol_recursive(name, None);
            if let Some(symbol_with_scope) = symbol_with_scope {
                return Some(symbol_with_scope.symbol);
            }
        } else {
            // Don't overwrite an existing symbol.
            let mut symbol = scope.look_up_symbol(name);
            if symbol.is_none() {
                let new_symbol = scope.add_symbol(name, SymbolFlags::InitiallyUnbound | SymbolFlags::ClassMember);

                let current_type = self.cur_scope().type_;
                if current_type == ScopeType::Module || current_type == ScopeType::Builtin {
                    if is_private_or_protected_name(&name) {
                        if is_private_name(&name) {
                            // Private names within classes are mangled, so they are always externally hidden.
                            if scope.type_ == ScopeType::Class {
                                new_symbol.set_is_externally_hidden();
                            } else {
                                self._potential_private_symbols.insert(name.to_string(), new_symbol);
                            }
                        } else if current_type == ScopeType::Builtin {
                            // Don't include private-named symbols in the builtin scope.
                            new_symbol.set_is_externally_hidden();
                        } else {
                            // Defer the private/protected decision until __all__ is processed
                            // so an explicit __all__ entry can promote the symbol to public.
                            self._potential_private_symbols.insert(name.to_string(), new_symbol);
                        }
                    }
                }

                if let Some(added_symbols) = added_symbols {
                    added_symbols.insert(name.to_string(), new_symbol);
                }
                symbol = Some(new_symbol);
            }

            return symbol;
        }

        None
    }

    // analyzer/binder.ts:3832
    fn _bind_possible_tuple_named_target(
        &mut self,
        target: ExpressionNode,
        mut added_symbols: Option<&mut JsMap<String, P<Symbol>>>,
    ) {
        if self.cur_scope().type_ == ScopeType::Class && !is_static_class_assignment_target(target) {
            self.cur_scope().has_potentially_dynamic_symbol_table.set(true);
        }

        match target.d() {
            D::Name(_) => {
                self._bind_name_to_scope(self.cur_scope(), NameNode(target), added_symbols);
            }

            D::Tuple(d) => {
                let items = d.items.borrow().clone();
                for expr in items {
                    self._bind_possible_tuple_named_target(expr, added_symbols.as_deref_mut());
                }
            }

            D::List(d) => {
                let items = d.items.borrow().clone();
                for expr in items {
                    self._bind_possible_tuple_named_target(expr, added_symbols.as_deref_mut());
                }
            }

            D::TypeAnnotation(d) => {
                self._bind_possible_tuple_named_target(d.value_expr.get(), added_symbols);
            }

            D::Unpack(d) => {
                self._bind_possible_tuple_named_target(d.expr.get(), added_symbols);
            }

            _ => {}
        }
    }

    // analyzer/binder.ts:3869
    /// `node` is a `ModuleNode | ClassNode | FunctionNode | LambdaNode`.
    fn _add_implicit_symbol_to_current_scope(
        &mut self,
        name_value: &str,
        node: P<ParseNode>,
        type_: IntrinsicType,
        is_class_member: bool,
    ) {
        let symbol = self._add_symbol_to_current_scope(name_value, /* isInitiallyUnbound */ false, is_class_member);
        symbol.add_declaration(Declaration::create(
            DeclarationType::Intrinsic,
            node,
            self._file_info.file_uri,
            get_empty_range(),
            self._file_info.module_name.clone(),
            self._is_in_except_suite,
            IntrinsicDeclarationDetails {
                name: name_value.to_string(),
                intrinsic_type: type_,
            },
        ));
        symbol.set_is_ignored_for_protocol_match();
    }

    // analyzer/binder.ts:3892
    // Adds a new symbol with the specified name if it doesn't already exist.
    fn _add_symbol_to_current_scope(
        &mut self,
        name_value: &str,
        is_initially_unbound: bool,
        is_class_member: bool,
    ) -> P<Symbol> {
        let mut symbol = self.cur_scope().look_up_symbol(name_value);

        if symbol.is_none() {
            let mut symbol_flags = SymbolFlags::None;

            if is_initially_unbound {
                symbol_flags |= SymbolFlags::InitiallyUnbound;
            }

            if self.cur_scope().type_ == ScopeType::Class && is_class_member {
                symbol_flags |= SymbolFlags::ClassMember;
            }

            if self._file_info.is_stub_file && is_private_or_protected_name(name_value) {
                symbol_flags |= SymbolFlags::ExternallyHidden;
            }

            // Add the symbol. Assume that symbols with a default type source ID
            // are "implicit" symbols added to the scope. These are not initially unbound.
            symbol = Some(self.cur_scope().add_symbol(name_value, symbol_flags));
        }

        symbol.unwrap()
    }

    // analyzer/binder.ts:3918
    fn _create_new_scope(
        &mut self,
        scope_type: ScopeType,
        parent_scope: Option<P<Scope>>,
        proxy_scope: Option<P<Scope>>,
        chained_module_level_scope_lookup: Option<ScopeChainedModuleLevelLookup>,
        callback: impl FnOnce(&mut Binder),
    ) -> P<Scope> {
        let prev_scope = self._current_scope;
        let new_scope = Scope::new(scope_type, parent_scope, proxy_scope, chained_module_level_scope_lookup);
        self._current_scope = Some(new_scope);

        // If this scope is an execution scope, allocate a new reference map.
        let is_execution_scope = scope_type == ScopeType::Builtin
            || scope_type == ScopeType::Module
            || scope_type == ScopeType::Function;
        let prev_expressions = self._current_scope_code_flow_expressions;

        if is_execution_scope {
            self._current_scope_code_flow_expressions = Some(new_string_set());
        }

        callback(self);

        self._current_scope_code_flow_expressions = prev_expressions;
        self._current_scope = prev_scope;

        new_scope
    }

    // The chained module-level lookup is installed only on Module scope. This ensures it is
    // consulted exactly once during recursive lookup — after the module's own symbol table
    // but before ascending to builtins — when `useChainedModuleLevelScopes` is set by a nested
    // evaluation context (function body, lambda, class header, or comprehension inside a
    // function). Non-module scopes pass `undefined` so they never trigger a redundant search.
    // analyzer/binder.ts:3951
    fn _create_cell_chain_module_level_lookup(&self) -> Option<ScopeChainedModuleLevelLookup> {
        let cell_chain_index = self._cell_chain_index?;
        let file_uri = self._file_info.file_uri;
        let node_info = self._node_info;
        // The callback preserves the caller's beyond-execution-scope
        // state so that hits from later cells are correctly marked.
        Some(std::rc::Rc::new(
            move |name: &str, context: Option<ChainedModuleLevelLookupContext>| -> Option<SymbolWithScope> {
                let context = context.unwrap_or_default();
                for module_node in cell_chain_index.get_later_module_nodes(file_uri).unwrap_or_default() {
                    let Some(module_scope) = node_info.get_scope(module_node) else {
                        continue;
                    };

                    let Some(symbol) = module_scope.look_up_symbol(name) else {
                        continue;
                    };

                    if context.is_outside_caller_module == Some(true) && symbol.is_externally_hidden() {
                        continue;
                    }

                    // Skip symbols whose only declarations are attribute assignments
                    // (e.g. `self.x = ...`); these are instance-level, not module globals.
                    let decls = symbol.get_declarations();
                    if decls.len() > 0
                        && !decls.iter().any(|decl| {
                            decl.type_ != DeclarationType::Variable
                                || decl
                                    .d().variable()
                                    .is_none_or(|v| v.is_defined_by_member_access.get() != Some(true))
                        })
                    {
                        continue;
                    }

                    return Some(SymbolWithScope {
                        symbol,
                        scope: module_scope,
                        is_outside_caller_module: context.is_outside_caller_module == Some(true),
                        is_beyond_execution_scope: context.is_beyond_execution_scope == Some(true),
                        uses_nonlocal_binding: context.uses_nonlocal_binding == Some(true),
                        uses_global_binding: context.uses_global_binding == Some(true),
                    });
                }

                None
            },
        ))
    }

    // analyzer/binder.ts:4000
    fn _add_inferred_type_assignment_for_variable(
        &mut self,
        target: ExpressionNode,
        source: P<ParseNode>,
        is_possible_type_alias: bool,
    ) {
        match target.d() {
            D::Name(name_details) => {
                let name = NameNode(target);
                let symbol_with_scope = self.cur_scope().look_up_symbol_recursive(name_details.value_atom(), None);
                if let Some(symbol_with_scope) = symbol_with_scope {
                    let details = VariableDeclarationDetails::default();
                    details.is_constant.set(Some(is_constant_name(name_details.value())));
                    details.inferred_type_source.set(Some(source));
                    details
                        .is_inference_allowed_in_py_typed
                        .set(Some(self._is_inference_allowed_in_py_typed(name_details.value())));
                    details
                        .type_alias_name
                        .set(if is_possible_type_alias { Some(name) } else { None });
                    *details.doc_string.borrow_mut() = self._get_variable_doc_string(target);
                    details
                        .is_explicit_binding
                        .set(Some(self.cur_scope().get_binding_type(name_details.value()).is_some()));
                    let declaration = Declaration::create(
                        DeclarationType::Variable,
                        target,
                        self._file_info.file_uri,
                        self._convert_range(&name),
                        self._file_info.module_name.clone(),
                        self._is_in_except_suite,
                        details,
                    );
                    symbol_with_scope.symbol.add_declaration(declaration);
                }
            }

            D::MemberAccess(member_access) => {
                let member_access_info = self._get_member_access_info(MemberAccessNode(target));
                if let Some(member_access_info) = member_access_info {
                    let name = member_access.member.get();

                    let mut symbol = member_access_info.class_scope.look_up_symbol(name.d().value_atom());
                    if symbol.is_none() {
                        let new_symbol = member_access_info
                            .class_scope
                            .add_symbol(name.d().value(), SymbolFlags::InitiallyUnbound);
                        let honor_private_naming =
                            self._file_info.diagnostic_rule_set.report_private_usage != DiagnosticLevel::None;
                        if is_private_or_protected_name(name.d().value()) && honor_private_naming {
                            new_symbol.set_is_private_member();
                        }
                        symbol = Some(new_symbol);
                    }
                    let symbol = symbol.unwrap();

                    if member_access_info.is_instance_member {
                        // If a method (which has a declared type) is being overwritten
                        // by an expression with no declared type, don't mark it as
                        // an instance member because the type evaluator will think
                        // that it doesn't need to perform object binding.
                        if !symbol.is_class_member()
                            || !symbol.get_declarations().iter().any(|decl| {
                                decl.type_ == DeclarationType::Function
                                    && decl.d().function().unwrap().is_method.get()
                            })
                        {
                            symbol.set_is_instance_member();
                        }
                    } else {
                        symbol.set_is_class_member();
                    }

                    let details = VariableDeclarationDetails::default();
                    details.is_constant.set(Some(is_constant_name(name.d().value())));
                    details.inferred_type_source.set(Some(source));
                    details.is_defined_by_member_access.set(Some(true));
                    *details.doc_string.borrow_mut() = self._get_variable_doc_string(target);
                    let declaration = Declaration::create(
                        DeclarationType::Variable,
                        name.node(),
                        self._file_info.file_uri,
                        self._convert_range(&name),
                        self._file_info.module_name.clone(),
                        self._is_in_except_suite,
                        details,
                    );
                    symbol.add_declaration(declaration);
                }
            }

            D::Tuple(d) => {
                let items = d.items.borrow().clone();
                for expr in items {
                    self._add_inferred_type_assignment_for_variable(expr, source, false);
                }
            }

            D::TypeAnnotation(d) => {
                self._add_inferred_type_assignment_for_variable(d.value_expr.get(), source, false);
            }

            D::Unpack(d) => {
                self._add_inferred_type_assignment_for_variable(d.expr.get(), source, false);
            }

            D::List(d) => {
                let items = d.items.borrow().clone();
                for entry in items {
                    self._add_inferred_type_assignment_for_variable(entry, source, false);
                }
            }

            _ => {}
        }
    }

    // analyzer/binder.ts:4103
    fn _is_inference_allowed_in_py_typed(&self, symbol_name: &str) -> bool {
        let exempt_symbols = ["__match_args__", "__slots__", "__all__"];
        exempt_symbols.contains(&symbol_name)
    }

    // analyzer/binder.ts:4108
    fn _add_type_declaration_for_variable(&mut self, target: ExpressionNode, type_annotation: ExpressionNode) {
        let mut declaration_handled = false;

        match target.d() {
            D::Name(name_details) => {
                let name = NameNode(target);
                let symbol_with_scope = self.cur_scope().look_up_symbol_recursive(name_details.value_atom(), None);
                if let Some(symbol_with_scope) = symbol_with_scope {
                    let final_info = self._is_annotation_final(Some(type_annotation));

                    let mut type_annotation_node: Option<ExpressionNode> = Some(type_annotation);
                    if final_info.is_final {
                        if final_info.final_type_node.is_none() {
                            type_annotation_node = None;
                        }
                    }

                    // Is this annotation indicating that the variable is a "ClassVar"?
                    let class_var_info = self._is_annotation_class_var(Some(type_annotation));

                    if class_var_info.is_class_var {
                        if class_var_info.class_var_type_node.is_none() {
                            type_annotation_node = None;
                        }
                    }

                    // PEP 591 indicates that a Final variable initialized within a class
                    // body should also be considered a ClassVar unless it's in a dataclass.
                    // We can't tell at this stage whether it's a dataclass, so we'll simply
                    // record whether it's a Final assigned in a class body.
                    let mut is_final_assigned_in_class_body = false;
                    if final_info.is_final {
                        let containing_class = ParseTreeUtils::get_enclosing_class_or_function(target);
                        if containing_class.is_some_and(|c| c.node_type == ParseNodeType::Class) {
                            // Make sure it's part of an assignment.
                            let parent = target.parent.get();
                            if parent.is_some_and(|p| p.node_type == ParseNodeType::Assignment)
                                || parent
                                    .and_then(|p| p.parent.get())
                                    .is_some_and(|gp| gp.node_type == ParseNodeType::Assignment)
                            {
                                is_final_assigned_in_class_body = true;
                            }
                        }
                    }

                    let details = VariableDeclarationDetails::default();
                    details.is_constant.set(Some(is_constant_name(name_details.value())));
                    details.is_final.set(Some(final_info.is_final));
                    details.type_alias_name.set(Some(name));
                    details.type_annotation_node.set(type_annotation_node);
                    *details.doc_string.borrow_mut() = self._get_variable_doc_string(target);
                    details
                        .is_explicit_binding
                        .set(Some(self.cur_scope().get_binding_type(name_details.value()).is_some()));
                    let declaration = Declaration::create(
                        DeclarationType::Variable,
                        target,
                        self._file_info.file_uri,
                        self._convert_range(&name),
                        self._file_info.module_name.clone(),
                        self._is_in_except_suite,
                        details,
                    );
                    symbol_with_scope.symbol.add_declaration(declaration);

                    if is_final_assigned_in_class_body {
                        symbol_with_scope.symbol.set_is_final_var_in_class_body();
                    }

                    if class_var_info.is_class_var {
                        symbol_with_scope.symbol.set_is_class_var();
                    } else if !is_final_assigned_in_class_body {
                        symbol_with_scope.symbol.set_is_instance_member();
                    }

                    // Look for an 'InitVar' either by itself or wrapped in an 'Annotated'.
                    if let D::Index(index) = type_annotation.d() {
                        let items = index.items.borrow().clone();
                        if self._is_dataclasses_annotation(index.left_expr.get(), "InitVar") {
                            symbol_with_scope.symbol.set_is_init_var();
                        } else if self._is_typing_annotation(index.left_expr.get(), "Annotated") && items.len() > 0 {
                            let item0_expr = items[0].d().value_expr.get();
                            if let D::Index(item0) = item0_expr.d() {
                                if self._is_dataclasses_annotation(item0.left_expr.get(), "InitVar") {
                                    symbol_with_scope.symbol.set_is_init_var();
                                }
                            }
                        }
                    }

                    if self._is_dataclasses_annotation(type_annotation, "KW_ONLY") {
                        symbol_with_scope.symbol.set_is_data_class_keyword_only();
                    } else if let D::Index(index) = type_annotation.d() {
                        let items = index.items.borrow().clone();
                        if self._is_typing_annotation(index.left_expr.get(), "Annotated")
                            && items.len() > 0
                            && self._is_dataclasses_annotation(items[0].d().value_expr.get(), "KW_ONLY")
                        {
                            symbol_with_scope.symbol.set_is_data_class_keyword_only();
                        }
                    }
                }

                declaration_handled = true;
            }

            D::MemberAccess(member_access) => {
                // We need to determine whether this expression is declaring a class or
                // instance variable. This is difficult because python doesn't provide
                // a keyword for accessing "this". Instead, it uses naming conventions
                // of "cls" and "self", but we don't want to rely on these naming
                // conventions here. Instead, we'll apply some heuristics to determine
                // whether the symbol on the LHS is a reference to the current class
                // or an instance of the current class.

                let member_access_info = self._get_member_access_info(MemberAccessNode(target));
                if let Some(member_access_info) = member_access_info {
                    let name = member_access.member.get();

                    let mut symbol = member_access_info.class_scope.look_up_symbol(name.d().value_atom());
                    if symbol.is_none() {
                        let new_symbol = member_access_info
                            .class_scope
                            .add_symbol(name.d().value(), SymbolFlags::InitiallyUnbound);
                        let honor_private_naming =
                            self._file_info.diagnostic_rule_set.report_private_usage != DiagnosticLevel::None;
                        if is_private_or_protected_name(name.d().value()) && honor_private_naming {
                            new_symbol.set_is_private_member();
                        }
                        symbol = Some(new_symbol);
                    }
                    let symbol = symbol.unwrap();

                    if member_access_info.is_instance_member {
                        symbol.set_is_instance_member();
                    } else {
                        symbol.set_is_class_member();
                    }

                    let final_info = self._is_annotation_final(Some(type_annotation));
                    let details = VariableDeclarationDetails::default();
                    details.is_constant.set(Some(is_constant_name(name.d().value())));
                    details.is_defined_by_member_access.set(Some(true));
                    details.is_final.set(Some(final_info.is_final));
                    details
                        .type_annotation_node
                        .set(if final_info.is_final && final_info.final_type_node.is_none() {
                            None
                        } else {
                            Some(type_annotation)
                        });
                    *details.doc_string.borrow_mut() = self._get_variable_doc_string(target);
                    let declaration = Declaration::create(
                        DeclarationType::Variable,
                        name.node(),
                        self._file_info.file_uri,
                        self._convert_range(&name),
                        self._file_info.module_name.clone(),
                        self._is_in_except_suite,
                        details,
                    );
                    symbol.add_declaration(declaration);

                    declaration_handled = true;
                }
            }

            _ => {}
        }

        if !declaration_handled {
            self._add_diagnostic(
                DiagnosticRule::reportInvalidTypeForm,
                LocMessage::annotation_not_supported(),
                &type_annotation.text_range(),
            );
        }
    }

    // analyzer/binder.ts:4275
    // Determines whether the expression refers to a type exported by the typing
    // or typing_extensions modules. We can directly evaluate the types at binding
    // time. We assume here that the code isn't making use of some custom type alias
    // to refer to the typing types.
    fn _is_typing_annotation(&self, type_annotation: ExpressionNode, name: &str) -> bool {
        self._is_known_annotation(
            type_annotation,
            name,
            &self._typing_import_aliases,
            &self._typing_symbol_aliases,
        )
    }

    // analyzer/binder.ts:4279
    fn _is_dataclasses_annotation(&self, type_annotation: ExpressionNode, name: &str) -> bool {
        self._is_known_annotation(
            type_annotation,
            name,
            &self._dataclasses_import_aliases,
            &self._dataclasses_symbol_aliases,
        )
    }

    // analyzer/binder.ts:4280
    fn _is_known_annotation(
        &self,
        type_annotation: ExpressionNode,
        name: &str,
        import_aliases: &[String],
        symbol_aliases: &JsMap<String, String>,
    ) -> bool {
        let mut annotation_node = type_annotation;

        // Is this a quoted annotation?
        if annotation_node.node_type == ParseNodeType::StringList {
            annotation_node = self
                ._node_info
                .get_string_annotation(StringListNode(annotation_node))
                .unwrap_or(annotation_node);
        }

        match annotation_node.d() {
            D::Name(d) => {
                let alias = symbol_aliases.get(d.value());
                if alias.is_some_and(|alias| alias == name) {
                    return true;
                }
            }
            D::MemberAccess(d) => {
                if let Some(base_name) = name_value(d.left_expr.get()) {
                    if d.member.get().d().value() == name {
                        return import_aliases.iter().any(|alias| alias == base_name);
                    }
                }
            }
            _ => {}
        }

        false
    }

    // analyzer/binder.ts:4316
    fn _get_variable_doc_string(&self, node: ExpressionNode) -> Option<String> {
        let doc_node = ParseTreeUtils::get_variable_doc_string_node(node)?;

        // A docstring can consist of multiple joined strings in a single expression.
        let strings = doc_node.d().strings.borrow().clone();
        let value_of = |s: &P<ParseNode>| match s.d() {
            D::String(d) => d.value.to_string_lossy(),
            D::FormatString(d) => d.value.to_string_lossy(),
            _ => unreachable!(),
        };
        if strings.len() == 1 {
            // Common case.
            return Some(value_of(&strings[0]));
        }

        Some(strings.iter().map(value_of).collect::<Vec<_>>().join(""))
    }

    // analyzer/binder.ts:4335
    // Determines if the specified type annotation expression is a "Final".
    // It returns a value indicating whether the expression is a "Final"
    // expression and whether it's a "raw" Final with no type arguments.
    fn _is_annotation_final(&self, type_annotation: Option<ExpressionNode>) -> FinalInfo {
        let mut is_final = false;
        let mut final_type_node: Option<ExpressionNode> = None;

        if let Some(mut type_annotation) = type_annotation {
            // Allow Final to be enclosed in ClassVar. Normally, Final implies
            // ClassVar, but this combination is required in the case of dataclasses.
            let class_var_info = self._is_annotation_class_var(Some(type_annotation));
            if let Some(class_var_type_node) = class_var_info.class_var_type_node {
                type_annotation = class_var_type_node;
            }

            let index = match type_annotation.d() {
                D::Index(d) => Some(d),
                _ => None,
            };
            let items = index.map(|d| d.items.borrow().clone()).unwrap_or_default();

            if self._is_typing_annotation(type_annotation, "Final") {
                is_final = true;
            } else if index.is_some()
                && items.len() > 0
                && self._is_typing_annotation(index.unwrap().left_expr.get(), "Annotated")
            {
                return self._is_annotation_final(Some(items[0].d().value_expr.get()));
            } else if let Some(index) = index.filter(|_| items.len() == 1) {
                // Recursively call to see if the base expression is "Final".
                let final_info = self._is_annotation_final(Some(index.left_expr.get()));
                if final_info.is_final
                    && items[0].d().arg_category.get() == ArgCategory::Simple
                    && items[0].d().name.get().is_none()
                    && !index.trailing_comma.get()
                {
                    is_final = true;
                    final_type_node = Some(items[0].d().value_expr.get());
                }
            }
        }

        FinalInfo {
            is_final,
            final_type_node,
        }
    }

    // analyzer/binder.ts:4376
    // Determines if the specified type annotation expression is a "ClassVar".
    // It returns a value indicating whether the expression is a "ClassVar"
    // expression and whether it's a "raw" ClassVar with no type arguments.
    fn _is_annotation_class_var(&self, type_annotation: Option<ExpressionNode>) -> ClassVarInfo {
        let mut is_class_var = false;
        let mut class_var_type_node: Option<ExpressionNode> = None;

        let mut type_annotation = type_annotation;
        while let Some(mut annotation) = type_annotation {
            // Is this a quoted annotation?
            if annotation.node_type == ParseNodeType::StringList {
                annotation = self
                    ._node_info
                    .get_string_annotation(StringListNode(annotation))
                    .unwrap_or(annotation);
            }

            let index = match annotation.d() {
                D::Index(d) => Some(d),
                _ => None,
            };
            let items = index.map(|d| d.items.borrow().clone()).unwrap_or_default();

            if index.is_some()
                && items.len() > 0
                && self._is_typing_annotation(index.unwrap().left_expr.get(), "Annotated")
            {
                type_annotation = Some(items[0].d().value_expr.get());
            } else if self._is_typing_annotation(annotation, "ClassVar") {
                is_class_var = true;
                break;
            } else if let Some(index) = index.filter(|_| items.len() == 1) {
                // Recursively call to see if the base expression is "ClassVar".
                let final_info = self._is_annotation_class_var(Some(index.left_expr.get()));
                if final_info.is_class_var
                    && items[0].d().arg_category.get() == ArgCategory::Simple
                    && items[0].d().name.get().is_none()
                    && !index.trailing_comma.get()
                {
                    is_class_var = true;
                    class_var_type_node = Some(items[0].d().value_expr.get());
                }
                break;
            } else {
                break;
            }
        }

        ClassVarInfo {
            is_class_var,
            class_var_type_node,
        }
    }

    // analyzer/binder.ts:4419
    // Determines whether a member access expression is referring to a
    // member of a class (either a class or instance member). This will
    // typically take the form "self.x" or "cls.x".
    fn _get_member_access_info(&self, node: MemberAccessNode) -> Option<MemberAccessInfo> {
        // We handle only simple names on the left-hand side of the expression,
        // not calls, nested member accesses, index expressions, etc.
        let left_symbol_name = name_value(node.d().left_expr.get())?;

        // Make sure the expression is within a function (i.e. a method) that's
        // within a class definition.
        let method_node = ParseTreeUtils::get_enclosing_function(node.node())?;

        let class_node = ParseTreeUtils::get_enclosing_class(method_node.node(), /* stopAtFunction */ true)?;

        // Determine whether the left-hand side indicates a class or
        // instance member.
        let mut is_instance_member: bool;

        let params = method_node.d().params.borrow().clone();
        if params.len() < 1 {
            return None;
        }
        let first_param_name = params[0].d().name.get()?;

        let class_name = class_node.d().name.get().d().value().to_string();
        let first_param_name = first_param_name.d().value().to_string();

        if left_symbol_name == class_name {
            is_instance_member = false;
        } else {
            if left_symbol_name != first_param_name {
                return None;
            }

            // To determine whether the first parameter of the method
            // refers to the class or the instance, we need to apply
            // some heuristics.
            let implicit_class_methods = ["__new__", "__init_subclass__", "__class_getitem__"];
            if implicit_class_methods.contains(&method_node.d().name.get().d().value()) {
                // Several methods are special. They act as class methods even
                // though they don't have a @classmethod decorator.
                is_instance_member = false;
            } else {
                // Assume that it's an instance member unless we find
                // a decorator that tells us otherwise.
                is_instance_member = true;
                let decorators = method_node.d().decorators.borrow().clone();
                for decorator in decorators {
                    let mut decorator_name: Option<String> = None;

                    let expr = decorator.d().expr.get();
                    match expr.d() {
                        D::Name(d) => {
                            decorator_name = Some(d.value().to_string());
                        }
                        D::MemberAccess(d) if name_value(d.left_expr.get()) == Some("builtins") => {
                            decorator_name = Some(d.member.get().d().value().to_string());
                        }
                        _ => {}
                    }

                    if decorator_name.as_deref() == Some("staticmethod") {
                        // A static method doesn't have a "self" or "cls" parameter.
                        return None;
                    } else if decorator_name.as_deref() == Some("classmethod") {
                        // A classmethod implies that the first parameter is "cls".
                        is_instance_member = false;
                        break;
                    }
                }
            }
        }

        let class_scope = self._node_info.get_scope(class_node);
        assert(class_scope.is_some(), None);

        Some(MemberAccessInfo {
            class_node,
            method_node,
            class_scope: class_scope.unwrap(),
            is_instance_member,
        })
    }

    // analyzer/binder.ts:4506
    fn _add_implicit_imports_to_loader_actions(
        &mut self,
        import_result: &ImportResult,
        loader_actions: &dyn ModuleLoaderActionsLike,
    ) {
        let Some(filtered_implicit_imports) = &import_result.filtered_implicit_imports else {
            return;
        };
        for implicit_import in filtered_implicit_imports.values() {
            let existing_loader_action = loader_actions
                .implicit_imports()
                .get()
                .and_then(|imports| imports.borrow().get(&implicit_import.name).copied());
            if let Some(existing_loader_action) = existing_loader_action {
                existing_loader_action.uri.set(implicit_import.uri);
                existing_loader_action.load_symbols_from_path.set(true);
            } else {
                if loader_actions.implicit_imports().get().is_none() {
                    loader_actions.implicit_imports().set(Some(create_implicit_import_map()));
                }
                let new_loader_actions = ModuleLoaderActions::create(implicit_import.uri, true);
                new_loader_actions.implicit_imports.set(Some(create_implicit_import_map()));
                loader_actions
                    .implicit_imports()
                    .get()
                    .unwrap()
                    .borrow_mut()
                    .insert(implicit_import.name.clone(), new_loader_actions);
            }
        }
    }

    // analyzer/binder.ts:4527
    fn _add_wildcard_imported_module_alias(
        &mut self,
        node: ImportFromNode,
        local_symbol: P<Symbol>,
        imported_symbol: P<Symbol>,
    ) -> bool {
        let Some(imported_module_alias_decl) =
            self._get_multipart_module_alias_declaration(imported_symbol, None, None)
        else {
            return false;
        };

        // The imported symbol may be both an implicitly-imported submodule and a
        // class/function/variable of the same name (e.g. a package that re-exports
        // a class whose name matches a submodule). In that case the non-module
        // declaration appears later in the declaration list and "wins" when the
        // symbol is resolved. Only treat this wildcard re-export as a pure submodule
        // re-export when the module alias is the symbol's last declaration;
        // otherwise fall through so a normal alias declaration is created that
        // resolves to the winning symbol.
        //
        // We compare against the raw last declaration (not getLastTypedDeclarationForSymbol)
        // on purpose: a module alias is a DeclarationType.Alias, which hasTypeForDeclaration
        // treats as untyped, so it never appears among a symbol's typed declarations.
        // The evaluator resolves alias symbols (like this one, whose declarations are all
        // imports) by declaration order, so the last declaration is the relevant "winner"
        // here. When this guard falls through, the alias created by the caller resolves to
        // the winning (e.g. class) declaration and intentionally has no submoduleFallback:
        // the class shadows the submodule, so submodule member access through the
        // re-exported name is no longer offered. The genuine-submodule case (where the
        // module alias is the last declaration) keeps its module/submodule behavior.
        let imported_decls = imported_symbol.get_declarations();
        if imported_decls.last().copied() != Some(imported_module_alias_decl) {
            return false;
        }

        let module_name = imported_module_alias_decl.module_name();
        let first_name_part = imported_module_alias_decl
            .d().alias()
            .unwrap()
            .first_name_part
            .borrow()
            .clone();
        let existing_module_alias_decl = self._get_multipart_module_alias_declaration(
            local_symbol,
            Some(&module_name),
            first_name_part.as_deref(),
        );

        if let Some(existing_module_alias_decl) = existing_module_alias_decl {
            self._merge_module_loader_actions(&existing_module_alias_decl, &imported_module_alias_decl);
        } else {
            local_symbol.add_declaration(self._clone_multipart_module_alias_declaration(node, imported_module_alias_decl));
        }

        true
    }

    // analyzer/binder.ts:4574
    // Finds the latest alias declaration that represents the root of a multipart import
    // chain, regardless of whether it originated from a direct import or wildcard merge.
    fn _get_multipart_module_alias_declaration(
        &self,
        symbol: P<Symbol>,
        module_name: Option<&str>,
        first_name_part: Option<&str>,
    ) -> Option<P<Declaration>> {
        let declarations = symbol.get_declarations();

        for declaration in declarations.iter().rev() {
            let Some(alias) = declaration.d().alias() else {
                continue;
            };
            if alias.symbol_name.borrow().as_ref().is_some_and(|s| !s.is_empty())
                || alias.first_name_part.borrow().as_ref().is_none_or(|s| s.is_empty())
            {
                continue;
            }

            if let Some(module_name) = module_name {
                if *declaration.module_name() != *module_name {
                    continue;
                }
            }

            if let Some(first_name_part) = first_name_part {
                if alias.first_name_part.borrow().as_deref() != Some(first_name_part) {
                    continue;
                }
            }

            return Some(*declaration);
        }

        None
    }

    // analyzer/binder.ts:4597
    fn _clone_multipart_module_alias_declaration(
        &mut self,
        node: ImportFromNode,
        declaration: P<Declaration>,
    ) -> P<Declaration> {
        let cloned_loader_actions = self._clone_module_loader_actions(&declaration);
        let details = AliasDeclarationDetails::new(false, cloned_loader_actions.load_symbols_from_path.get());
        *details.first_name_part.borrow_mut() = declaration.d().alias().unwrap().first_name_part.borrow().clone();
        details
            .implicit_imports
            .set(cloned_loader_actions.implicit_imports.get());
        let cloned_declaration = Declaration::create(
            DeclarationType::Alias,
            node.node(),
            cloned_loader_actions.uri.get(),
            get_empty_range(),
            declaration.module_name(),
            self._is_in_except_suite,
            details,
        );
        let cloned_alias = cloned_declaration.d().alias().unwrap();

        if cloned_loader_actions.is_unresolved.get() == Some(true) {
            cloned_alias.is_unresolved.set(Some(true));
        }

        if declaration.d().alias().unwrap().is_native_lib.get() == Some(true) {
            cloned_alias.is_native_lib.set(Some(true));
        }

        cloned_declaration
    }

    // analyzer/binder.ts:4626
    fn _clone_module_loader_actions(&self, loader_actions: &dyn ModuleLoaderActionsLike) -> P<ModuleLoaderActions> {
        let cloned_loader_actions =
            ModuleLoaderActions::create(loader_actions.uri().get(), loader_actions.load_symbols_from_path().get());

        if loader_actions.is_unresolved().get() == Some(true) {
            cloned_loader_actions.is_unresolved.set(Some(true));
        }

        if let Some(implicit_imports) = loader_actions.implicit_imports().get() {
            let cloned_map = create_implicit_import_map();
            cloned_loader_actions.implicit_imports.set(Some(cloned_map));
            let entries: Vec<(String, P<ModuleLoaderActions>)> = implicit_imports
                .borrow()
                .iter()
                .map(|(name, actions)| (name.clone(), *actions))
                .collect();
            for (name, implicit_import) in entries {
                let cloned = self._clone_module_loader_actions(implicit_import.get());
                cloned_map.borrow_mut().insert(name, cloned);
            }
        }

        cloned_loader_actions
    }

    // analyzer/binder.ts:4646
    fn _merge_module_loader_actions(
        &self,
        target: &dyn ModuleLoaderActionsLike,
        source: &dyn ModuleLoaderActionsLike,
    ) {
        if !source.uri().get().is_empty()
            && (target.uri().get().is_empty() || !target.load_symbols_from_path().get())
        {
            target.uri().set(source.uri().get());
        }

        if source.load_symbols_from_path().get() {
            target.load_symbols_from_path().set(true);
        }

        if source.is_unresolved().get() != Some(true) {
            target.is_unresolved().set(None);
        }

        let Some(source_imports) = source.implicit_imports().get() else {
            return;
        };
        let entries: Vec<(String, P<ModuleLoaderActions>)> = source_imports
            .borrow()
            .iter()
            .map(|(name, actions)| (name.clone(), *actions))
            .collect();
        for (name, implicit_import) in entries {
            let target_implicit_import = target
                .implicit_imports()
                .get()
                .and_then(|imports| imports.borrow().get(&name).copied());
            let Some(target_implicit_import) = target_implicit_import else {
                if target.implicit_imports().get().is_none() {
                    target.implicit_imports().set(Some(create_implicit_import_map()));
                }

                let target_implicit_import = self._clone_module_loader_actions(implicit_import.get());
                target
                    .implicit_imports()
                    .get()
                    .unwrap()
                    .borrow_mut()
                    .insert(name, target_implicit_import);
                continue;
            };

            self._merge_module_loader_actions(target_implicit_import.get(), implicit_import.get());
        }
    }

    // analyzer/binder.ts:4677
    // Handles some special-case assignment statements that are found
    // within the typings.pyi file.
    /// `node` is an `AssignmentNode | TypeAnnotationNode`.
    fn _handle_typing_stub_assignment_or_annotation(&mut self, node: P<ParseNode>) -> bool {
        if !self._file_info.is_typing_stub_file {
            return false;
        }

        let annotation_node: TypeAnnotationNode;

        if let Some(type_annotation) = TypeAnnotationNode::cast(node) {
            annotation_node = type_annotation;
        } else {
            let left_expr = AssignmentNode::cast(node).unwrap().d().left_expr.get();
            let Some(left) = TypeAnnotationNode::cast(left_expr) else {
                return false;
            };

            annotation_node = left;
        }

        let Some(assigned_name_node) = NameNode::cast(annotation_node.d().value_expr.get()) else {
            return false;
        };

        let special_types = [
            "Tuple",
            "Generic",
            "Protocol",
            "Callable",
            "Type",
            "ClassVar",
            "Final",
            "Literal",
            "TypedDict",
            "Union",
            "Optional",
            "Annotated",
            "TypeAlias",
            "Concatenate",
            "TypeGuard",
            "Unpack",
            "Self",
            "NoReturn",
            "Never",
            "LiteralString",
            "OrderedDict",
            "TypeIs",
        ];

        let assigned_name = assigned_name_node.d().value();

        if !special_types.contains(&assigned_name) {
            return false;
        }

        let special_built_in_class_declaration = Declaration::create(
            DeclarationType::SpecialBuiltInClass,
            annotation_node.node(),
            self._file_info.file_uri,
            self._convert_range(&annotation_node),
            self._file_info.module_name.clone(),
            self._is_in_except_suite,
            NoDeclarationDetails,
        );

        let symbol = self._bind_name_to_scope(self.cur_scope(), assigned_name_node, None);
        if let Some(symbol) = symbol {
            symbol.add_declaration(special_built_in_class_declaration);
        }

        self._node_info.set_declaration(node, special_built_in_class_declaration);
        true
    }

    // analyzer/binder.ts:4748
    fn _defer_binding(&mut self, callback: Box<dyn FnOnce(&mut Binder)>) {
        if self._module_symbol_only == Some(true) {
            return;
        }

        self._deferred_binding_tasks.push(DeferredBindingTask {
            scope: self.cur_scope(),
            code_flow_expressions: self.cur_code_flow_expressions(),
            callback,
        });
    }

    // analyzer/binder.ts:4760
    fn _bind_deferred(&mut self) {
        while !self._deferred_binding_tasks.is_empty() {
            let next_item = self._deferred_binding_tasks.remove(0);

            // Reset the state
            self._current_scope = Some(next_item.scope);
            self._current_scope_code_flow_expressions = Some(next_item.code_flow_expressions);

            (next_item.callback)(self);
        }
    }

    // analyzer/binder.ts:4772
    /// `node` is a `YieldNode | YieldFromNode`.
    fn _bind_yield(&mut self, node: P<ParseNode>) {
        let function_node = ParseTreeUtils::get_enclosing_function(node);

        match function_node {
            None => {
                if ParseTreeUtils::get_enclosing_lambda(node).is_none() {
                    self._add_syntax_error(LocMessage::yield_outside_function(), &node.text_range());
                }
            }
            Some(function_node) => {
                if function_node.d().is_async.get() && node.node_type == ParseNodeType::YieldFrom {
                    // PEP 525 indicates that 'yield from' is not allowed in an
                    // async function.
                    self._add_syntax_error(LocMessage::yield_from_outside_async(), &node.text_range());
                }
            }
        }

        if let Some(decl) = self._target_function_declaration {
            let function = decl.d().function().unwrap();
            function
                .yield_statements
                .borrow_mut()
                .get_or_insert_with(Vec::new)
                .push(node);
            function.is_generator.set(true);
        }

        let expr = match node.d() {
            D::Yield(d) => d.expr.get(),
            D::YieldFrom(d) => Some(d.expr.get()),
            _ => unreachable!(),
        };
        if let Some(expr) = expr {
            self.walk(expr);
        }

        self._node_info.set_flow_node(node, self.cur_flow());
    }

    // analyzer/binder.ts:4800
    fn _get_unique_flow_node_id(&mut self) -> u32 {
        self._code_flow_complexity += FLOW_NODE_COMPLEXITY_CONTRIBUTION;
        get_unique_flow_node_id()
    }

    // analyzer/binder.ts:4805
    fn _add_diagnostic(&self, rule: DiagnosticRule, message: String, text_range: &TextRange) -> Option<P<Diagnostic>> {
        let diag_level = self._file_info.diagnostic_rule_set.get_level(rule).unwrap();

        let diagnostic = match diag_level {
            DiagnosticLevel::Error | DiagnosticLevel::Warning | DiagnosticLevel::Information => Some(
                self._file_info
                    .diagnostic_sink
                    .add_diagnostic_with_text_range(diag_level, message, *text_range),
            ),

            DiagnosticLevel::None => None,
        };

        if let Some(diagnostic) = diagnostic {
            diagnostic.set_rule(rule.as_str());
        }

        diagnostic
    }

    // analyzer/binder.ts:4830
    fn _add_syntax_error(&self, message: String, text_range: &TextRange) -> P<Diagnostic> {
        self._file_info
            .diagnostic_sink
            .add_diagnostic_with_text_range(DiagnosticLevel::Error, message, *text_range)
    }
}

// analyzer/binder.ts:4835
pub struct YieldFinder {
    _contains_yield: bool,
}

impl YieldFinder {
    pub fn new() -> YieldFinder {
        YieldFinder {
            _contains_yield: false,
        }
    }

    // analyzer/binder.ts:4838
    pub fn check_contains_yield(&mut self, node: P<ParseNode>) -> bool {
        self.walk(node);
        self._contains_yield
    }
}

impl ParseTreeVisitor<bool> for YieldFinder {
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/binder.ts:4843
    fn visit_yield(&mut self, _node: YieldNode) -> bool {
        self._contains_yield = true;
        false
    }

    // analyzer/binder.ts:4848
    fn visit_yield_from(&mut self, _node: YieldFromNode) -> bool {
        self._contains_yield = true;
        false
    }
}

impl ParseTreeWalker for YieldFinder {}

// analyzer/binder.ts:4854
pub struct ReturnFinder {
    _contains_return: bool,
}

impl ReturnFinder {
    pub fn new() -> ReturnFinder {
        ReturnFinder {
            _contains_return: false,
        }
    }

    // analyzer/binder.ts:4857
    pub fn check_contains_return(&mut self, node: P<ParseNode>) -> bool {
        self.walk(node);
        self._contains_return
    }
}

impl ParseTreeVisitor<bool> for ReturnFinder {
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/binder.ts:4862
    fn visit_return(&mut self, _node: ReturnNode) -> bool {
        self._contains_return = true;
        false
    }
}

impl ParseTreeWalker for ReturnFinder {}

// Creates dummy scopes for classes or functions within a parse tree.
// This is needed in cases where the parse tree has been determined
// to be unreachable. There are code paths where the type evaluator
// will still evaluate these types, and it depends on the presence
// of a scope.
// analyzer/binder.ts:4873
pub struct DummyScopeGenerator {
    _current_scope: Option<P<Scope>>,
    _node_info: AnalyzerNodeInfoAccessor,
}

impl DummyScopeGenerator {
    // analyzer/binder.ts:4876
    pub fn new(current_scope: P<Scope>, node_info: AnalyzerNodeInfoAccessor) -> DummyScopeGenerator {
        DummyScopeGenerator {
            _current_scope: Some(current_scope),
            _node_info: node_info,
        }
    }

    // analyzer/binder.ts:4908
    fn _create_new_scope(&mut self, scope_type: ScopeType, callback: impl FnOnce(&mut DummyScopeGenerator)) -> P<Scope> {
        let prev_scope = self._current_scope;
        let new_scope = Scope::new(scope_type, self._current_scope, None, None);
        self._current_scope = Some(new_scope);

        callback(self);

        self._current_scope = prev_scope;
        new_scope
    }
}

impl ParseTreeVisitor<bool> for DummyScopeGenerator {
    fn default_visit_result(&self) -> bool {
        true
    }

    // analyzer/binder.ts:4884
    fn visit_class(&mut self, node: ClassNode) -> bool {
        let new_scope = self._create_new_scope(ScopeType::Class, |this| {
            this.walk(node.d().suite.get().node());
        });

        if self._node_info.get_scope(node).is_none() {
            self._node_info.set_scope(node, new_scope);
        }

        false
    }

    // analyzer/binder.ts:4896
    fn visit_function(&mut self, node: FunctionNode) -> bool {
        let new_scope = self._create_new_scope(ScopeType::Function, |this| {
            this.walk(node.d().suite.get().node());
        });

        if self._node_info.get_scope(node).is_none() {
            self._node_info.set_scope(node, new_scope);
        }

        false
    }
}

impl ParseTreeWalker for DummyScopeGenerator {}
