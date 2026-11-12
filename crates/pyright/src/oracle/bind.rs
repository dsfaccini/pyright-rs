//! The bind stage (`oracle/FORMAT.md`, "bind stage"), plus the tree walk and
//! scope numbering that the flow stage shares.
//!
//! The TypeScript dumper prints any object by its own enumerable fields. Here
//! each binder type builds a `Value` tree with the fields TypeScript would
//! have set; `Value::Object` sorts them and drops unset ones on output.

use jsrt::{FxHashMap, P};

use crate::analyzer::analyzer_node_info::{AnalyzerNodeInfoReader, DunderAllInfo, get_scope};
use crate::analyzer::declaration::{Declaration, DeclarationDetails, ModuleLoaderActions};
use crate::analyzer::import_result::{ImplicitImport, ImportResult};
use crate::analyzer::py_typed_utils::PyTypedInfo;
use crate::analyzer::scope::Scope;
use crate::analyzer::symbol::Symbol;
use crate::common::text_range::Range;
use crate::common::uri::Uri;
use crate::oracle::format::{self, DumpWriter};
use crate::parser::parse_nodes::{ParseNode, ParseNodeArray, get_parser_string_annotation};
use crate::parser::parse_tree_utils::get_child_nodes;

/// A printable value: the shapes `Printer.emit` distinguishes.
pub enum Value {
    /// A scalar or one-line form, already formatted.
    Line(String),
    /// An object; fields print sorted by name, `None` fields are skipped.
    Object(Vec<(&'static str, Option<Value>)>),
    Array(Vec<Value>),
    /// A `Map` with its keys already formatted.
    Map(Vec<(String, Value)>),
    Set(Vec<Value>),
}

impl Value {
    pub fn str(text: &str) -> Value {
        Value::Line(format::str(text))
    }

    pub fn bool(b: bool) -> Value {
        Value::Line(format::boolean(b).to_string())
    }

    pub fn number(n: f64) -> Value {
        Value::Line(format::number(n))
    }

    pub fn node(node: P<ParseNode>) -> Value {
        Value::Line(node_ref(&node))
    }

    pub fn uri(uri: Uri) -> Value {
        Value::Line(uri_form(uri))
    }

    pub fn range(range: Range) -> Value {
        Value::Line(format!(
            "{}:{}-{}:{}",
            range.start.line, range.start.character, range.end.line, range.end.character
        ))
    }

    pub fn emit(&self, w: &mut DumpWriter, depth: usize, key: &str) {
        match self {
            Value::Line(text) => w.field(depth, key, text),
            Value::Object(fields) => {
                let mut fields: Vec<&(&'static str, Option<Value>)> =
                    fields.iter().filter(|(_, value)| value.is_some()).collect();
                if fields.is_empty() {
                    w.field(depth, key, "{}");
                    return;
                }
                fields.sort_by(|a, b| a.0.cmp(b.0));
                w.open(depth, key);
                for (name, value) in fields {
                    value.as_ref().unwrap().emit(w, depth + 1, name);
                }
            }
            Value::Array(items) => {
                w.field(depth, key, &format::array_header(items.len()));
                for (i, item) in items.iter().enumerate() {
                    item.emit(w, depth + 1, &format::index_key(i));
                }
            }
            Value::Map(entries) => {
                w.field(depth, key, &format::map_header(entries.len()));
                for (map_key, value) in entries {
                    value.emit(w, depth + 1, map_key);
                }
            }
            Value::Set(items) => {
                w.field(depth, key, &format!("Set({})", items.len()));
                for (i, item) in items.iter().enumerate() {
                    item.emit(w, depth + 1, &format::index_key(i));
                }
            }
        }
    }
}

/// The parse node type name, as `ParseNodeType` names it.
pub fn node_type_name(node: &ParseNode) -> String {
    format!("{:?}", node.node_type)
}

/// `<NodeTypeName>@<start>+<length>`.
pub fn node_ref(node: &ParseNode) -> String {
    format!(
        "{}@{}+{}",
        node_type_name(node),
        node.start.get(),
        node.length.get()
    )
}

/// `Uri("<text>")`: the mapped path of a file Uri, else `toString()`.
pub fn uri_form(uri: Uri) -> String {
    let text = if uri.scheme() == "file" {
        format::map_path(uri.get_file_path())
    } else {
        uri.to_uri_string()
    };
    format!("Uri({})", format::str(&text))
}

/// Pre-order walk from `root`, children in `getChildNodes` order with the
/// parser's string annotations, skipping `undefined` entries.
pub fn walk_tree(root: P<ParseNode>) -> Vec<P<ParseNode>> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        result.push(node);
        let children = get_child_nodes::<ParseNodeArray>(node, Some(&get_parser_string_annotation));
        for child in children.iter().rev().flatten() {
            stack.push(*child);
        }
    }
    result
}

/// Scopes numbered in walk order.
pub struct ScopeNumbering {
    pub numbers: FxHashMap<P<Scope>, usize>,
    pub scopes: Vec<P<Scope>>,
}

pub fn number_scopes(walk: &[P<ParseNode>], reader: &dyn AnalyzerNodeInfoReader) -> ScopeNumbering {
    let mut numbering = ScopeNumbering {
        numbers: FxHashMap::default(),
        scopes: Vec::new(),
    };
    for node in walk {
        if let Some(scope) = get_scope(*node, reader) {
            if !numbering.numbers.contains_key(&scope) {
                numbering.numbers.insert(scope, numbering.scopes.len());
                numbering.scopes.push(scope);
            }
        }
    }
    numbering
}

fn scope_ref(numbering: &ScopeNumbering, scope: P<Scope>) -> Value {
    Value::Line(match numbering.numbers.get(&scope) {
        Some(index) => format!("#{index}"),
        None => "external".to_string(),
    })
}

fn opt<T>(value: Option<T>, f: impl FnOnce(T) -> Value) -> Option<Value> {
    value.map(f)
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|s| Value::str(s)).collect())
}

