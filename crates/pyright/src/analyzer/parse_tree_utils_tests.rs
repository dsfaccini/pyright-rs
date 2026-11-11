//! Port of `packages/pyright-internal/src/tests/parseTreeUtils.test.ts`.
//!
//! The fourslash tests parse their one file directly: `parse_and_get_test_state`
//! strips the `[|...|]` ranges and `/*name*/` markers as `fourSlashParser.ts`
//! does and records their offsets. `getNodeAtMarker` reads the parse tree of the
//! bound file; binding does not change that tree, so a parse is enough.
//!
//! Not ported: `printExpression forward declarations use parser annotations but
//! not owner annotations`. It runs `program.analyze()` so the evaluator stores
//! the tier-2 annotation of `cast("Data", ...)`.
//!
//! The tests after the port exercise the scope walks, which no TypeScript test
//! covers, and a few `printExpression` quirks.

use jsrt::JsString;

use super::*;
use crate::analyzer::analyzer_node_info::{AnalyzerNodeInfoStore, set_scope};
use crate::analyzer::scope::{Scope, ScopeType};
use crate::common::diagnostic_sink::DiagnosticSink;
use crate::common::position_utils::convert_offset_to_position;
use crate::parser::parser::{ParseOptions, Parser};

/// Runs a test body on a thread with a large stack, as the parser tests do.
fn big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

fn parse_text(text: &str) -> ParseFileResults {
    let text: Vec<u16> = text.encode_utf16().collect();
    Parser::new().parse_source_file(
        JsString::from(text),
        ParseOptions::new(),
        P::new(DiagnosticSink::new(None)),
        None,
    )
}

/// One `[|...|]` range of a fourslash file, with the marker it holds.
struct FourSlashRange {
    pos: i32,
    end: i32,
    marker: Option<String>,
}

/// A one-file fourslash test: its markers, its ranges and its parse.
struct TestState {
    markers: Vec<(String, i32)>,
    ranges: Vec<FourSlashRange>,
    parse_results: ParseFileResults,
}

const VALID_MARKER_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz$1234567890_";

fn flush(output: &mut String, chars: &[char], last_normal_char_position: usize, safe_index: usize) {
    if safe_index > last_normal_char_position {
        output.extend(&chars[last_normal_char_position..safe_index]);
    }
}

