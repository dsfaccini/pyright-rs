//! Port of `packages/pyright-internal/src/analyzer/importStatementUtils.ts`.
//!
//! Only `getDirectoryLeadingDotsPointsTo`, which the import resolver uses,
//! `getWildcardImportNames`, which the binder calls, and `getRelativeModuleName`
//! and `getTopLevelImports`, which the checker calls. The rest serves the
//! language server.

use std::rc::Rc;

use jsrt::{JsMap, JsStr, P};

use super::analyzer_file_info::ImportLookupResult;
use super::analyzer_node_info::{AnalyzerNodeInfoReader, get_import_info};
use super::import_result::ImportResult;
use crate::common::uri::Uri;
use crate::parser::parse_nodes::{ImportAsNode, ImportFromAsNode, ImportFromNode, ImportNode, ModuleNameNode, ModuleNode, ParseNode, ParseNodeType};

// analyzer/importStatementUtils.ts:45
/// `node` is an `ImportNode | ImportFromNode`.
pub struct ImportStatement {
    pub node: P<ParseNode>,
    pub subnode: Option<ImportAsNode>,
    pub import_result: Option<P<ImportResult>>,
    pub resolved_path: Option<Uri>,
    pub module_name: String,
    pub follows_non_import_statement: bool,
}

// analyzer/importStatementUtils.ts:54
/// A statement sits in both `ordered_imports` and `map_by_file_path`, so it is an `Rc`.
pub struct ImportStatements {
    pub ordered_imports: Vec<Rc<ImportStatement>>,
    pub map_by_file_path: JsMap<String, Rc<ImportStatement>>,
    pub implicit_imports: Option<JsMap<String, ImportFromAsNode>>,
}

// analyzer/importStatementUtils.ts:127
/// TS defaults: includeImplicitImports = false.
pub fn get_top_level_imports(
    parse_tree: ModuleNode,
    include_implicit_imports: bool,
    node_info: &dyn AnalyzerNodeInfoReader,
) -> ImportStatements {
    _get_top_level_imports(parse_tree, include_implicit_imports, Some(node_info))
}

// analyzer/importStatementUtils.ts:139
fn _get_top_level_imports(
    parse_tree: ModuleNode,
    include_implicit_imports: bool,
    node_info: Option<&dyn AnalyzerNodeInfoReader>,
) -> ImportStatements {
    let mut local_imports = ImportStatements {
        ordered_imports: Vec::new(),
        map_by_file_path: JsMap::default(),
        implicit_imports: None,
    };

    let mut follows_non_import_statement = false;
    let mut found_first_import_statement = false;

    let statements = parse_tree.d().statements.borrow().clone();
    for statement in statements {
        if statement.node_type == ParseNodeType::StatementList {
            let sub_statements = crate::parser::parse_nodes::StatementListNode(statement).d().statements.borrow().clone();
            for sub_statement in sub_statements {
                if let Some(import_node) = ImportNode::cast(sub_statement) {
                    found_first_import_statement = true;
                    _process_import_node(import_node, &mut local_imports, follows_non_import_statement, node_info);
                    follows_non_import_statement = false;
                } else if let Some(import_from_node) = ImportFromNode::cast(sub_statement) {
                    found_first_import_statement = true;
                    _process_import_from_node(
                        import_from_node,
                        &mut local_imports,
                        follows_non_import_statement,
                        include_implicit_imports,
                        node_info,
                    );
                    follows_non_import_statement = false;
                } else {
                    follows_non_import_statement = found_first_import_statement;
                }
            }
        } else {
            follows_non_import_statement = found_first_import_statement;
        }
    }

    local_imports
}

// analyzer/importStatementUtils.ts:683
fn _process_import_node(
    node: ImportNode,
    local_imports: &mut ImportStatements,
    follows_non_import_statement: bool,
    node_info: Option<&dyn AnalyzerNodeInfoReader>,
) {
    let list = node.d().list.borrow().clone();
    for import_as_node in list {
        let import_result = node_info.and_then(|node_info| get_import_info(import_as_node.d().module.get(), node_info));
        let mut resolved_path: Option<Uri> = None;

        if let Some(import_result) = import_result {
            if import_result.is_import_found {
                resolved_path = import_result.resolved_uris.last().copied();
            }
        }

        let local_import = Rc::new(ImportStatement {
            node: node.0,
            subnode: Some(import_as_node),
            import_result,
            resolved_path,
            module_name: format_module_name(import_as_node.d().module.get()),
            follows_non_import_statement,
        });

        local_imports.ordered_imports.push(local_import.clone());

        // Add it to the map.
        if let Some(resolved_path) = resolved_path.filter(|path| !path.is_empty()) {
            // Don't overwrite existing import or import from statements
            // because we always want to prefer 'import from' over 'import'
            // in the map.
            if !local_imports.map_by_file_path.contains_key(resolved_path.key()) {
                local_imports.map_by_file_path.insert(resolved_path.key().to_string(), local_import);
            }
        }
    }
}

