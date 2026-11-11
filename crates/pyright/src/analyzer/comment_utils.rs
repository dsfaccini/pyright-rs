//! Port of `packages/pyright-internal/src/analyzer/commentUtils.ts`.

use jsrt::{JsStr, P};

use crate::common::config_options::{
    DiagnosticLevel, DiagnosticRuleSet, clone_diagnostic_rule_set, get_basic_diagnostic_rule_set,
    get_boolean_diagnostic_rules, get_diag_level_diagnostic_rules, get_standard_diagnostic_rule_set,
    get_strict_diagnostic_rule_set, get_strict_mode_not_overridden_rules,
};
use crate::common::debug::assert;
use crate::common::diagnostic::{
    DEFAULT_MAX_DIAGNOSTIC_DEPTH, DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT, DiagnosticAddendum,
};
use crate::common::diagnostic_rules::DiagnosticRule;
use crate::common::position_utils::convert_offset_to_position;
use crate::common::text_range::TextRange;
use crate::common::text_range_collection::TextRangeCollection;
use crate::localization::localize::{LocAddendum, LocMessage};
use crate::parser::tokenizer_types::Token;

// analyzer/commentUtils.ts:31
const STRICT_SETTING: &str = "strict";
// analyzer/commentUtils.ts:32
const STANDARD_SETTING: &str = "standard";
// analyzer/commentUtils.ts:33
const BASIC_SETTING: &str = "basic";

// analyzer/commentUtils.ts:35
#[derive(Clone, Debug)]
pub struct CommentDiagnostic {
    pub message: String,
    pub range: TextRange,
}

// analyzer/commentUtils.ts:40
pub fn get_file_level_directives(
    tokens: &TextRangeCollection<P<Token>>,
    lines: &TextRangeCollection<TextRange>,
    default_rule_set: &DiagnosticRuleSet,
    use_strict: bool,
    diagnostics: &mut Vec<CommentDiagnostic>,
) -> DiagnosticRuleSet {
    let mut rule_set = clone_diagnostic_rule_set(default_rule_set);
    let mut is_modified = false;

    if use_strict {
        apply_strict_rules(&mut rule_set);
        is_modified = true;
    }

    for i in 0..tokens.count() {
        let token = tokens.get_item_at(i);
        if let Some(comments) = Token::comments(token) {
            for comment in comments {
                let (value, text_range) =
                    trim_text_with_range(&comment.value, TextRange { start: comment.start, length: comment.length });

                let is_comment_on_own_line = || -> bool {
                    let cur_token_line_offset = convert_offset_to_position(comment.start, lines).character;
                    cur_token_line_offset <= 1
                };

                rule_set = parse_pyright_comment(&value, text_range, &is_comment_on_own_line, rule_set, diagnostics);
                is_modified = true;
            }
        }
    }

    if is_modified { rule_set } else { default_rule_set.clone() }
}

// analyzer/commentUtils.ts:79
fn apply_strict_rules(rule_set: &mut DiagnosticRuleSet) {
    override_rules(rule_set, &get_strict_diagnostic_rule_set(), &get_strict_mode_not_overridden_rules());
}

// analyzer/commentUtils.ts:83
fn apply_standard_rules(rule_set: &mut DiagnosticRuleSet) {
    overwrite_rules(rule_set, &get_standard_diagnostic_rule_set());
}

// analyzer/commentUtils.ts:87
fn apply_basic_rules(rule_set: &mut DiagnosticRuleSet) {
    overwrite_rules(rule_set, &get_basic_diagnostic_rule_set());
}