/// `parseAndGetTestState` for one anonymous file: the content rules of
/// `fourSlashParser.ts` (`////` lines, one leading space chomped when every
/// line has one, ranges and slash-star markers removed), then a parse.
fn parse_and_get_test_state(code: &str) -> TestState {
    let mut content: Option<String> = None;
    for line in code.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(text) = line.strip_prefix("////") {
            content = Some(match content {
                None => text.to_string(),
                Some(content) => content + "\n" + text,
            });
        }
    }
    let content = content.unwrap_or_default();
    // Char indices are UTF-16 offsets only for ASCII.
    assert!(content.is_ascii());

    // chompLeadingSpaceWithMapping
    let lines: Vec<&str> = content.split('\n').collect();
    let content = if lines
        .iter()
        .all(|line| line.is_empty() || line.starts_with(' '))
    {
        lines
            .iter()
            .map(|line| line.get(1..).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        content.clone()
    };

    let chars: Vec<char> = content.chars().collect();
    let mut output = String::new();
    let mut markers: Vec<(String, i32)> = vec![];
    let mut ranges: Vec<FourSlashRange> = vec![];
    let mut open_ranges: Vec<FourSlashRange> = vec![];
    // The source index and output position of the `/*` of an open marker.
    let mut open_marker: Option<(usize, i32)> = None;
    let mut last_normal_char_position = 0;
    let mut difference = 0;

    if !chars.is_empty() {
        let mut previous_char = chars[0];
        for i in 1..chars.len() {
            let current_char = chars[i];
            match open_marker {
                None => {
                    if previous_char == '[' && current_char == '|' {
                        // found a range start
                        open_ranges.push(FourSlashRange {
                            pos: i as i32 - 1 - difference,
                            end: -1,
                            marker: None,
                        });
                        flush(&mut output, &chars, last_normal_char_position, i - 1);
                        last_normal_char_position = i + 1;
                        difference += 2;
                    } else if previous_char == '|' && current_char == ']' {
                        // found a range end
                        let mut range = open_ranges
                            .pop()
                            .expect("Found range end with no matching start.");
                        range.end = i as i32 - 1 - difference;
                        ranges.push(range);
                        flush(&mut output, &chars, last_normal_char_position, i - 1);
                        last_normal_char_position = i + 1;
                        difference += 2;
                    } else if previous_char == '/' && current_char == '*' {
                        // found a possible marker start
                        open_marker = Some((i - 1, i as i32 - 1 - difference));
                    }
                }
                Some((source_position, position)) => {
                    if previous_char == '*' && current_char == '/' {
                        let name: String = chars[source_position + 2..i - 1].iter().collect();
                        let name = name.trim().to_string();
                        if let Some(open_range) = open_ranges.last_mut() {
                            open_range.marker = Some(name.clone());
                        }
                        markers.push((name, position));

                        flush(
                            &mut output,
                            &chars,
                            last_normal_char_position,
                            source_position,
                        );
                        last_normal_char_position = i + 1;
                        difference += (i + 1 - source_position) as i32;
                        open_marker = None;
                    } else if !VALID_MARKER_CHARS.contains(current_char) {
                        if current_char == '*' && i < chars.len() - 1 && chars[i + 1] == '/' {
                            // The marker is about to be closed, ignore the 'invalid' char
                        } else {
                            // A block comment, not a marker.
                            flush(&mut output, &chars, last_normal_char_position, i);
                            last_normal_char_position = i;
                            open_marker = None;
                        }
                    }
                }
            }

            // A line break leaves `previousChar` unchanged.
            if current_char == '\n' || current_char == '\r' {
                continue;
            }

            previous_char = current_char;
        }
    }

    flush(&mut output, &chars, last_normal_char_position, chars.len());
    assert!(open_ranges.is_empty(), "Unterminated range.");
    assert!(open_marker.is_none(), "Unterminated marker.");

    TestState {
        markers,
        ranges,
        parse_results: parse_text(&output),
    }
}

impl TestState {
    fn parse_tree(&self) -> P<ParseNode> {
        self.parse_results.parser_output.parse_tree.node()
    }

    fn get_marker_position(&self, marker_name: &str) -> i32 {
        self.markers
            .iter()
            .find(|(name, _)| name == marker_name)
            .unwrap_or_else(|| panic!("no marker {marker_name}"))
            .1
    }

    fn get_range_by_marker_name(&self, marker_name: &str) -> &FourSlashRange {
        self.ranges
            .iter()
            .find(|range| range.marker.as_deref() == Some(marker_name))
            .unwrap_or_else(|| panic!("no range for {marker_name}"))
    }

    fn convert_position_range(&self, range: &FourSlashRange) -> Range {
        let lines = &self.parse_results.tokenizer_output.lines;
        Range {
            start: convert_offset_to_position(range.pos, lines),
            end: convert_offset_to_position(range.end, lines),
        }
    }
}

fn get_node_at_marker(state: &TestState, marker_name: &str) -> P<ParseNode> {
    let position = state.get_marker_position(marker_name);
    find_node_by_offset(state.parse_tree(), position, None).unwrap()
}

fn get_node_for_range(state: &TestState, marker_name: &str) -> P<ParseNode> {
    let range = state.get_range_by_marker_name(marker_name);
    let text_range = TextRange::from_bounds(range.pos, range.end);

    let node = get_node_at_marker(state, marker_name);
    let mut current = Some(node);
    while let Some(cur) = current {
        if TextRange::contains_range(&cur, &text_range) {
            return cur;
        }

        current = cur.parent.get();
    }

    node
}

/// `getNodeAtMarker(...) as NameNode`: the cast is unchecked, as in
/// TypeScript. `a.b.(c)` puts an `ErrorNode` at its marker.
fn name_at_marker(state: &TestState, marker_name: &str) -> NameNode {
    NameNode(get_node_at_marker(state, marker_name))
}

