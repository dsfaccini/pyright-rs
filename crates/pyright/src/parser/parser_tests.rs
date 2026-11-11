//! Port of `packages/pyright-internal/src/tests/parser.test.ts`.
//!
//! Not ported: `LazyImport - AliasDeclaration.isLazy is propagated` and
//! `LazyImport - submoduleFallback AliasDeclaration carries isLazy` test the
//! binder's declarations. The fourslash tests (`ModuleName range`, `Inline
//! TypedDict dict key ...`) parse their file directly and locate the markers by
//! offset.

use std::path::PathBuf;

use jsrt::{JsStr, JsString, P};

use super::{ParseFileResults, ParseOptions, Parser};
use crate::common::diagnostic_sink::DiagnosticSink;
use crate::common::python_version::{PYTHON_VERSION3_13, PYTHON_VERSION3_14, PYTHON_VERSION3_15};
use crate::common::string_utils::{StringFingerprint, get_string_fingerprint};
use crate::common::text_range::TextRange;
use crate::localization::localize::LocMessage;
use crate::parser::parse_nodes::{
    AugmentedAssignmentNode, ImportFromNode, ImportNode, NameNode, ParseNode, ParseNodeType,
    StatementListNode, StringListNode, get_parser_string_annotation,
    get_parser_string_annotation_info,
};
use crate::parser::parse_tree_utils::get_child_nodes;

/// Runs a test body on a thread with a large stack: the parser recurses once
/// per nesting level, and the max-parse-depth samples nest deeply.
fn big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// `TestUtils.parseText`.
fn parse_text(
    text: &str,
    diag_sink: P<DiagnosticSink>,
    parse_options: ParseOptions,
) -> ParseFileResults {
    let text: Vec<u16> = text.encode_utf16().collect();
    Parser::new().parse_source_file(JsString::from(text), parse_options, diag_sink, None)
}

/// `TestUtils.readSampleFile`: the spec checkout's `tests/samples`, read as Node's `utf8` does.
fn read_sample_file(file_name: &str) -> String {
    let root = std::env::var_os("PYRIGHT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("pyright"));
    let path = root
        .join("packages/pyright-internal/src/tests/samples")
        .join(file_name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `TestUtils.parseSampleFile`; `ParseOptions::new()` has the default execution
/// environment's Python version.
fn parse_sample_file(file_name: &str, diag_sink: P<DiagnosticSink>) -> ParseFileResults {
    parse_sample_file_with_version(file_name, diag_sink, ParseOptions::new().python_version)
}

fn parse_sample_file_with_version(
    file_name: &str,
    diag_sink: P<DiagnosticSink>,
    python_version: crate::common::python_version::PythonVersion,
) -> ParseFileResults {
    let text = read_sample_file(file_name);
    let mut parse_options = ParseOptions::new();
    if file_name.ends_with("pyi") {
        parse_options.is_stub_file = true;
    }
    parse_options.python_version = python_version;
    parse_text(&text, diag_sink, parse_options)
}

fn new_sink() -> P<DiagnosticSink> {
    P::new(DiagnosticSink::new(None))
}

/// A linear `findNodeByOffset` (`analyzer/parseTreeUtils.ts`) with the parser's
/// string annotations; the analyzer's version adds a binary search that returns
/// the same node.
fn find_node_by_offset(node: P<ParseNode>, offset: i32) -> Option<P<ParseNode>> {
    if !TextRange::overlaps(&node, offset) {
        return None;
    }

    for child in get_child_nodes(node, None).into_iter().flatten() {
        if let Some(containing_child) = find_node_by_offset(child, offset) {
            // For augmented assignments, prefer the dest expression.
            if let Some(augmented) = AugmentedAssignmentNode::cast(node)
                && containing_child == augmented.d().left_expr.get()
            {
                return Some(augmented.d().dest_expr.get());
            }
            return Some(containing_child);
        }
    }

    Some(node)
}

/// `getFirstAncestorOrSelfOfKind` (`analyzer/parseTreeUtils.ts`).
fn get_first_ancestor_or_self_of_kind(
    node: Option<P<ParseNode>>,
    node_type: ParseNodeType,
) -> Option<P<ParseNode>> {
    let mut current = node;
    while let Some(node) = current {
        if node.node_type == node_type {
            return Some(node);
        }
        current = node.parent.get();
    }
    None
}

fn statement(parse_results: &ParseFileResults, index: usize) -> P<ParseNode> {
    let statement_list = StatementListNode::cast(
        parse_results
            .parser_output
            .parse_tree
            .d()
            .statements
            .borrow()[index],
    )
    .unwrap();
    statement_list.d().statements.borrow()[0]
}

fn utf16_index_of(text: &str, needle: &str) -> i32 {
    let text: Vec<u16> = text.encode_utf16().collect();
    let needle: Vec<u16> = needle.encode_utf16().collect();
    text.js_index_of(&needle, 0)
}

/// `assert.deepEqual` on a `StringFingerprint`.
fn fingerprint_parts(fingerprint: &StringFingerprint) -> (i32, i32) {
    (fingerprint.primary, fingerprint.secondary)
}

// tests/parser.test.ts:32
#[test]
fn empty() {
    let diag_sink = new_sink();
    let parser_output = parse_text("", diag_sink, ParseOptions::new()).parser_output;

    assert_eq!(diag_sink.fetch_and_clear().len(), 0);
    assert_eq!(parser_output.parse_tree.d().statements.borrow().len(), 0);
}

// tests/parser.test.ts:40
#[test]
fn parser_uses_a_precomputed_content_fingerprint_when_provided() {
    let fingerprint = get_string_fingerprint("precomputed");
    let expected = fingerprint_parts(&fingerprint);
    let results = Parser::new().parse_source_file(
        JsString::from(vec![]),
        ParseOptions::new(),
        new_sink(),
        Some(fingerprint),
    );
    assert_eq!(fingerprint_parts(&results.content_hash), expected);
}

// tests/parser.test.ts:47
#[test]
#[cfg_attr(miri, ignore = "builds the full Unicode identifier tables: over an hour under Miri")]
fn parser_computes_the_complete_content_fingerprint_for_unicode_text_when_one_is_not_provided() {
    let results = parse_text("π😀\n", new_sink(), ParseOptions::new());

    assert_eq!(
        fingerprint_parts(&results.content_hash),
        (83559239, -893512024)
    );
}

// tests/parser.test.ts:56
#[test]
fn parser1() {
    big_stack(|| {
        let diag_sink = new_sink();
        let parser_output = parse_sample_file("parser1.py", diag_sink).parser_output;

        assert_eq!(diag_sink.fetch_and_clear().len(), 0);
        assert_eq!(parser_output.parse_tree.d().statements.borrow().len(), 4);
    });
}

// tests/parser.test.ts:64
#[test]
fn parser2() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("parser2.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 0);
    });
}

// tests/parser.test.ts:70
#[test]
fn f_string_empty_tuple() {
    big_stack(|| {
        parse_sample_file("fstring6.py", new_sink());
    });
}

// tests/parser.test.ts:77
#[test]
fn suite_expected_colon1() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("suiteExpectedColon1.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 1);
    });
}