// analyzer/importStatementUtils.ts:720
fn _process_import_from_node(
    node: ImportFromNode,
    local_imports: &mut ImportStatements,
    follows_non_import_statement: bool,
    include_implicit_imports: bool,
    node_info: Option<&dyn AnalyzerNodeInfoReader>,
) {
    let import_result = node_info.and_then(|node_info| get_import_info(node.d().module.get(), node_info));
    let mut resolved_path: Option<Uri> = None;

    if let Some(import_result) = import_result {
        if import_result.is_import_found {
            resolved_path = import_result.resolved_uris.last().copied();
        }
    }

    if include_implicit_imports {
        if let Some(import_result) = import_result {
            let implicit_imports = local_imports.implicit_imports.get_or_insert_with(JsMap::default);

            if let Some(result_implicit_imports) = &import_result.implicit_imports {
                for implicit_import in result_implicit_imports.values() {
                    let import_from_as = node
                        .d()
                        .imports
                        .borrow()
                        .iter()
                        .find(|i| i.d().name.get().d().value == implicit_import.name)
                        .copied();
                    if let Some(import_from_as) = import_from_as {
                        implicit_imports.insert(implicit_import.uri.key().to_string(), import_from_as);
                    }
                }
            }
        }
    }

    let local_import = Rc::new(ImportStatement {
        node: node.0,
        subnode: None,
        import_result,
        resolved_path,
        module_name: format_module_name(node.d().module.get()),
        follows_non_import_statement,
    });

    local_imports.ordered_imports.push(local_import.clone());

    // Add it to the map.
    if let Some(resolved_path) = resolved_path.filter(|path| !path.is_empty()) {
        let prev_entry = local_imports.map_by_file_path.get(resolved_path.key());
        // Overwrite existing import statements because we always want to prefer
        // 'import from' over 'import'. Also, overwrite existing 'import from' if
        // the module name is shorter.
        if prev_entry.is_none_or(|prev_entry| {
            prev_entry.node.node_type == ParseNodeType::Import
                || prev_entry.module_name.js_len() > local_import.module_name.js_len()
        }) {
            local_imports.map_by_file_path.insert(resolved_path.key().to_string(), local_import);
        }
    }
}

// analyzer/importStatementUtils.ts:773
pub fn format_module_name(node: ModuleNameNode) -> String {
    let mut module_name = String::new();
    for _ in 0..node.d().leading_dots.get() {
        module_name += ".";
    }

    module_name += &node.d().name_parts.borrow().iter().map(|part| part.d().value.clone()).collect::<Vec<_>>().join(".");

    module_name
}

// analyzer/importStatementUtils.ts:921
/// TS defaults: ignoreFolderStructure = false.
pub fn get_relative_module_name(
    fs: &dyn crate::common::file_system::ReadOnlyFileSystem,
    source_path: Uri,
    target_path: Uri,
    config_options: &crate::common::config_options::ConfigOptions,
    ignore_folder_structure: bool,
    source_is_file: Option<bool>,
) -> Option<String> {
    let mut src_path = source_path;
    let source_is_file =
        source_is_file.unwrap_or_else(|| crate::common::uri::uri_utils::is_file(fs, source_path, false));
    if source_is_file {
        src_path = source_path.get_directory();
    }

    let mut symbol_name: Option<String> = None;
    let mut dest_path = target_path;
    if config_options.stub_path.is_some_and(|stub_path| dest_path.is_child(stub_path))
        || config_options.typeshed_path.is_some_and(|typeshed_path| dest_path.is_child(typeshed_path))
    {
        // Always use absolute imports for files in these library-like directories.
        return None;
    }
    if source_is_file {
        dest_path = target_path.get_directory();

        let file_name = target_path.strip_all_extensions().file_name();
        if file_name != "__init__" {
            // ex) src: a.py, dest: b.py -> ".b" will be returned.
            symbol_name = Some(file_name.to_string());
        } else if ignore_folder_structure {
            // ex) src: nested1/nested2/__init__.py, dest: nested1/__init__.py -> "...nested1" will be returned
            //     like how it would return for sibling folder.
            //
            // if folder structure is not ignored, ".." will be returned
            symbol_name = Some(dest_path.file_name().to_string());
            dest_path = dest_path.get_directory();
        }
    }

    let relative_paths = src_path.get_relative_path_components(dest_path);

    // This assumes both file paths are under the same importing root.
    // So this doesn't handle paths pointing to 2 different import roots.
    // ex) user file A to library file B
    let mut current_paths = ".".to_string();
    for (i, relative_path) in relative_paths.iter().enumerate() {
        if relative_path == ".." {
            current_paths += ".";
        } else {
            current_paths += relative_path;
        }

        if relative_path != ".." && i != relative_paths.len() - 1 {
            current_paths += ".";
        }
    }

    // `if (symbolName)`: an empty name is falsy.
    if let Some(symbol_name) = symbol_name.filter(|name| !name.is_empty()) {
        current_paths = if current_paths.ends_with('.') {
            current_paths + &symbol_name
        } else {
            current_paths + "." + &symbol_name
        };
    }

    Some(current_paths)
}

// analyzer/importStatementUtils.ts:988
pub fn get_directory_leading_dots_points_to(from_directory: Uri, leading_dots: i32) -> Option<Uri> {
    let mut current_directory = from_directory;
    for _ in 1..leading_dots {
        if current_directory.is_root() {
            return None;
        }

        current_directory = current_directory.get_directory();
    }

    Some(current_directory)
}

// analyzer/importStatementUtils.ts:1040
pub fn get_wildcard_import_names(lookup_info: &ImportLookupResult) -> Vec<String> {
    let mut names_to_import: Vec<String> = Vec::new();

    // If a dunder all symbol is defined, it takes precedence.
    if let Some(dunder_all_names) = &lookup_info.dunder_all_names {
        if !lookup_info.uses_unsupported_dunder_all_form {
            return dunder_all_names.to_vec();
        }

        names_to_import.extend(dunder_all_names.iter().cloned());
    }

    lookup_info.symbol_table.for_each(|symbol, name| {
        if !symbol.is_externally_hidden() && !name.js_starts_with("_", 0) {
            names_to_import.push(name.to_string());
        }
    });

    names_to_import
}