#[test]
fn is_import_module_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// import [|/*marker*/os|]
    "#,
        );

        assert!(is_import_module_name(get_node_at_marker(&state, "marker")));
    });
}

#[test]
fn is_import_alias_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// import os as [|/*marker*/os|]
    "#,
        );

        assert!(is_import_alias(get_node_at_marker(&state, "marker")));
    });
}

#[test]
fn is_from_import_module_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// from [|/*marker*/os|] import path
    "#,
        );

        assert!(is_from_import_module_name(get_node_at_marker(
            &state, "marker"
        )));
    });
}

#[test]
fn is_from_import_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// from . import [|/*marker*/os|]
    "#,
        );

        assert!(is_from_import_name(get_node_at_marker(&state, "marker")));
    });
}

#[test]
fn is_from_import_alias_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// from . import os as [|/*marker*/os|]
    "#,
        );

        assert!(is_from_import_alias(get_node_at_marker(&state, "marker")));
    });
}

#[test]
fn get_first_ancestor_or_self_of_kind_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// import a.b.c
//// a.b.c.function(
////     1 + 2 + 3,
////     [|/*result*/a.b.c.function2(
////         [|/*marker*/"name"|]
////     )|]
//// )
    "#,
        );

        let node = get_first_ancestor_or_self_of_kind(
            Some(get_node_at_marker(&state, "marker")),
            ParseNodeType::Call,
        );
        let node = node.unwrap();

        let result = state.get_range_by_marker_name("result");
        assert_eq!(node.node_type, ParseNodeType::Call);
        assert_eq!(node.start.get(), result.pos);
        assert_eq!(TextRange::get_end(&node), result.end);
    });
}

#[test]
fn is_within_type_annotation_requires_an_actual_quoted_string_association() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// from typing import TypedDict
////
//// class Data:
////     pass
////
//// unquoted: /*unquoted*/Data
//// quoted: "/*quoted*/Data"
//// td: TypedDict[{"/*key*/field": "/*value*/int"}]
    "#,
        );

        let unquoted_node = get_node_at_marker(&state, "unquoted");
        assert_eq!(unquoted_node.node_type, ParseNodeType::Name);
        assert!(!is_within_type_annotation(
            unquoted_node,
            /* requireQuotedAnnotation */ true,
            None
        ));

        let quoted_string_list = get_first_ancestor_or_self_of_kind(
            Some(get_node_at_marker(&state, "quoted")),
            ParseNodeType::StringList,
        );
        let quoted_string_list = StringListNode::cast(quoted_string_list.unwrap()).unwrap();
        let quoted_annotation = get_parser_string_annotation(quoted_string_list);
        let quoted_annotation = quoted_annotation.unwrap();
        assert!(is_within_type_annotation(
            quoted_annotation,
            /* requireQuotedAnnotation */ true,
            None
        ));

        let key_string_list = get_first_ancestor_or_self_of_kind(
            Some(get_node_at_marker(&state, "key")),
            ParseNodeType::StringList,
        );
        let key_string_list = StringListNode::cast(key_string_list.unwrap()).unwrap();
        assert_eq!(get_parser_string_annotation(key_string_list), None);
        assert!(!is_within_type_annotation(
            key_string_list.node(),
            /* requireQuotedAnnotation */ true,
            None
        ));

        let value_string_list = get_first_ancestor_or_self_of_kind(
            Some(get_node_at_marker(&state, "value")),
            ParseNodeType::StringList,
        );
        let value_string_list = StringListNode::cast(value_string_list.unwrap()).unwrap();
        let value_annotation = get_parser_string_annotation(value_string_list);
        let value_annotation = value_annotation.unwrap();
        assert!(is_within_type_annotation(
            value_annotation,
            /* requireQuotedAnnotation */ true,
            None
        ));
    });
}