fn nodes<T: Copy + Into<P<ParseNode>>>(values: &[T]) -> Value {
    Value::Array(values.iter().map(|n| Value::node((*n).into())).collect())
}

fn py_typed_info_value(info: &PyTypedInfo) -> Value {
    Value::Object(vec![
        ("isPartiallyTyped", Some(Value::bool(info.is_partially_typed))),
        ("pyTypedPath", Some(Value::uri(info.py_typed_path))),
    ])
}

fn implicit_import_value(import: &ImplicitImport) -> Value {
    Value::Object(vec![
        ("isNativeLib", Some(Value::bool(import.is_native_lib))),
        ("isStubFile", Some(Value::bool(import.is_stub_file))),
        ("name", Some(Value::str(&import.name))),
        ("pyTypedInfo", opt(import.py_typed_info.as_ref(), py_typed_info_value)),
        ("uri", Some(Value::uri(import.uri))),
    ])
}

fn implicit_imports_value(imports: &jsrt::JsMap<String, ImplicitImport>) -> Value {
    Value::Map(
        imports
            .iter()
            .map(|(name, import)| (format::str(name), implicit_import_value(import)))
            .collect(),
    )
}

/// An `ImportResult`, without `importFailureInfo`.
pub fn import_result_value(result: &ImportResult) -> Value {
    Value::Object(vec![
        (
            "filteredImplicitImports",
            opt(result.filtered_implicit_imports.as_deref(), implicit_imports_value),
        ),
        ("implicitImports", opt(result.implicit_imports.as_deref(), implicit_imports_value)),
        ("importName", Some(Value::str(&result.import_name))),
        ("importType", Some(Value::number(result.import_type as i32 as f64))),
        ("isImportFound", Some(Value::bool(result.is_import_found))),
        ("isInitFilePresent", Some(Value::bool(result.is_init_file_present))),
        ("isLocalTypingsFile", opt(result.is_local_typings_file, Value::bool)),
        ("isNamespacePackage", Some(Value::bool(result.is_namespace_package))),
        ("isNativeLib", Some(Value::bool(result.is_native_lib))),
        ("isPartlyResolved", Some(Value::bool(result.is_partly_resolved))),
        ("isRelative", Some(Value::bool(result.is_relative))),
        ("isStdlibTypeshedFile", opt(result.is_stdlib_typeshed_file, Value::bool)),
        ("isStubFile", Some(Value::bool(result.is_stub_file))),
        ("isStubPackage", Some(Value::bool(result.is_stub_package))),
        (
            "isThirdPartyTypeshedFile",
            opt(result.is_third_party_typeshed_file, Value::bool),
        ),
        (
            "nonStubImportResult",
            opt(result.non_stub_import_result, |r| import_result_value(&r)),
        ),
        ("packageDirectory", opt(result.package_directory, Value::uri)),
        ("pyTypedInfo", opt(result.py_typed_info.as_ref(), py_typed_info_value)),
        (
            "resolvedUris",
            Some(Value::Array(result.resolved_uris.iter().map(|u| Value::uri(*u)).collect())),
        ),
        ("searchPath", opt(result.search_path, Value::uri)),
    ])
}

