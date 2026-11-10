//! The `tests/tokenizer.test.ts` cases that also check
//! `StringTokenUtils.getUnescapedString`.

// The assertions keep the shape of the TypeScript `assert.equal` calls.
#![allow(clippy::bool_assert_comparison)]

use super::tokenize;
use crate::parser::string_token_utils::{UnescapeErrorType, get_unescaped_string};
use crate::parser::tokenizer_types::{StringToken, StringTokenFlags, TokenType};

use super::IMPLICIT_TOKEN_COUNT;

#[test]
fn strings_bytes_string_with_non_ascii() {
    let results = tokenize("B\"Teßt\" b'''Teñt'''");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(
        string_token0.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Bytes
    );
    assert_eq!(unescaped_value_0.non_ascii_in_bytes, true);
    assert_eq!(string_token0.length, 7);

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(
        string_token1.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Bytes | StringTokenFlags::Triplicate
    );
    assert_eq!(unescaped_value_1.non_ascii_in_bytes, true);
    assert_eq!(string_token1.length, 11);
}

#[test]
fn strings_raw_strings_with_escapes() {
    let results = tokenize("R\"\\\"\" r\"\\\r\n\\\n\\a\"");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(
        string_token0.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Raw
    );
    assert_eq!(string_token0.length, 5);
    assert_eq!(string_token0.d().escaped_value, "\\\"");
    assert_eq!(unescaped_value_0.value, "\\\"");

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(
        string_token1.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Raw
    );
    assert_eq!(string_token1.length, 10);
    assert_eq!(string_token1.d().escaped_value, "\\\r\n\\\n\\a");
    assert_eq!(unescaped_value_1.value, "\\\r\n\\\n\\a");
}

#[test]
fn strings_special_escape_characters() {
    let results = tokenize("\"\\r\\n\\a\\v\\t\\b\\f\\\\\"");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value = get_unescaped_string(&string_token, true);
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(string_token.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token.length, 18);
    assert_eq!(unescaped_value.value, "\r\n\u{0007}\u{b}\t\u{8}\u{c}\\");

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(17), 0);
    assert_eq!(results.tokens.get_item_at_position(18), 2);

    assert_eq!(results.tokens.contains(17), true);
    assert_eq!(results.tokens.contains(18), false);
}

#[test]
fn strings_invalid_escape_characters() {
    let results = tokenize("\"\\d  \\ \"");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value = get_unescaped_string(&string_token, true);
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(string_token.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token.length, 8);
    assert_eq!(string_token.d().escaped_value, "\\d  \\ ");
    assert_eq!(unescaped_value.unescape_errors.len(), 2);
    assert_eq!(unescaped_value.unescape_errors[0].offset, 0);
    assert_eq!(unescaped_value.unescape_errors[0].length, 2);
    assert_eq!(
        unescaped_value.unescape_errors[0].error_type,
        UnescapeErrorType::InvalidEscapeSequence
    );
    assert_eq!(unescaped_value.unescape_errors[1].offset, 4);
    assert_eq!(unescaped_value.unescape_errors[1].length, 2);
    assert_eq!(
        unescaped_value.unescape_errors[1].error_type,
        UnescapeErrorType::InvalidEscapeSequence
    );
}

#[test]
fn strings_good_hex_escapes() {
    let results = tokenize("\"\\x4d\" \"\\u006b\" \"\\U0000006F\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(string_token0.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token0.length, 6);
    assert_eq!(string_token0.d().escaped_value, "\\x4d");
    assert_eq!(unescaped_value_0.value, "M");

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(string_token1.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token1.length, 8);
    assert_eq!(string_token1.d().escaped_value, "\\u006b");
    assert_eq!(unescaped_value_1.value, "k");

    let string_token2 = StringToken::cast(results.tokens.get_item_at(2)).unwrap();
    let unescaped_value_2 = get_unescaped_string(&string_token2, true);
    assert_eq!(string_token2.type_, TokenType::String);
    assert_eq!(string_token2.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token2.length, 12);
    assert_eq!(string_token2.d().escaped_value, "\\U0000006F");
    assert_eq!(unescaped_value_2.value, "o");
}

#[test]
fn strings_bad_hex_escapes() {
    let results = tokenize("\"\\x4g\" \"\\u006\" \"\\U0000006m\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(string_token0.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(unescaped_value_0.unescape_errors.len(), 1);
    assert_eq!(string_token0.length, 6);
    assert_eq!(unescaped_value_0.value, "\\x4g");

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(string_token1.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(unescaped_value_1.unescape_errors.len(), 1);
    assert_eq!(string_token1.length, 7);
    assert_eq!(unescaped_value_1.value, "\\u006");

    let string_token2 = StringToken::cast(results.tokens.get_item_at(2)).unwrap();
    let unescaped_value_2 = get_unescaped_string(&string_token2, true);
    assert_eq!(string_token2.type_, TokenType::String);
    assert_eq!(string_token2.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(unescaped_value_2.unescape_errors.len(), 1);
    assert_eq!(string_token2.length, 12);
    assert_eq!(unescaped_value_2.value, "\\U0000006m");
}

#[test]
fn strings_good_name_escapes() {
    let results = tokenize("\"\\N{caret escape blah}\" \"a\\N{A9}a\"");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(
        string_token0.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::NamedUnicodeEscape
    );
    assert_eq!(string_token0.length, 23);
    assert_eq!(string_token0.d().escaped_value, "\\N{caret escape blah}");
    assert_eq!(unescaped_value_0.value, "-");

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(
        string_token1.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::NamedUnicodeEscape
    );
    assert_eq!(string_token1.length, 10);
    assert_eq!(string_token1.d().escaped_value, "a\\N{A9}a");
    assert_eq!(unescaped_value_1.value, "a-a");
}

#[test]
fn strings_bad_name_escapes() {
    let results = tokenize("\"\\N{caret\" \"\\N{.A9}\"");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    let unescaped_value_0 = get_unescaped_string(&string_token0, true);
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(
        string_token0.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::NamedUnicodeEscape
    );
    assert_eq!(unescaped_value_0.unescape_errors.len(), 1);
    assert_eq!(string_token0.length, 10);
    assert_eq!(string_token0.d().escaped_value, "\\N{caret");
    assert_eq!(unescaped_value_0.value, "\\N{caret");

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    let unescaped_value_1 = get_unescaped_string(&string_token1, true);
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(
        string_token1.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::NamedUnicodeEscape
    );
    assert_eq!(unescaped_value_1.unescape_errors.len(), 1);
    assert_eq!(string_token1.length, 9);
    assert_eq!(string_token1.d().escaped_value, "\\N{.A9}");
    assert_eq!(unescaped_value_1.value, "\\N{.A9}");
}