#[test]
fn get_dotted_name_with_given_node_as_last_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*result1*/[|/*marker1*/a|]|]
//// [|/*result2*/a.[|/*marker2*/b|]|]
//// [|/*result3*/a.b.[|/*marker3*/c|]|]
//// [|/*result4*/a.[|/*marker4*/b|]|].c
//// [|/*result5*/[|/*marker5*/a|]|].b.c
    "#,
        );

        for i in 1..=5 {
            let marker_name = format!("marker{i}");
            let result_name = format!("result{i}");
            let node =
                get_dotted_name_with_given_node_as_last_name(name_at_marker(&state, &marker_name));
            let result = state.get_range_by_marker_name(&result_name);

            assert!(
                node.node_type == ParseNodeType::Name
                    || node.node_type == ParseNodeType::MemberAccess
            );
            assert_eq!(node.start.get(), result.pos);
            assert_eq!(TextRange::get_end(&node), result.end);
        }
    });
}

#[test]
fn get_dotted_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/a|]
//// [|/*marker2*/a.b|]
//// [|/*marker3*/a.b.c|]
//// [|/*marker4*/a.b|].c
//// [|/*marker5*/a|].b.c
    "#,
        );

        let get_dotted_name_string = |marker: &str| {
            let node = get_node_for_range(&state, marker);
            get_dotted_name(node).map(|names| {
                names
                    .iter()
                    .map(|n| n.d().value())
                    .collect::<Vec<_>>()
                    .join(".")
            })
        };

        assert_eq!(get_dotted_name_string("marker1").as_deref(), Some("a"));
        assert_eq!(get_dotted_name_string("marker2").as_deref(), Some("a.b"));
        assert_eq!(get_dotted_name_string("marker3").as_deref(), Some("a.b.c"));
        assert_eq!(get_dotted_name_string("marker4").as_deref(), Some("a.b"));
        assert_eq!(get_dotted_name_string("marker5").as_deref(), Some("a"));
    });
}

#[test]
fn get_first_name_of_dotted_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/a|]
//// [|/*marker2*/a.b|]
//// [|/*marker3*/a.b.c|]
//// [|/*marker4*/a.b|].c
//// [|/*marker5*/a|].b.c
        "#,
        );

        let get_dotted_name_string = |marker: &str| {
            let node = get_node_for_range(&state, marker);
            get_first_name_of_dotted_name(node)
                .map(|n| n.d().value().to_string())
                .unwrap_or_default()
        };

        assert_eq!(get_dotted_name_string("marker1"), "a");
        assert_eq!(get_dotted_name_string("marker2"), "a");
        assert_eq!(get_dotted_name_string("marker3"), "a");
        assert_eq!(get_dotted_name_string("marker4"), "a");
        assert_eq!(get_dotted_name_string("marker5"), "a");
    });
}

#[test]
fn is_last_name_of_dotted_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/a|]
//// a.[|/*marker2*/b|]
//// a.b.[|/*marker3*/c|]
//// a.[|/*marker4*/b|].c
//// [|/*marker5*/a|].b.c
//// (a).[|/*marker6*/b|]
//// (a.b).[|/*marker7*/c|]
//// a().[|/*marker8*/b|]
//// a[0].[|/*marker9*/b|]
//// a.b([|/*marker10*/c|]).d
//// a.b.([|/*marker11*/c|])
//// a.[|/*marker12*/b|].c()
//// a.[|/*marker13*/b|]()
//// a.[|/*marker14*/b|][]
    "#,
        );

        let expected = [
            true, true, true, false, false, true, true, false, false, true, true, false, true, true,
        ];
        for (i, expected) in expected.iter().enumerate() {
            let marker_name = format!("marker{}", i + 1);
            assert_eq!(
                is_last_name_of_dotted_name(name_at_marker(&state, &marker_name)),
                *expected,
                "{marker_name}"
            );
        }
    });
}