// tests/parser.test.ts:83
#[test]
fn suite_expected_colon2() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("suiteExpectedColon2.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 1);
    });
}

// tests/parser.test.ts:89
#[test]
fn suite_expected_colon3() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("suiteExpectedColon3.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 1);
    });
}

// tests/parser.test.ts:95
#[test]
fn expression_wrapped_in_parens() {
    let diag_sink = new_sink();
    let parse_results = parse_text("(str)", diag_sink, ParseOptions::new());
    let parse_tree = parse_results.parser_output.parse_tree;

    assert_eq!(diag_sink.fetch_and_clear().len(), 0);
    assert_eq!(parse_tree.d().statements.borrow().len(), 1);
    assert_eq!(
        parse_tree.d().statements.borrow()[0].node_type,
        ParseNodeType::StatementList
    );

    let statement_list = StatementListNode::cast(parse_tree.d().statements.borrow()[0]).unwrap();
    assert_eq!(statement_list.d().statements.borrow().len(), 1);

    // length of node should include parens
    assert_eq!(
        statement_list.d().statements.borrow()[0].node_type,
        ParseNodeType::Name
    );
    assert_eq!(statement_list.d().statements.borrow()[0].length.get(), 5);
}

// tests/parser.test.ts:111
#[test]
fn max_parse_depth1() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("maxParseDepth1.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 1);
    });
}

// tests/parser.test.ts:117
#[test]
fn max_parse_depth2() {
    big_stack(|| {
        let diag_sink = new_sink();
        parse_sample_file("maxParseDepth2.py", diag_sink);
        assert_eq!(diag_sink.get_errors().len(), 4);
    });
}