// analyzer/commentUtils.ts:91
fn override_rules(rule_set: &mut DiagnosticRuleSet, override_rule_set: &DiagnosticRuleSet, skip_rule_names: &[DiagnosticRule]) {
    let bool_rule_names = get_boolean_diagnostic_rules(false);
    let diag_rule_names = get_diag_level_diagnostic_rules();

    // Enable the strict rules as appropriate.
    for rule_name in bool_rule_names {
        if skip_rule_names.contains(&rule_name) {
            continue;
        }

        if override_rule_set.get_bool(rule_name).unwrap() {
            rule_set.set_bool(rule_name, true);
        }
    }

    for rule_name in diag_rule_names {
        if skip_rule_names.contains(&rule_name) {
            continue;
        }

        let override_value = override_rule_set.get_level(rule_name).unwrap();
        let prev_value = rule_set.get_level(rule_name).unwrap();

        // Override only if the new value is more strict than the existing value.
        if override_value == DiagnosticLevel::Error
            || (override_value == DiagnosticLevel::Warning && prev_value != DiagnosticLevel::Error)
            || (override_value == DiagnosticLevel::Information
                && prev_value != DiagnosticLevel::Error
                && prev_value != DiagnosticLevel::Warning)
        {
            rule_set.set_level(rule_name, override_value);
        }
    }
}

// analyzer/commentUtils.ts:129
fn overwrite_rules(rule_set: &mut DiagnosticRuleSet, override_rule_set: &DiagnosticRuleSet) {
    for rule_name in get_boolean_diagnostic_rules(false) {
        rule_set.set_bool(rule_name, override_rule_set.get_bool(rule_name).unwrap());
    }

    for rule_name in get_diag_level_diagnostic_rules() {
        rule_set.set_level(rule_name, override_rule_set.get_level(rule_name).unwrap());
    }
}

// analyzer/commentUtils.ts:142
fn parse_pyright_comment(
    comment_value: &str,
    comment_range: TextRange,
    is_comment_on_own_line: &dyn Fn() -> bool,
    mut rule_set: DiagnosticRuleSet,
    diagnostics: &mut Vec<CommentDiagnostic>,
) -> DiagnosticRuleSet {
    // Is this a pyright comment?
    let comment_prefix = "pyright:";
    if comment_value.starts_with(comment_prefix) {
        let operands = comment_value.js_substring(comment_prefix.js_len(), None).into_owned();

        // Handle (actual ignore) "ignore" directives.
        if operands.js_trim().starts_with("ignore") {
            return rule_set;
        }

        if !is_comment_on_own_line() {
            let diag_addendum = DiagnosticAddendum::new();
            diag_addendum.add_message(LocAddendum::pyright_comment_ignore_tip());
            let diag = CommentDiagnostic {
                message: LocMessage::pyright_comment_not_on_own_line()
                    + &diag_addendum.get_string(DEFAULT_MAX_DIAGNOSTIC_DEPTH, DEFAULT_MAX_DIAGNOSTIC_LINE_COUNT),
                range: comment_range,
            };

            diagnostics.push(diag);
        }

        let operand_list: Vec<String> = operands.js_split(",", None).into_iter().map(|s| s.into_owned()).collect();

        // If it contains a "strict" operand, replace the existing
        // diagnostic rules with their strict counterparts.
        if operand_list.iter().any(|s| s.js_trim() == STRICT_SETTING) {
            apply_strict_rules(&mut rule_set);
        } else if operand_list.iter().any(|s| s.js_trim() == STANDARD_SETTING) {
            apply_standard_rules(&mut rule_set);
        } else if operand_list.iter().any(|s| s.js_trim() == BASIC_SETTING) {
            apply_basic_rules(&mut rule_set);
        }

        let mut range_offset = 0;
        for operand in &operand_list {
            let (trimmed_operand, operand_range) = trim_text_with_range(
                operand,
                TextRange {
                    start: comment_range.start + comment_prefix.js_len() + range_offset,
                    length: operand.js_len(),
                },
            );

            rule_set = parse_pyright_operand(&trimmed_operand, operand_range, rule_set, diagnostics);
            range_offset += operand.js_len() + 1;
        }
    }

    rule_set
}