#[test]
fn is_first_name_of_dotted_name_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/a|]
//// a.[|/*marker2*/b|]
//// a.b.[|/*marker3*/c|]
//// a.[|/*marker4*/b|].c
//// [|/*marker5*/a|].b.c
//// ([|/*marker6*/a|]).b
//// (a.b).[|/*marker7*/c|]
//// [|/*marker8*/a|]().b
//// a[0].[|/*marker9*/b|]
//// a.b([|/*marker10*/c|]).d
//// a.b.([|/*marker11*/c|])
//// a.[|/*marker12*/b|].c()
//// [|/*marker13*/a|].b()
//// a.[|/*marker14*/b|][]
//// [|/*marker15*/a|][]
    "#,
        );

        let expected = [
            true, false, false, false, true, true, false, true, false, true, true, false, true,
            false, true,
        ];
        for (i, expected) in expected.iter().enumerate() {
            let marker_name = format!("marker{}", i + 1);
            assert_eq!(
                is_first_name_of_dotted_name(name_at_marker(&state, &marker_name)),
                *expected,
                "{marker_name}"
            );
        }
    });
}

#[test]
fn get_string_node_value_range_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// a = "[|/*marker1*/test|]"
//// b = '[|/*marker2*/test2|]'
//// c = '''[|/*marker3*/test3|]'''
    "#,
        );

        for i in 1..=3 {
            let marker_name = format!("marker{i}");
            let range = get_string_node_value_range(
                StringNode::cast(get_node_at_marker(&state, &marker_name)).unwrap(),
            );
            let result = state.get_range_by_marker_name(&marker_name);

            assert_eq!(range.start, result.pos);
            assert_eq!(TextRange::get_end(&range), result.end);
        }
    });
}

fn test_node_range(
    state: &TestState,
    marker_name: &str,
    type_: ParseNodeType,
    include_trailing_blank_lines: bool,
) {
    let range = state.get_range_by_marker_name(marker_name);

    let statement_node =
        get_first_ancestor_or_self_of_kind(Some(get_node_at_marker(state, marker_name)), type_)
            .unwrap();
    let statement_range = get_full_statement_range(
        statement_node,
        &state.parse_results,
        Some(include_trailing_blank_lines),
    );

    let expected_range = state.convert_position_range(range);

    assert_eq!(expected_range, statement_range, "{marker_name}");
}

#[test]
fn get_full_statement_range_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/import a
//// |][|/*marker2*/a = 1; |][|/*marker3*/b = 2
//// |]
//// try:
//// [|    /*marker4*/a = 1
//// |]except Exception:
////     pass
//// [|/*marker5*/if True:
////     pass|]
    "#,
        );

        test_node_range(&state, "marker1", ParseNodeType::Import, false);
        test_node_range(&state, "marker2", ParseNodeType::Assignment, false);
        test_node_range(&state, "marker3", ParseNodeType::Assignment, false);
        test_node_range(&state, "marker4", ParseNodeType::Assignment, false);
        test_node_range(&state, "marker5", ParseNodeType::If, false);
    });
}

#[test]
fn get_full_statement_range_with_trailing_blank_lines() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker*/def foo():
////     return 1
////
//// |]def bar():
////     pass
    "#,
        );

        test_node_range(&state, "marker", ParseNodeType::Function, true);
    });
}

#[test]
fn get_full_statement_range_with_only_trailing_blank_lines() {
    big_stack(|| {
        // The last line is "//// ", with a trailing space.
        let state = parse_and_get_test_state(
            "\n//// [|/*marker*/def foo():\n////     return 1\n//// |]\n//// \n    ",
        );

        test_node_range(&state, "marker", ParseNodeType::Function, true);
    });
}

fn check_expression(node: P<ParseNode>, expected: &str) {
    assert!(is_expression_node(&node));
    assert_eq!(print_expression(node, PrintExpressionFlags::None), expected);
}

#[test]
fn print_expression_test() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/not x|]
//// [|/*marker2*/+x|]
    "#,
        );
        check_expression(get_node_at_marker(&state, "marker1"), "not x");
        check_expression(get_node_at_marker(&state, "marker2"), "+x");
    });
}