// tests/parser.test.ts:123
#[test]
fn module_name_range() {
    // `from [|/*marker*/...|] import A`
    let code = "from ... import A";
    let (pos, end) = (5, 8);

    let parse_results = parse_text(code, new_sink(), ParseOptions::new());
    let node = find_node_by_offset(parse_results.parser_output.parse_tree.0, pos).unwrap();

    assert_eq!(node.start.get(), pos);
    assert_eq!(TextRange::get_end(&*node), end);
}

// tests/parser.test.ts:136
#[test]
fn inline_typed_dict_dict_key_is_not_a_forward_reference_annotation() {
    // An inline TypedDict field-name key must not be parsed into a
    // forward-reference expression even though the dictionary appears inside a type
    // annotation. Suspending type-annotation parsing for the key leaves its StringList
    // without a parser-derived annotation association, while the value remains a type
    // annotation and must still parse its forward reference.
    let code = "from typing import TypedDict\ntd: TypedDict[{\"as_var\": \"int\"}]";
    let parse_results = parse_text(code, new_sink(), ParseOptions::new());
    let parse_tree = parse_results.parser_output.parse_tree.0;

    let key_node = find_node_by_offset(parse_tree, utf16_index_of(code, "as_var"));
    let key_string_list = get_first_ancestor_or_self_of_kind(key_node, ParseNodeType::StringList)
        .expect("Expected the dict key to be a StringList node");
    assert_eq!(
        get_parser_string_annotation(StringListNode(key_string_list)),
        None,
        "Inline TypedDict key string must not be parsed into a forward-reference annotation"
    );

    let value_node = find_node_by_offset(parse_tree, utf16_index_of(code, "int\""));
    let value_string_list =
        get_first_ancestor_or_self_of_kind(value_node, ParseNodeType::StringList)
            .expect("Expected the dict value to be a StringList node");
    assert!(
        get_parser_string_annotation(StringListNode(value_string_list)).is_some(),
        "Inline TypedDict value string is a type annotation and must still parse its forward reference"
    );
}

// tests/parser.test.ts:165
#[test]
fn parser_string_annotation_side_data_preserves_recursive_associations_and_structural_ownership() {
    let source = "value: \"list['Data']\"\n";
    let parser_output = parse_text(source, new_sink(), ParseOptions::new()).parser_output;
    let parse_tree = parser_output.parse_tree.0;
    let outer_node = find_node_by_offset(parse_tree, utf16_index_of(source, "list"));
    let outer_string_list =
        get_first_ancestor_or_self_of_kind(outer_node, ParseNodeType::StringList).unwrap();

    let outer_annotation = parser_output
        .string_annotations
        .get(StringListNode(outer_string_list))
        .unwrap();
    let nested_node = find_node_by_offset(outer_annotation, utf16_index_of(source, "Data"));
    let nested_string_list =
        get_first_ancestor_or_self_of_kind(nested_node, ParseNodeType::StringList).unwrap();

    let nested_annotation = parser_output
        .string_annotations
        .get(StringListNode(nested_string_list))
        .unwrap();
    assert_eq!(nested_annotation.node_type, ParseNodeType::Name);
    let nested_name = NameNode::cast(nested_annotation)
        .expect("Expected the nested quoted annotation to parse as a name");

    assert_eq!(nested_name.d().value(), "Data");
    assert_eq!(outer_annotation.parent.get(), Some(outer_string_list));
    assert_eq!(nested_annotation.parent.get(), Some(nested_string_list));
    assert_eq!(outer_string_list.a, parser_output.parse_tree.a);
    assert_eq!(nested_string_list.a, parser_output.parse_tree.a);
    assert_eq!(
        parser_output.string_annotations,
        get_parser_string_annotation_info(&parse_tree)
    );
    // `Object.keys(outerStringList.d)` is `['strings', 'hasParens']`: the Rust
    // `StringListNodeDetails` holds exactly those two fields.
}

// tests/parser.test.ts:194
#[test]
fn parser_string_annotation_side_data_excludes_unsupported_and_non_annotation_strings() {
    let cases = [
        ("value = \"Ordinary\"\n", "Ordinary", false),
        ("value: \"Esc\\x61ped\"\n", "Esc", false),
        ("value: r\"Raw\"\n", "Raw", false),
        ("value: b\"Bytes\"\n", "Bytes", false),
        ("value: f\"Formatted\"\n", "Formatted", false),
        ("value: \"Positive\"\n", "Positive", true),
    ];

    for (source, needle, expected) in cases {
        let parser_output = parse_text(source, new_sink(), ParseOptions::new()).parser_output;
        let node = find_node_by_offset(parser_output.parse_tree.0, utf16_index_of(source, needle));
        let string_list = get_first_ancestor_or_self_of_kind(node, ParseNodeType::StringList)
            .unwrap_or_else(|| panic!("Expected a StringList for {}", source.trim()));
        assert_eq!(
            parser_output
                .string_annotations
                .get(StringListNode(string_list))
                .is_some(),
            expected,
            "{}",
            source.trim()
        );
    }
}