fn module_loader_actions_value(actions: &ModuleLoaderActions) -> Value {
    Value::Object(vec![
        (
            "implicitImports",
            opt(actions.implicit_imports.get(), |map| {
                Value::Map(
                    map.borrow()
                        .iter()
                        .map(|(name, a)| (format::str(name), module_loader_actions_value(a)))
                        .collect(),
                )
            }),
        ),
        ("isUnresolved", opt(actions.is_unresolved.get(), Value::bool)),
        ("loadSymbolsFromPath", Some(Value::bool(actions.load_symbols_from_path.get()))),
        ("uri", Some(Value::uri(actions.uri.get()))),
    ])
}

/// A declaration with the fields its object literal set.
pub fn declaration_value(decl: P<Declaration>) -> Value {
    let mut fields: Vec<(&'static str, Option<Value>)> = vec![
        ("isInExceptSuite", Some(Value::bool(decl.is_in_except_suite.get()))),
        ("isInInlinedTypedDict", opt(decl.is_in_inlined_typed_dict.get(), Value::bool)),
        ("moduleName", Some(Value::str(&decl.module_name()))),
        ("node", Some(Value::node(decl.node.get()))),
        ("range", Some(Value::range(decl.range.get()))),
        ("type", Some(Value::number(decl.type_ as i32 as f64))),
        ("uri", Some(Value::uri(decl.uri.get()))),
    ];

    match decl.d() {
        DeclarationDetails::Intrinsic(d) => {
            fields.push(("intrinsicType", Some(Value::str(d.intrinsic_type))));
            fields.push(("name", Some(Value::str(&d.name))));
        }
        DeclarationDetails::Variable(d) => {
            fields.extend([
                ("alternativeTypeNode", opt(d.alternative_type_node.get(), Value::node)),
                ("docString", opt(d.doc_string.borrow().as_deref(), Value::str)),
                ("inferredTypeSource", opt(d.inferred_type_source.get(), Value::node)),
                ("isConstant", opt(d.is_constant.get(), Value::bool)),
                ("isDefinedByMemberAccess", opt(d.is_defined_by_member_access.get(), Value::bool)),
                ("isDefinedBySlots", opt(d.is_defined_by_slots.get(), Value::bool)),
                ("isExplicitBinding", opt(d.is_explicit_binding.get(), Value::bool)),
                ("isFinal", opt(d.is_final.get(), Value::bool)),
                (
                    "isInferenceAllowedInPyTyped",
                    opt(d.is_inference_allowed_in_py_typed.get(), Value::bool),
                ),
                ("isRuntimeTypeExpression", opt(d.is_runtime_type_expression.get(), Value::bool)),
                ("typeAliasName", opt(d.type_alias_name.get(), |n| Value::node(n.node()))),
                ("typeAnnotationNode", opt(d.type_annotation_node.get(), Value::node)),
            ]);
        }
        DeclarationDetails::Param(d) => {
            fields.push(("inferredName", opt(d.inferred_name.borrow().as_deref(), Value::str)));
            fields.push((
                "inferredTypeNodes",
                opt(d.inferred_type_nodes.borrow().as_deref(), nodes),
            ));
        }
        DeclarationDetails::TypeAlias(d) => {
            fields.push(("docString", opt(d.doc_string.borrow().as_deref(), Value::str)));
        }
        DeclarationDetails::Function(d) => {
            fields.extend([
                ("isGenerator", Some(Value::bool(d.is_generator.get()))),
                ("isMethod", Some(Value::bool(d.is_method.get()))),
                ("raiseStatements", opt(d.raise_statements.borrow().as_deref(), nodes)),
                ("returnStatements", opt(d.return_statements.borrow().as_deref(), nodes)),
                ("yieldStatements", opt(d.yield_statements.borrow().as_deref(), nodes)),
            ]);
        }
        DeclarationDetails::Alias(d) => {
            fields.extend([
                ("firstNamePart", opt(d.first_name_part.borrow().as_deref(), Value::str)),
                (
                    "implicitImports",
                    opt(d.implicit_imports.get(), |map| {
                        Value::Map(
                            map.borrow()
                                .iter()
                                .map(|(name, a)| (format::str(name), module_loader_actions_value(a)))
                                .collect(),
                        )
                    }),
                ),
                ("isLazy", opt(d.is_lazy.get(), Value::bool)),
                ("isNativeLib", opt(d.is_native_lib.get(), Value::bool)),
                ("isUnresolved", opt(d.is_unresolved.get(), Value::bool)),
                ("loadSymbolsFromPath", Some(Value::bool(d.load_symbols_from_path.get()))),
                ("submoduleFallback", opt(d.submodule_fallback.get(), declaration_value)),
                ("symbolName", opt(d.symbol_name.borrow().as_deref(), Value::str)),
                ("usesLocalName", Some(Value::bool(d.uses_local_name.get()))),
            ]);
        }
        DeclarationDetails::TypeParam
        | DeclarationDetails::Class
        | DeclarationDetails::SpecialBuiltInClass => {}
    }

    Value::Object(fields)
}