#[test]
fn print_expression_slice() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// x = [1, 2, 3]
//// [|/*marker1*/x[:]|]
//// [|/*marker2*/x[1:]|]
//// [|/*marker3*/x[:1]|]
//// [|/*marker4*/x[1:2]|]
//// [|/*marker5*/x[::2]|]
//// [|/*marker6*/x[1::2]|]
//// [|/*marker7*/x[::-1]|]
//// [|/*marker8*/x[1:2:3]|]
    "#,
        );
        check_expression(get_node_for_range(&state, "marker1"), "x[:]");
        check_expression(get_node_for_range(&state, "marker2"), "x[1:]");
        check_expression(get_node_for_range(&state, "marker3"), "x[:1]");
        check_expression(get_node_for_range(&state, "marker4"), "x[1:2]");
        check_expression(get_node_for_range(&state, "marker5"), "x[::2]");
        check_expression(get_node_for_range(&state, "marker6"), "x[1::2]");
        check_expression(get_node_for_range(&state, "marker7"), "x[::-1]");
        check_expression(get_node_for_range(&state, "marker8"), "x[1:2:3]");
    });
}

#[test]
fn print_expression_set() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*marker1*/{1}|]
//// [|/*marker2*/{1, 2}|]
    "#,
        );
        check_expression(get_node_for_range(&state, "marker1"), "{1}");
        check_expression(get_node_for_range(&state, "marker2"), "{1, 2}");
    });
}

fn find_name_r(code: &str) {
    let state = parse_and_get_test_state(code);
    let range = state.get_range_by_marker_name("marker");

    let node = find_node_by_offset(state.parse_tree(), range.pos, None);
    let node = node.unwrap();
    assert_eq!(node.node_type, ParseNodeType::Name);
    assert_eq!(NameNode(node).d().value(), "r");
}

#[test]
fn find_node_by_offset_test() {
    big_stack(|| {
        find_name_r(
            r#"
//// class A:
////     def read(self): pass
////
//// class B(A):
////     x1 = 1
////     def r[|/*marker*/|]
////
    "#,
        );
    });
}

#[test]
fn find_node_by_offset_with_binary_search() {
    big_stack(|| {
        find_name_r(
            r#"
//// class A:
////     def read(self): pass
////
//// class B(A):
////     x1 = 1
////     x2 = 2
////     x3 = 3
////     x4 = 4
////     x5 = 5
////     x6 = 6
////     x7 = 7
////     x8 = 8
////     x9 = 9
////     x10 = 10
////     x11 = 11
////     x12 = 12
////     x13 = 13
////     x14 = 14
////     x15 = 15
////     x16 = 16
////     x17 = 17
////     x18 = 18
////     x19 = 19
////     def r[|/*marker*/|]
////
    "#,
        );
    });
}

#[test]
fn find_node_by_offset_with_binary_search_choose_earliest_match() {
    big_stack(|| {
        find_name_r(
            r#"
//// class A:
////     def read(self): pass
////
//// class B(A):
////     x1 = 1
////     x2 = 2
////     x3 = 3
////     x4 = 4
////     x5 = 5
////     x6 = 6
////     x7 = 7
////     x8 = 8
////     x9 = 9
////     x10 = 10
////     x11 = 11
////     x12 = 12
////     x13 = 13
////     x14 = 14
////     x15 = 15
////     x16 = 16
////     x17 = 17
////     x18 = 18
////     x19 = 19
////     def r[|/*marker*/|]
////     x20 = 20
////     x21 = 21
////
    "#,
        );
    });
}

// Not in `parseTreeUtils.test.ts`.

#[test]
fn print_operator_names() {
    assert_eq!(print_operator(OperatorType::Add), "+");
    assert_eq!(print_operator(OperatorType::Not), "not ");
    assert_eq!(print_operator(OperatorType::IsNot), "is not");
    // `Walrus` has no entry in `OperatorTypeNameMap`.
    assert_eq!(print_operator(OperatorType::Walrus), "unknown");
    assert_eq!(print_parse_node_type(ParseNodeType::TypeAlias), "TypeAlias");
}