// analyzer/commentUtils.ts:197
fn parse_pyright_operand(
    operand: &str,
    operand_range: TextRange,
    mut rule_set: DiagnosticRuleSet,
    diagnostics: &mut Vec<CommentDiagnostic>,
) -> DiagnosticRuleSet {
    let operand_split: Vec<String> = operand.js_split("=", None).into_iter().map(|s| s.into_owned()).collect();
    let (trimmed_rule, rule_range) = trim_text_with_range(
        &operand_split[0],
        TextRange { start: operand_range.start, length: operand_split[0].js_len() },
    );

    // Handle basic directives "basic", "standard" and "strict".
    if operand_split.len() == 1
        && !trimmed_rule.is_empty()
        && [STRICT_SETTING, STANDARD_SETTING, BASIC_SETTING].contains(&trimmed_rule.as_str())
    {
        return rule_set;
    }

    let diag_level_rules = get_diag_level_diagnostic_rules();
    let bool_rules = get_boolean_diagnostic_rules(false);

    let rule_value = if !operand_split.is_empty() { operand_split[1..].join("=") } else { String::new() };
    let (trimmed_rule_value, rule_value_range) = trim_text_with_range(
        &rule_value,
        TextRange { start: operand_range.start + operand_split[0].js_len() + 1, length: rule_value.js_len() },
    );

    if let Some(rule) = diag_level_rules.iter().find(|r| r.as_str() == trimmed_rule) {
        match parse_diag_level(&trimmed_rule_value) {
            Some(diag_level_value) => rule_set.set_level(*rule, diag_level_value),
            None => diagnostics.push(CommentDiagnostic {
                message: LocMessage::pyright_comment_invalid_diagnostic_severity_value(),
                range: if !trimmed_rule_value.is_empty() { rule_value_range } else { rule_range },
            }),
        }
    } else if let Some(rule) = bool_rules.iter().find(|r| r.as_str() == trimmed_rule) {
        match parse_bool_setting(&trimmed_rule_value) {
            Some(bool_value) => rule_set.set_bool(*rule, bool_value),
            None => diagnostics.push(CommentDiagnostic {
                message: LocMessage::pyright_comment_invalid_diagnostic_bool_value(),
                range: if !trimmed_rule_value.is_empty() { rule_value_range } else { rule_range },
            }),
        }
    } else if !trimmed_rule.is_empty() {
        diagnostics.push(CommentDiagnostic {
            message: if !trimmed_rule_value.is_empty() {
                LocMessage::pyright_comment_unknown_diagnostic_rule().format(&[("rule", &trimmed_rule)])
            } else {
                LocMessage::pyright_comment_unknown_directive().format(&[("directive", &trimmed_rule)])
            },
            range: rule_range,
        });
    } else {
        diagnostics.push(CommentDiagnostic { message: LocMessage::pyright_comment_missing_directive(), range: rule_range });
    }

    rule_set
}

// analyzer/commentUtils.ts:266
fn parse_diag_level(value: &str) -> Option<DiagnosticLevel> {
    match value {
        "false" | "none" => Some(DiagnosticLevel::None),
        "true" | "error" => Some(DiagnosticLevel::Error),
        "warning" => Some(DiagnosticLevel::Warning),
        "information" => Some(DiagnosticLevel::Information),
        _ => None,
    }
}

// analyzer/commentUtils.ts:287
fn parse_bool_setting(value: &str) -> Option<bool> {
    match value {
        "false" => Some(false),
        "true" => Some(true),
        _ => None,
    }
}

// analyzer/commentUtils.ts:299
fn trim_text_with_range(text: &str, range: TextRange) -> (String, TextRange) {
    assert(text.js_len() == range.length, None);
    let value1 = text.js_trim_start();

    let mut updated_range = range;

    if value1 != text {
        let delta = text.js_len() - value1.js_len();
        updated_range = TextRange { start: updated_range.start + delta, length: updated_range.length - delta };
    }

    let value2 = value1.js_trim_end();
    if value2 != value1 {
        updated_range =
            TextRange { start: updated_range.start, length: updated_range.length - value1.js_len() + value2.js_len() };
    }

    assert(value2.js_len() == updated_range.length, None);
    (value2.to_string(), updated_range)
}