fn function_parent_type_near_end(file_name: &str) -> ParseNodeType {
    let parse_results = parse_sample_file(file_name, new_sink());

    let node = find_node_by_offset(
        parse_results.parser_output.parse_tree.0,
        parse_results.text.js_len() - 2,
    );
    let function_node = get_first_ancestor_or_self_of_kind(node, ParseNodeType::Function).unwrap();
    function_node.parent.get().unwrap().node_type
}

// tests/parser.test.ts:217
#[test]
fn parser_recovery1() {
    big_stack(|| {
        assert_eq!(
            function_parent_type_near_end("parserRecovery1.py"),
            ParseNodeType::Module
        )
    });
}

// tests/parser.test.ts:226
#[test]
fn parser_recovery2() {
    big_stack(|| {
        assert_eq!(
            function_parent_type_near_end("parserRecovery2.py"),
            ParseNodeType::Suite
        )
    });
}

// tests/parser.test.ts:235
#[test]
fn parser_recovery3() {
    big_stack(|| {
        assert_eq!(
            function_parent_type_near_end("parserRecovery3.py"),
            ParseNodeType::Module
        )
    });
}

// tests/parser.test.ts:244
#[test]
fn finally_exit1() {
    big_stack(|| {
        let diag_sink1 = new_sink();
        parse_sample_file_with_version("finallyExit1.py", diag_sink1, PYTHON_VERSION3_13);
        assert_eq!(diag_sink1.get_errors().len(), 0);

        let diag_sink2 = new_sink();
        parse_sample_file_with_version("finallyExit1.py", diag_sink2, PYTHON_VERSION3_14);
        assert_eq!(diag_sink2.get_errors().len(), 5);
    });
}

// tests/parser.test.ts:265
#[test]
fn trailing_backslash_cr_at_eof() {
    // A file that ends with a line-continuation backslash followed by a CR
    // should produce a syntax error.
    let code = "\"\"\"Comment\"\"\"\n\n\\\r";

    let diag_sink = new_sink();
    parse_text(code, diag_sink, ParseOptions::new());
    let errors = diag_sink.get_errors();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.message.contains("Unexpected EOF")));
}

fn options_for(python_version: crate::common::python_version::PythonVersion) -> ParseOptions {
    let mut parse_options = ParseOptions::new();
    parse_options.python_version = python_version;
    parse_options
}

// tests/parser.test.ts:277
#[test]
fn lazy_import_python_3_15() {
    let diag_sink = new_sink();
    let parse_results = parse_text(
        "lazy import json\nlazy from json import loads\nlazy = 1\n",
        diag_sink,
        options_for(PYTHON_VERSION3_15),
    );

    // No syntax errors at 3.15.
    assert_eq!(diag_sink.get_errors().len(), 0);

    // First statement: lazy import json
    let import_node = ImportNode::cast(statement(&parse_results, 0)).unwrap();
    assert_eq!(import_node.d().is_lazy.get(), Some(true));
    assert!(import_node.d().lazy_token.get().is_some());

    // Second statement: lazy from json import loads
    let import_from_node = ImportFromNode::cast(statement(&parse_results, 1)).unwrap();
    assert_eq!(import_from_node.d().is_lazy.get(), Some(true));
    assert!(import_from_node.d().lazy_token.get().is_some());

    // Third statement: lazy = 1 (identifier use, not a keyword)
    assert_eq!(
        statement(&parse_results, 2).node_type,
        ParseNodeType::Assignment
    );
}

// tests/parser.test.ts:315
#[test]
fn lazy_import_python_3_14_produces_error() {
    let diag_sink = new_sink();
    parse_text(
        "lazy import json\n",
        diag_sink,
        options_for(PYTHON_VERSION3_14),
    );

    let errors = diag_sink.get_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, LocMessage::lazy_import_illegal());
}

// tests/parser.test.ts:327
#[test]
fn lazy_import_from_form_version_gate_at_3_14() {
    let diag_sink = new_sink();
    parse_text(
        "lazy from json import loads\n",
        diag_sink,
        options_for(PYTHON_VERSION3_14),
    );

    let errors = diag_sink.get_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, LocMessage::lazy_import_illegal());
}