#[test]
fn print_expression_quirks() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*dict*/{a: 1, b: 2}|]
//// [|/*bigint*/12345678901234567890|]
//// [|/*imaginary*/3j|]
//// [|/*fstring*/f"a{b}c"|]
//// [|/*tuple*/(a,)|]
//// [|/*lambda*/lambda a, *b, c=1, **d: a|]
//// [|/*comprehension*/[x for x in y if x]|]
    "#,
        );
        // A template literal of an array joins the entries with "," and no space.
        check_expression(get_node_for_range(&state, "dict"), "{ a: 1,b: 2 }");
        check_expression(get_node_for_range(&state, "bigint"), "12345678901234567890");
        check_expression(get_node_for_range(&state, "imaginary"), "3j");
        check_expression(get_node_for_range(&state, "fstring"), "f\"a{b}c\"");
        check_expression(get_node_for_range(&state, "tuple"), "(a, )");
        check_expression(
            get_node_for_range(&state, "lambda"),
            "lambda a, *b, c = 1, **d: a",
        );
        check_expression(
            get_node_for_range(&state, "comprehension"),
            "[x for x in y if x]",
        );
    });
}

#[test]
fn matching_expressions() {
    big_stack(|| {
        let state = parse_and_get_test_state(
            r#"
//// [|/*a*/a|]
//// [|/*a2*/a|]
//// [|/*b*/b|]
//// [|/*ab*/a.b|]
//// [|/*ab2*/a.b|]
//// [|/*ac*/a.c|]
//// [|/*a0*/a[0]|]
//// [|/*a0_2*/a[0]|]
//// [|/*a1*/a[1]|]
//// [|/*aneg*/a[-1]|]
//// [|/*aneg2*/a[-1]|]
//// [|/*akey*/a["k"]|]
//// [|/*akey2*/a["k"]|]
    "#,
        );
        let node = |marker: &str| get_node_for_range(&state, marker);

        assert!(is_matching_expression(node("a"), node("a2"), None));
        assert!(!is_matching_expression(node("a"), node("b"), None));
        assert!(!is_matching_expression(
            node("a"),
            node("a2"),
            Some(&|_, _| false)
        ));
        assert!(is_matching_expression(node("ab"), node("ab2"), None));
        assert!(!is_matching_expression(node("ab"), node("ac"), None));
        assert!(is_partial_matching_expression(node("ab"), node("a")));
        assert!(is_matching_expression(node("a0"), node("a0_2"), None));
        assert!(!is_matching_expression(node("a0"), node("a1"), None));
        assert!(is_matching_expression(node("aneg"), node("aneg2"), None));
        assert!(is_matching_expression(node("akey"), node("akey2"), None));
    });
}

const CLASS_CODE: &str = r#"
//// class C:
////     def /*name*/f(self):
////         self./*member*/x
    "#;

/// Scopes for the module, `C` and `f` of `CLASS_CODE`, as the binder sets them.
fn set_class_code_scopes(state: &TestState) -> (P<AnalyzerNodeInfoStore>, [P<Scope>; 3]) {
    let member = get_node_at_marker(state, "member");
    let function =
        get_first_ancestor_or_self_of_kind(Some(member), ParseNodeType::Function).unwrap();
    let class = get_first_ancestor_or_self_of_kind(Some(member), ParseNodeType::Class).unwrap();

    let store = AnalyzerNodeInfoStore::new();
    let builtins = Scope::new(ScopeType::Builtin, None, None, None);
    let module_scope = Scope::new(ScopeType::Module, Some(builtins), None, None);
    let class_scope = Scope::new(ScopeType::Class, Some(module_scope), None, None);
    let function_scope = Scope::new(ScopeType::Function, Some(class_scope), None, None);
    set_scope(state.parse_tree(), module_scope, store.get());
    set_scope(class, class_scope, store.get());
    set_scope(function, function_scope, store.get());
    (store, [module_scope, class_scope, function_scope])
}