fn symbol_value(symbol: &Symbol) -> Value {
    Value::Object(vec![
        (
            "_declarations",
            opt(symbol._declarations.borrow().as_ref(), |decls| {
                Value::Array(decls.iter().map(|d| declaration_value(*d)).collect())
            }),
        ),
        ("_flags", Some(Value::number(symbol._flags.get().0 as f64))),
        (
            "_synthesizedTypeInfo",
            opt(*symbol._synthesized_type_info.borrow(), |info| {
                Value::Object(vec![
                    ("node", opt(info.node, |n| Value::node(n.node()))),
                    ("type", Some(Value::Line("<type>".to_string()))),
                ])
            }),
        ),
        ("_typingSymbolAlias", opt(symbol._typing_symbol_alias.borrow().as_deref(), Value::str)),
    ])
}

fn scope_value(scope: &Scope, numbering: &ScopeNumbering) -> Value {
    Value::Object(vec![
        (
            "chainedModuleLevelScopeLookup",
            scope
                .chained_module_level_scope_lookup
                .as_ref()
                .map(|_| Value::Line("<function>".to_string())),
        ),
        ("hasNonEmptySlots", Some(Value::bool(scope.has_non_empty_slots.get()))),
        (
            "hasPotentiallyDynamicSymbolTable",
            Some(Value::bool(scope.has_potentially_dynamic_symbol_table.get())),
        ),
        (
            "notLocalBindings",
            Some(Value::Map(
                scope
                    .not_local_bindings
                    .borrow()
                    .iter()
                    .map(|(name, binding)| (format::str(name), Value::number(*binding as i32 as f64)))
                    .collect(),
            )),
        ),
        ("parent", opt(scope.parent, |p| scope_ref(numbering, p))),
        ("proxy", opt(scope.proxy, |p| scope_ref(numbering, p))),
        ("slotsNames", opt(scope.slots_names.borrow().as_deref(), strings)),
        (
            "symbolTable",
            Some(Value::Map(
                scope
                    .symbol_table
                    .entries()
                    .iter()
                    .map(|(name, symbol)| (format::str(name), symbol_value(symbol)))
                    .collect(),
            )),
        ),
        ("type", Some(Value::number(scope.type_ as i32 as f64))),
    ])
}

fn dunder_all_info_value(info: &DunderAllInfo) -> Value {
    Value::Object(vec![
        ("names", Some(strings(&info.names))),
        ("stringNodes", Some(nodes(&info.string_nodes))),
        (
            "usesUnsupportedDunderAllForm",
            Some(Value::bool(info.uses_unsupported_dunder_all_form)),
        ),
    ])
}

/// The bind dump of one file.
pub fn dump_bind(
    walk: &[P<ParseNode>],
    numbering: &ScopeNumbering,
    reader: &dyn AnalyzerNodeInfoReader,
) -> String {
    let mut w = DumpWriter::new();

    w.open(0, "nodes");
    for node in walk {
        let Some(info) = reader.get(*node) else {
            continue;
        };
        let cold = info.cold();
        let fields = Value::Object(vec![
            ("codeFlowComplexity", opt(cold.and_then(|c| c.code_flow_complexity.get()), Value::number)),
            (
                "codeFlowExpressions",
                opt(cold.and_then(|c| c.code_flow_expressions.get()), |set| {
                    Value::Set(set.borrow().iter().map(|s| Value::str(s)).collect())
                }),
            ),
            ("declaration", opt(cold.and_then(|c| c.declaration.get()), declaration_value)),
            ("dunderAllInfo", opt(cold.and_then(|c| c.dunder_all_info.get()), |d| dunder_all_info_value(&d))),
            ("importInfo", opt(cold.and_then(|c| c.import_info.get()), |r| import_result_value(&r))),
            ("scope", opt(cold.and_then(|c| c.scope.get()), |s| scope_ref(numbering, s))),
            ("staticConditionValue", opt(cold.and_then(|c| c.static_condition_value.get()), Value::bool)),
        ]);
        let Value::Object(fields) = fields else {
            unreachable!()
        };
        if fields.iter().all(|(_, value)| value.is_none()) {
            continue;
        }
        Value::Object(fields).emit(&mut w, 1, &node_ref(node));
    }

    w.field(0, "scopes", &format::array_header(numbering.scopes.len()));
    for (i, scope) in numbering.scopes.iter().enumerate() {
        scope_value(scope, numbering).emit(&mut w, 1, &format::index_key(i));
    }

    w.finish()
}