// tests/parser.test.ts:339
#[test]
fn lazy_import_stub_file_at_3_14_does_not_error() {
    let diag_sink = new_sink();
    let mut parse_options = options_for(PYTHON_VERSION3_14);
    parse_options.is_stub_file = true;

    parse_text("lazy import json\n", diag_sink, parse_options);

    assert_eq!(diag_sink.get_errors().len(), 0);
}

// tests/parser.test.ts:350
#[test]
fn lazy_import_lazy_as_module_name_at_3_15() {
    let diag_sink = new_sink();
    let parse_results = parse_text(
        "import lazy\nfrom lazy import x\n",
        diag_sink,
        options_for(PYTHON_VERSION3_15),
    );

    assert_eq!(diag_sink.get_errors().len(), 0);

    // 'import lazy' — regular import, not lazy
    let import_node = ImportNode::cast(statement(&parse_results, 0)).unwrap();
    assert!(!import_node.d().is_lazy.get().unwrap_or(false));

    // 'from lazy import x' — regular import, not lazy
    let import_from_node = ImportFromNode::cast(statement(&parse_results, 1)).unwrap();
    assert!(!import_from_node.d().is_lazy.get().unwrap_or(false));
}

// tests/parser.test.ts:376
#[test]
fn lazy_import_soft_keyword_identifier_interactions_at_3_15() {
    let snippets = [
        "def lazy(): pass\n",
        "class lazy: pass\n",
        "f(lazy=1)\n",
        "def f(lazy=1): pass\n",
        "obj.lazy\n",
        "(lazy := 1)\n",
        "lambda lazy: lazy\n",
    ];

    for snippet in snippets {
        let sink = new_sink();
        parse_text(snippet, sink, options_for(PYTHON_VERSION3_15));
        assert_eq!(
            sink.get_errors().len(),
            0,
            "Unexpected error for: {}",
            snippet.trim()
        );
    }
}

// tests/parser.test.ts:397
#[test]
fn lazy_import_payload_variants_at_3_15() {
    let variants = [
        "lazy import a.b.c\n",
        "lazy import os as o\n",
        "lazy import os, sys\n",
        "lazy from .pkg import x\n",
        "lazy from ..pkg import x\n",
        "lazy from m import (a, b, c)\n",
    ];

    for variant in variants {
        let sink = new_sink();
        let parse_results = parse_text(variant, sink, options_for(PYTHON_VERSION3_15));
        assert_eq!(
            sink.get_errors().len(),
            0,
            "Unexpected error for: {}",
            variant.trim()
        );

        let node = statement(&parse_results, 0);
        if let Some(import_node) = ImportNode::cast(node) {
            assert_eq!(
                import_node.d().is_lazy.get(),
                Some(true),
                "Expected isLazy=true for: {}",
                variant.trim()
            );
        } else if let Some(import_from_node) = ImportFromNode::cast(node) {
            assert_eq!(
                import_from_node.d().is_lazy.get(),
                Some(true),
                "Expected isLazy=true for: {}",
                variant.trim()
            );
        } else {
            panic!("Unexpected node type for: {}", variant.trim());
        }
    }
}

// tests/parser.test.ts:427
#[test]
fn lazy_import_wildcard_import_rejected_at_3_15() {
    let diag_sink = new_sink();
    parse_text(
        "lazy from m import *\n",
        diag_sink,
        options_for(PYTHON_VERSION3_15),
    );

    let errors = diag_sink.get_errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].message,
        LocMessage::lazy_import_wildcard_illegal()
    );
}

/// A parse node's tokens outlive the tokenizer output, which frees the rest of
/// its tokens, and the next parse, which reuses the freed memory.
#[test]
fn node_tokens_outlive_the_tokenizer_output() {
    let results = parse_text("first = 1\ndef second(third): pass\n", new_sink(), ParseOptions::new());
    let tree = results.parser_output.parse_tree;
    drop(results);
    // Enough lines to reuse the freed scratch chunks. Miri retires them instead
    // (PYRIGHT_RS_TOKENS=check in tools/miri.sh), so a stale token is a use after free.
    let lines = if cfg!(miri) { 200 } else { 20_000 };
    drop(parse_text(&"other = more\n".repeat(lines), new_sink(), ParseOptions::new()));

    let mut names = vec![];
    let mut stack = vec![tree.0];
    while let Some(node) = stack.pop() {
        if let Some(name) = crate::parser::parse_nodes::NameNode::cast(node) {
            names.push(name.d().value());
        }
        stack.extend(get_child_nodes(node, None).into_iter().flatten());
    }
    names.sort();
    assert_eq!(names, ["first", "second", "third"]);
}