#[test]
fn enclosing_nodes() {
    big_stack(|| {
        let state = parse_and_get_test_state(CLASS_CODE);
        let member = get_node_at_marker(&state, "member");
        let expr = member.parent.get().unwrap();
        assert_eq!(expr.node_type, ParseNodeType::MemberAccess);
        let function = FunctionNode::cast(
            get_first_ancestor_or_self_of_kind(Some(expr), ParseNodeType::Function).unwrap(),
        )
        .unwrap();
        let class = ClassNode::cast(
            get_first_ancestor_or_self_of_kind(Some(expr), ParseNodeType::Class).unwrap(),
        )
        .unwrap();
        let module = state.parse_results.parser_output.parse_tree;

        assert_eq!(get_enclosing_class(expr, false), Some(class));
        assert_eq!(get_enclosing_class(expr, true), None);
        assert_eq!(get_enclosing_function(expr), Some(function));
        assert_eq!(get_enclosing_module(expr), module);
        assert_eq!(get_enclosing_lambda(expr), None);
        assert_eq!(get_enclosing_class_or_function(expr), Some(function.node()));
        assert_eq!(get_module_node(expr), Some(module));
        assert_eq!(get_class_full_name(class.node(), "mod", "C"), "mod.C");
        assert_eq!(get_type_var_scope_node(expr), Some(function.node()));
        // `self.x` is in `f`, not `__init__`.
        assert!(!is_final_allowed_for_assignment_target(expr));
        assert!(!is_required_allowed_for_assignment_target(expr));
        assert!(is_node_contained_within(expr, class.node()));
        assert_eq!(print_expression(expr, PrintExpressionFlags::None), "self.x");
    });
}

#[test]
fn evaluation_scope_nodes() {
    big_stack(|| {
        let state = parse_and_get_test_state(CLASS_CODE);
        let (store, _) = set_class_code_scopes(&state);
        let member = get_node_at_marker(&state, "member");
        let function =
            get_first_ancestor_or_self_of_kind(Some(member), ParseNodeType::Function).unwrap();
        let class = get_first_ancestor_or_self_of_kind(Some(member), ParseNodeType::Class).unwrap();

        let info = get_evaluation_scope_node(member, store.get());
        assert_eq!(info.node, function);
        assert_eq!(info.use_proxy_scope, None);
        assert_eq!(info.use_chained_module_level_scopes, Some(true));

        // The function name is evaluated in the class scope.
        let name = get_node_at_marker(&state, "name");
        let info = get_evaluation_scope_node(name, store.get());
        assert_eq!(info.node, class);
        assert_eq!(info.use_chained_module_level_scopes, None);

        // A class is not an execution scope.
        assert_eq!(
            get_execution_scope_node(name, store.get()),
            state.parse_tree()
        );
        assert_eq!(
            get_enclosing_function_evaluation_scope(member, store.get()),
            FunctionNode::cast(function)
        );
    });
}

#[test]
fn scope_utils_walks() {
    use crate::analyzer::scope_utils::{
        find_top_node_in_scope, get_built_in_scope, get_scope_for_node, get_scope_hierarchy,
        is_scope_contained_within,
    };

    big_stack(|| {
        let state = parse_and_get_test_state(CLASS_CODE);
        let (store, [module_scope, class_scope, function_scope]) = set_class_code_scopes(&state);
        let member = get_node_at_marker(&state, "member");
        let name = get_node_at_marker(&state, "name");
        let function =
            get_first_ancestor_or_self_of_kind(Some(member), ParseNodeType::Function).unwrap();
        let builtins = module_scope.parent.unwrap();

        assert_eq!(get_built_in_scope(function_scope), builtins);
        assert!(is_scope_contained_within(function_scope, module_scope));
        assert!(!is_scope_contained_within(module_scope, class_scope));

        assert_eq!(
            get_scope_for_node(member, store.get()),
            Some(function_scope)
        );
        assert_eq!(get_scope_for_node(name, store.get()), Some(class_scope));

        assert_eq!(
            get_scope_hierarchy(member, None, store.get()),
            Some(vec![function_scope, class_scope, module_scope])
        );
        assert_eq!(
            get_scope_hierarchy(member, Some(class_scope), store.get()),
            Some(vec![function_scope, class_scope])
        );
        assert_eq!(
            get_scope_hierarchy(member, Some(builtins), store.get()),
            None
        );

        assert_eq!(
            find_top_node_in_scope(member, function_scope, store.get()),
            Some(function)
        );
    });
}
