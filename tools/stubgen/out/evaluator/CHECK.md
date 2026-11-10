# compile check: evaluator

cargo check -p pyright: pass (0 error lines, 0 warning lines)
files placed: 9; ported crate files used instead of placeholders: analyzer/scope.rs, analyzer/code_flow_types.rs, analyzer/analyzer_file_info.rs, analyzer/declaration.rs, analyzer/py_typed_utils.rs, analyzer/declaration_utils.rs, analyzer/analyzer_node_info.rs, analyzer/source_file.rs, analyzer/symbol.rs, analyzer/import_result.rs, common/diagnostic.rs, common/python_version.rs, common/config_options.rs, common/console.rs, common/uri/uri.rs, common/text_range_collection.rs, common/diagnostic_sink.rs, common/text_range.rs, common/diagnostic_rules.rs, parser/parse_nodes.rs, parser/tokenizer_types.rs
stubs not placed because the crate file is already ported: none
items missing from ported files, served from placeholders (needed from their owners): analyzer/declaration_utils.rs: ResolvedAliasInfo
jsrt items shimmed (needed from the jsrt owner): flag_ops
jsrt items with no shim: none

