//! Port of `packages/pyright-internal/src/tests/tokenizer.test.ts`.

// The assertions keep the shape of the TypeScript `assert.equal` calls.
#![allow(clippy::bool_assert_comparison, clippy::identity_op)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{Tokenizer, TokenizerOutput};
use crate::parser::tokenizer_types::{
    CommentType, DedentToken, FStringEndToken, FStringMiddleToken, FStringStartToken,
    IdentifierToken, IndentToken, NewLineToken, NewLineType, NumberToken, OperatorToken,
    OperatorType, StringToken, StringTokenFlags, TokenType,
};
use jsrt::{JsStr, NumberValue};

// tests/tokenizer.test.ts:36
const IMPLICIT_TOKEN_COUNT: i32 = 2;
// tests/tokenizer.test.ts:37
const IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE: i32 = 1;

/// `new Tokenizer().tokenize(text)`.
fn tokenize(text: &str) -> TokenizerOutput {
    let text: Vec<u16> = text.encode_utf16().collect();
    Tokenizer::new().tokenize(&text, None, None, 0, false)
}

#[test]
fn empty() {
    let results = tokenize("");
    assert_eq!(results.tokens.count(), 0 + IMPLICIT_TOKEN_COUNT);
    assert_eq!(results.tokens.length(), 0);
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::EndOfStream);

    assert_eq!(results.tokens.get_item_at_position(-1), -1);
    assert_eq!(results.tokens.get_item_at_position(2), -1);

    assert!(catch_unwind(AssertUnwindSafe(|| results.tokens.get_item_at(-1))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| results.tokens.get_item_at(10))).is_err());

    assert_eq!(results.tokens.contains(-1), false);
    assert_eq!(results.tokens.contains(2), false);
}

#[test]
fn new_lines() {
    let results = tokenize("\na\r\nb\r");
    assert_eq!(
        results.tokens.count(),
        5 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::LineFeed
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::CarriageReturnLineFeed
    );
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::CarriageReturn
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::EndOfStream);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 1);
    assert_eq!(results.tokens.get_item_at_position(2), 2);
    assert_eq!(results.tokens.get_item_at_position(3), 2);
    assert_eq!(results.tokens.get_item_at_position(4), 3);
    assert_eq!(results.tokens.get_item_at_position(5), 4);
    assert_eq!(results.tokens.get_item_at_position(6), 5);

    assert_eq!(results.tokens.contains(5), true);
    assert_eq!(results.tokens.contains(6), false);
}

#[test]
fn invalid_with_new_line() {
    let results = tokenize("\\\\\r\n\\aaa \t\u{c}\n");
    assert_eq!(
        results.tokens.count(),
        4 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Invalid);
    assert_eq!(results.tokens.get_item_at(0).length, 2);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::CarriageReturnLineFeed
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Invalid);
    assert_eq!(results.tokens.get_item_at(2).length, 4);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(3))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::LineFeed
    );
}

#[test]
fn invalid_indent() {
    let results = tokenize("\tpass\n");
    assert_eq!(
        results.tokens.count(),
        4 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Indent);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Keyword);
}

#[test]
fn paren_new_lines() {
    let results = tokenize("\n(\n(\n)\n)\n)\n");
    assert_eq!(
        results.tokens.count(),
        8 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(
        results.tokens.get_item_at(3).type_,
        TokenType::CloseParenthesis
    );
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseParenthesis
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::NewLine);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseParenthesis
    );

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 1);
    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(3), 2);
    assert_eq!(results.tokens.get_item_at_position(4), 2);
    assert_eq!(results.tokens.get_item_at_position(5), 3);
    assert_eq!(results.tokens.get_item_at_position(6), 3);
    assert_eq!(results.tokens.get_item_at_position(7), 4);
    assert_eq!(results.tokens.get_item_at_position(8), 5);
    assert_eq!(results.tokens.get_item_at_position(9), 6);
    assert_eq!(results.tokens.get_item_at_position(10), 7);
    assert_eq!(results.tokens.get_item_at_position(11), 8);

    assert_eq!(results.tokens.contains(10), true);
    assert_eq!(results.tokens.contains(11), false);
}

#[test]
fn brace_new_lines() {
    let results = tokenize("\n{\n{\n}\n}\n}\n");
    assert_eq!(
        results.tokens.count(),
        8 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(3).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::NewLine);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseCurlyBrace
    );

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 1);
    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(3), 2);
    assert_eq!(results.tokens.get_item_at_position(4), 2);
    assert_eq!(results.tokens.get_item_at_position(5), 3);
    assert_eq!(results.tokens.get_item_at_position(6), 3);
    assert_eq!(results.tokens.get_item_at_position(7), 4);
    assert_eq!(results.tokens.get_item_at_position(8), 5);
    assert_eq!(results.tokens.get_item_at_position(9), 6);
    assert_eq!(results.tokens.get_item_at_position(10), 7);
    assert_eq!(results.tokens.get_item_at_position(11), 8);

    assert_eq!(results.tokens.contains(10), true);
    assert_eq!(results.tokens.contains(11), false);
}

#[test]
fn bracket_new_lines() {
    let results = tokenize("\n[\n[\n]\n]\n]\n");
    assert_eq!(
        results.tokens.count(),
        8 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::OpenBracket);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::OpenBracket);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::CloseBracket);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::CloseBracket);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::CloseBracket);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 1);
    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(3), 2);
    assert_eq!(results.tokens.get_item_at_position(4), 2);
    assert_eq!(results.tokens.get_item_at_position(5), 3);
    assert_eq!(results.tokens.get_item_at_position(6), 3);
    assert_eq!(results.tokens.get_item_at_position(7), 4);
    assert_eq!(results.tokens.get_item_at_position(8), 5);
    assert_eq!(results.tokens.get_item_at_position(9), 6);
    assert_eq!(results.tokens.get_item_at_position(10), 7);
    assert_eq!(results.tokens.get_item_at_position(11), 8);

    assert_eq!(results.tokens.contains(10), true);
    assert_eq!(results.tokens.contains(11), false);
}

#[test]
fn new_lines_with_white_space() {
    let results = tokenize("  \na   \r\nb  \rc");
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(0).length, 1);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::LineFeed
    );

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::CarriageReturnLineFeed
    );
    assert_eq!(results.tokens.get_item_at(2).length, 2);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::CarriageReturn
    );
    assert_eq!(results.tokens.get_item_at(4).length, 1);

    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::NewLine);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(6))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::Implied
    );
    assert_eq!(results.tokens.get_item_at(6).length, 0);

    assert_eq!(results.tokens.get_item_at_position(0), -1);
    assert_eq!(results.tokens.get_item_at_position(1), -1);
    assert_eq!(results.tokens.get_item_at_position(2), 0);
    assert_eq!(results.tokens.get_item_at_position(3), 1);
    assert_eq!(results.tokens.get_item_at_position(6), 1);
    assert_eq!(results.tokens.get_item_at_position(7), 2);
    assert_eq!(results.tokens.get_item_at_position(8), 2);
    assert_eq!(results.tokens.get_item_at_position(9), 3);
    assert_eq!(results.tokens.get_item_at_position(11), 3);
    assert_eq!(results.tokens.get_item_at_position(12), 4);
    assert_eq!(results.tokens.get_item_at_position(13), 5);
    assert_eq!(results.tokens.get_item_at_position(14), 7);

    assert_eq!(results.tokens.contains(13), true);
    assert_eq!(results.tokens.contains(14), false);
}

#[test]
fn new_line_eliding() {
    let results = tokenize("\n\r\n\r");
    assert_eq!(
        results.tokens.count(),
        1 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(0).length, 1);
    assert_eq!(
        NewLineToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .new_line_type,
        NewLineType::LineFeed
    );

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(3), 0);
    assert_eq!(results.tokens.get_item_at_position(4), 1);

    assert_eq!(results.tokens.contains(3), true);
    assert_eq!(results.tokens.contains(4), false);
}

#[test]
fn line_continuation() {
    let results = tokenize("foo  \\\na   \\\r\nb  \\\rc  \\ \n # Comment \\\n");
    assert_eq!(
        results.tokens.count(),
        6 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Invalid);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::NewLine);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(6), 0);
    assert_eq!(results.tokens.get_item_at_position(7), 1);
    assert_eq!(results.tokens.get_item_at_position(13), 1);
    assert_eq!(results.tokens.get_item_at_position(14), 2);
    assert_eq!(results.tokens.get_item_at_position(18), 2);
    assert_eq!(results.tokens.get_item_at_position(19), 3);
    assert_eq!(results.tokens.get_item_at_position(21), 3);
    assert_eq!(results.tokens.get_item_at_position(22), 4);
    assert_eq!(results.tokens.get_item_at_position(23), 4);
    assert_eq!(results.tokens.get_item_at_position(24), 5);
    assert_eq!(results.tokens.get_item_at_position(37), 5);
    assert_eq!(results.tokens.get_item_at_position(38), 6);

    assert_eq!(results.tokens.contains(37), true);
    assert_eq!(results.tokens.contains(38), false);
}

#[test]
fn dots() {
    let results = tokenize(". .. ... ....");
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Dot);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Dot);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Dot);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Ellipsis);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Ellipsis);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Dot);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 0);

    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(3), 2);
    assert_eq!(results.tokens.get_item_at_position(4), 2);

    assert_eq!(results.tokens.get_item_at_position(5), 3);
    assert_eq!(results.tokens.get_item_at_position(8), 3);

    assert_eq!(results.tokens.get_item_at_position(9), 4);
    assert_eq!(results.tokens.get_item_at_position(11), 4);

    assert_eq!(results.tokens.get_item_at_position(12), 5);
    assert_eq!(results.tokens.get_item_at_position(13), 7);

    assert_eq!(results.tokens.contains(12), true);
    assert_eq!(results.tokens.contains(13), false);
}

#[test]
fn punctuation_tokens() {
    let results = tokenize(":;,()[]{}->");
    assert_eq!(results.tokens.count(), 10 + IMPLICIT_TOKEN_COUNT);
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Colon);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Semicolon);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Comma);
    assert_eq!(
        results.tokens.get_item_at(3).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseParenthesis
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::OpenBracket);
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::CloseBracket);
    assert_eq!(
        results.tokens.get_item_at(7).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(8).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(9).type_, TokenType::Arrow);
}

#[test]
fn indent_dedent() {
    let results = tokenize(concat!(
        "test\n",
        "  i1\n",
        "  i2  # \n",
        "       # \n",
        "  \ti3\n",
        "\ti4\n",
        " i1"
    ));
    assert_eq!(results.tokens.count(), 16 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Indent);
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .indent_amount,
        2
    );
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .length,
        2
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Indent);
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .d()
            .indent_amount,
        8
    );
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .length,
        3
    );
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(9).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::Indent);
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(10))
            .unwrap()
            .d()
            .is_indent_ambiguous,
        true
    );
    assert_eq!(
        IndentToken::cast(results.tokens.get_item_at(10))
            .unwrap()
            .length,
        1
    );
    assert_eq!(results.tokens.get_item_at(11).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(12).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(13).type_, TokenType::Dedent);
    assert_eq!(
        DedentToken::cast(results.tokens.get_item_at(13))
            .unwrap()
            .d()
            .indent_amount,
        2
    );
    assert_eq!(
        DedentToken::cast(results.tokens.get_item_at(13))
            .unwrap()
            .d()
            .matches_indent,
        true
    );
    assert_eq!(results.tokens.get_item_at(14).type_, TokenType::Dedent);
    assert_eq!(
        DedentToken::cast(results.tokens.get_item_at(14))
            .unwrap()
            .d()
            .indent_amount,
        1
    );
    assert_eq!(
        DedentToken::cast(results.tokens.get_item_at(14))
            .unwrap()
            .d()
            .matches_indent,
        false
    );
    assert_eq!(results.tokens.get_item_at(15).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(16).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(17).type_, TokenType::EndOfStream);
}

#[test]
fn indent_dedent_paren() {
    let results = tokenize("test (\n  i1\n       )\n  foo");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);

    // Test that indent and dedent tokens are suppressed within
    // a parenthetical clause.
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(3).type_,
        TokenType::CloseParenthesis
    );
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Indent);
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Dedent);
    assert_eq!(results.tokens.get_item_at(9).type_, TokenType::EndOfStream);
}

#[test]
fn strings_simple() {
    let results = tokenize(" \"a\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Indent);
    let string_token = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(string_token.length, 3);
    assert_eq!(string_token.d().escaped_value, "a");
    assert_eq!(string_token.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::NewLine);
}

#[test]
fn strings_unclosed() {
    let results = tokenize(" \"string\" \"\"\"line1\n#line2\"\"\"\t'un#closed");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);

    let ranges = [[1, 8], [10, 18], [29, 10]];
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Indent);
    for i in 0..ranges.len() {
        assert_eq!(results.tokens.get_item_at(i as i32 + 1).start, ranges[i][0]);
        assert_eq!(
            results.tokens.get_item_at(i as i32 + 1).length,
            ranges[i][1]
        );
        assert_eq!(
            results.tokens.get_item_at(i as i32 + 1).type_,
            TokenType::String
        );
    }
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Dedent);
}

#[test]
fn strings_escaped_across_multiple_lines() {
    let results = tokenize(" \"a\\\nb\" 'c\\\r\nb'");
    assert_eq!(results.tokens.count(), 4 + IMPLICIT_TOKEN_COUNT);

    let ranges = [[1, 6], [8, 7]];
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Indent);
    for i in 0..ranges.len() {
        assert_eq!(results.tokens.get_item_at(i as i32 + 1).start, ranges[i][0]);
        assert_eq!(
            results.tokens.get_item_at(i as i32 + 1).length,
            ranges[i][1]
        );
        assert_eq!(
            results.tokens.get_item_at(i as i32 + 1).type_,
            TokenType::String
        );
    }
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::EndOfStream);
}

#[test]
fn strings_block_next_to_regular_double_quoted() {
    let results = tokenize("\"string\"\"\"\"s2\"\"\"");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let ranges = [[0, 8], [8, 8]];
    for i in 0..ranges.len() {
        assert_eq!(results.tokens.get_item_at(i as i32).start, ranges[i][0]);
        assert_eq!(results.tokens.get_item_at(i as i32).length, ranges[i][1]);
        assert_eq!(
            results.tokens.get_item_at(i as i32).type_,
            TokenType::String
        );
    }
}

#[test]
fn strings_block_next_to_block_double_quoted() {
    let results = tokenize("\"\"\"\"\"\"\"\"");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let ranges = [[0, 6], [6, 2]];
    for i in 0..ranges.len() {
        assert_eq!(results.tokens.get_item_at(i as i32).start, ranges[i][0]);
        assert_eq!(results.tokens.get_item_at(i as i32).length, ranges[i][1]);
        assert_eq!(
            results.tokens.get_item_at(i as i32).type_,
            TokenType::String
        );
    }
}

#[test]
fn strings_unclosed_sequence_of_quotes() {
    let results = tokenize("\"\"\"\"\"");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let ranges = [[0, 5]];
    for i in 0..ranges.len() {
        assert_eq!(results.tokens.get_item_at(i as i32).start, ranges[i][0]);
        assert_eq!(results.tokens.get_item_at(i as i32).length, ranges[i][1]);
        assert_eq!(
            results.tokens.get_item_at(i as i32).type_,
            TokenType::String
        );
    }
}

#[test]
fn strings_single_quote_escape() {
    let results = tokenize("'\\'quoted\\''");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(string_token.d().flags, StringTokenFlags::SingleQuote);
    assert_eq!(string_token.length, 12);
    assert_eq!(string_token.d().prefix_length, 0);
    assert_eq!(string_token.d().escaped_value, "\\'quoted\\'");
}

#[test]
fn strings_double_quote_escape() {
    let results = tokenize("\"\\\"quoted\\\"\"");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(string_token.d().flags, StringTokenFlags::DoubleQuote);
    assert_eq!(string_token.length, 12);
    assert_eq!(string_token.d().escaped_value, "\\\"quoted\\\"");
}

#[test]
fn strings_triplicate_double_quote_escape() {
    let results = tokenize("\"\"\"\\\"quoted\\\"\"\"\"");
    assert_eq!(results.tokens.count(), 1 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(
        string_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Triplicate
    );
    assert_eq!(string_token.length, 16);
    assert_eq!(string_token.d().escaped_value, "\\\"quoted\\\"");
}

#[test]
fn strings_single_quoted_f_string() {
    let results = tokenize("a+f'quoted'");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Operator);

    let f_string_start_token = FStringStartToken::cast(results.tokens.get_item_at(2)).unwrap();
    assert_eq!(f_string_start_token.type_, TokenType::FStringStart);
    assert_eq!(
        f_string_start_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_start_token.length, 2);

    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(3)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(
        f_string_middle_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_middle_token.length, 6);
    assert_eq!(f_string_middle_token.d().escaped_value, "quoted");

    let f_string_end_token = FStringEndToken::cast(results.tokens.get_item_at(4)).unwrap();
    assert_eq!(f_string_end_token.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_end_token.length, 1);
}

#[test]
fn strings_double_quoted_f_string() {
    let results = tokenize("x(1,f\"quoted\")");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Comma);
    assert_eq!(
        results.tokens.get_item_at(7).type_,
        TokenType::CloseParenthesis
    );

    let f_string_start_token = FStringStartToken::cast(results.tokens.get_item_at(4)).unwrap();
    assert_eq!(f_string_start_token.type_, TokenType::FStringStart);
    assert_eq!(
        f_string_start_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_start_token.length, 2);

    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(5)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(
        f_string_middle_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_middle_token.length, 6);
    assert_eq!(f_string_middle_token.d().escaped_value, "quoted");

    let f_string_end_token = FStringEndToken::cast(results.tokens.get_item_at(6)).unwrap();
    assert_eq!(f_string_end_token.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_end_token.length, 1);
}

#[test]
fn strings_single_quoted_multiline_f_string() {
    let results = tokenize("f'''quoted'''");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let f_string_start_token = FStringStartToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(f_string_start_token.type_, TokenType::FStringStart);
    assert_eq!(
        f_string_start_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_start_token.length, 4);

    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(
        f_string_middle_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_middle_token.length, 6);
    assert_eq!(f_string_middle_token.d().escaped_value, "quoted");

    let f_string_end_token = FStringEndToken::cast(results.tokens.get_item_at(2)).unwrap();
    assert_eq!(f_string_end_token.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_end_token.length, 3);
}

#[test]
fn strings_double_quoted_multiline_f_string() {
    let results = tokenize("f\"\"\"quoted \"\"\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let f_string_start_token = FStringStartToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(f_string_start_token.type_, TokenType::FStringStart);
    assert_eq!(
        f_string_start_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_start_token.length, 4);

    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(
        f_string_middle_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_middle_token.length, 7);
    assert_eq!(f_string_middle_token.d().escaped_value, "quoted ");

    let f_string_end_token = FStringEndToken::cast(results.tokens.get_item_at(2)).unwrap();
    assert_eq!(f_string_end_token.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Triplicate | StringTokenFlags::Format
    );
    assert_eq!(f_string_end_token.length, 3);
}

#[test]
fn strings_f_string_with_single_right_brace() {
    let results = tokenize("f'hello}'");
    assert_eq!(results.tokens.count(), 4 + IMPLICIT_TOKEN_COUNT);

    let f_string_start_token = FStringStartToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(f_string_start_token.type_, TokenType::FStringStart);
    assert_eq!(f_string_start_token.length, 2);
    assert_eq!(
        f_string_start_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Format
    );

    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(f_string_middle_token.length, 5);
    assert_eq!(
        f_string_middle_token.d().flags,
        StringTokenFlags::SingleQuote
            | StringTokenFlags::Format
            | StringTokenFlags::ReplacementFieldEnd
    );

    let brace_token = results.tokens.get_item_at(2).type_;
    assert_eq!(brace_token, TokenType::CloseCurlyBrace);

    let f_string_end_token = FStringEndToken::cast(results.tokens.get_item_at(3)).unwrap();
    assert_eq!(f_string_end_token.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Format
    );
    assert_eq!(f_string_end_token.length, 1);
}

#[test]
fn strings_f_string_with_backslash_escape() {
    let results = tokenize("f'\\\\'");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    let f_string_middle_token = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(f_string_middle_token.type_, TokenType::FStringMiddle);
    assert_eq!(f_string_middle_token.length, 2);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_new_line_escape() {
    let results = tokenize("f'x \\\ny'");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_escape_in_expression() {
    let results = tokenize("f'hello { \"\\t\" }'");
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::String);
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_escape_in_format_string_1() {
    let results = tokenize("f'he\\{ 1 }lo'");
    assert_eq!(results.tokens.count(), 7 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);

    let middle_fstring = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(middle_fstring.type_, TokenType::FStringMiddle);
    assert_eq!(middle_fstring.d().escaped_value.js_len(), 3);

    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Number);
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_escape_in_format_string_2() {
    let results = tokenize("f\"'{{\\\"{0}\\\": {0}}}'\"");
    assert_eq!(results.tokens.count(), 11 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);

    let middle_fstring = FStringMiddleToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(middle_fstring.type_, TokenType::FStringMiddle);
    assert_eq!(middle_fstring.d().escaped_value.js_len(), 5);

    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Number);
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Number);
    assert_eq!(
        results.tokens.get_item_at(8).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(9).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_double_brace() {
    let results = tokenize("f\"hello {{{0==0}}}\"");
    assert_eq!(results.tokens.count(), 9 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Number);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(7).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_walrus_operator() {
    let results = tokenize("f\"{(x:=0)}\"");
    assert_eq!(results.tokens.count(), 9 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenParenthesis
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Number);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseParenthesis
    );
    assert_eq!(
        results.tokens.get_item_at(7).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_single_right_brace_2() {
    let results = tokenize("f\"}\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_comment() {
    let results = tokenize("f'''hello{\nx # comment\n}'''");
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    let close_brace_token = results.tokens.get_item_at(4);
    assert_eq!(close_brace_token.type_, TokenType::CloseCurlyBrace);
    let comments = close_brace_token.comments().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].type_, CommentType::Regular);
    assert_eq!(comments[0].value, " comment");
    assert_eq!(comments[0].start, 14);
    assert_eq!(comments[0].length, 8);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_unterminated_expression() {
    let results = tokenize("f'hello { a'");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);

    let f_string_end = FStringEndToken::cast(results.tokens.get_item_at(4)).unwrap();
    assert_eq!(f_string_end.type_, TokenType::FStringEnd);
    assert_eq!(
        f_string_end.d().flags,
        StringTokenFlags::Format | StringTokenFlags::SingleQuote
    );
}

#[test]
fn strings_f_string_with_replacement_field() {
    let results = tokenize("f'hello { a + b}'");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_format_specifier() {
    let results = tokenize("f'hello { a ! b}'");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::ExclamationMark
    );
    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::FStringEnd);
}

#[test]
fn strings_f_string_with_debug_format_specifier() {
    let results = tokenize("f'hello { a =}'");
    assert_eq!(results.tokens.count(), 7 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Operator);
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::FStringEnd);
}

#[test]
fn strings_nested_f_string() {
    let results = tokenize("f'{f'{a}'}'");
    assert_eq!(results.tokens.count(), 9 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(3).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::FStringEnd);
    assert_eq!(
        results.tokens.get_item_at(7).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::FStringEnd);
}

#[test]
fn strings_nested_f_string_formats_1() {
    let results = tokenize("f'{a:x{{b}+:x{c}+}}'");
    assert_eq!(results.tokens.count(), 19 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Colon);
    assert_eq!(
        results.tokens.get_item_at(4).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(8).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(9).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::Colon);
    assert_eq!(
        results.tokens.get_item_at(11).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(12).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(13).type_, TokenType::Identifier);
    assert_eq!(
        results.tokens.get_item_at(14).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(15).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(16).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(17).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(18).type_, TokenType::FStringEnd);
}

#[test]
fn strings_nested_f_string_formats_2() {
    let results = tokenize("f'hi{'x':*^{8:{'':}}0}'");
    assert_eq!(results.tokens.count(), 17 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::FStringStart);
    assert_eq!(
        results.tokens.get_item_at(1).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(2).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::String);
    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Colon);
    assert_eq!(
        results.tokens.get_item_at(5).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(6).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Colon);
    assert_eq!(
        results.tokens.get_item_at(9).type_,
        TokenType::OpenCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::String);
    assert_eq!(results.tokens.get_item_at(11).type_, TokenType::Colon);
    assert_eq!(
        results.tokens.get_item_at(12).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(13).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(
        results.tokens.get_item_at(14).type_,
        TokenType::FStringMiddle
    );
    assert_eq!(
        results.tokens.get_item_at(15).type_,
        TokenType::CloseCurlyBrace
    );
    assert_eq!(results.tokens.get_item_at(16).type_, TokenType::FStringEnd);
}

#[test]
fn strings_escape_at_the_end_of_single_quoted_string() {
    let results = tokenize("'quoted\\'\nx");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(
        string_token.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Unterminated
    );
    assert_eq!(string_token.length, 9);
    assert_eq!(string_token.d().escaped_value, "quoted\\'");

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(8), 0);
    assert_eq!(results.tokens.get_item_at_position(9), 1);
    assert_eq!(results.tokens.get_item_at_position(10), 2);
    assert_eq!(results.tokens.get_item_at_position(11), 4);

    assert_eq!(results.tokens.contains(10), true);
    assert_eq!(results.tokens.contains(11), false);
}

#[test]
fn strings_escape_at_the_end_of_double_quoted_string() {
    let results = tokenize("\"quoted\\\"\nx");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(
        string_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Unterminated
    );
    assert_eq!(string_token.length, 9);
    assert_eq!(string_token.d().escaped_value, "quoted\\\"");

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
}

#[test]
fn strings_b_u_r_string() {
    let results = tokenize("b\"b\" U'u' bR\"br\"");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token0 = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token0.type_, TokenType::String);
    assert_eq!(
        string_token0.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Bytes
    );
    assert_eq!(string_token0.length, 4);
    assert_eq!(string_token0.d().escaped_value, "b");
    assert_eq!(string_token0.d().prefix_length, 1);

    let string_token1 = StringToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(string_token1.type_, TokenType::String);
    assert_eq!(
        string_token1.d().flags,
        StringTokenFlags::SingleQuote | StringTokenFlags::Unicode
    );
    assert_eq!(string_token1.length, 4);
    assert_eq!(string_token1.d().escaped_value, "u");
    assert_eq!(string_token1.d().prefix_length, 1);

    let string_token2 = StringToken::cast(results.tokens.get_item_at(2)).unwrap();
    assert_eq!(string_token2.type_, TokenType::String);
    assert_eq!(
        string_token2.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Bytes | StringTokenFlags::Raw
    );
    assert_eq!(string_token2.length, 6);
    assert_eq!(string_token2.d().escaped_value, "br");
    assert_eq!(string_token2.d().prefix_length, 2);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(4), 0);
    assert_eq!(results.tokens.get_item_at_position(5), 1);
    assert_eq!(results.tokens.get_item_at_position(9), 1);
    assert_eq!(results.tokens.get_item_at_position(10), 2);
    assert_eq!(results.tokens.get_item_at_position(15), 2);

    assert_eq!(results.tokens.contains(15), true);
    assert_eq!(results.tokens.contains(16), false);
}

#[test]
fn strings_escape_at_the_end_of_double_quoted_string_2() {
    let results = tokenize("\"quoted\\\"\nx");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    let string_token = StringToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(string_token.type_, TokenType::String);
    assert_eq!(
        string_token.d().flags,
        StringTokenFlags::DoubleQuote | StringTokenFlags::Unterminated
    );
    assert_eq!(string_token.length, 9);
    assert_eq!(string_token.d().escaped_value, "quoted\\\"");

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::NewLine);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
}

#[test]
fn comments() {
    let results = tokenize(" #co\"\"\"mment1\n\t\n#x'y2 ");
    assert_eq!(
        results.tokens.count(),
        1 + IMPLICIT_TOKEN_COUNT_NO_IMPLICIT_NEW_LINE
    );
    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::NewLine);
}

#[test]
fn period_to_operator_token() {
    let results = tokenize("x.y");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Dot);
    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
}

#[test]
fn to_operator_token() {
    let results = tokenize("@x");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Identifier);
}

#[test]
fn unknown_token() {
    let results = tokenize("`$");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Backtick);
    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Invalid);
}

#[test]
fn hex_number() {
    let results = tokenize("1 0X2 0xFe_Ab 0x");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(0).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(1))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(1).length, 3);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(2))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(2).length, 7);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0xfeab))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(3).length, 1);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).length, 1);
}

#[test]
fn binary_number() {
    let results = tokenize("1 0B1 0b010 0b3 0b");
    assert_eq!(results.tokens.count(), 7 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(0).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(1))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(1).length, 3);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(1))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(2).length, 5);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(2))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(3).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(3))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(3))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(4).length, 2);

    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(5).length, 1);

    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(6).length, 1);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 0);
    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(5), 1);
    assert_eq!(results.tokens.get_item_at_position(6), 2);
    assert_eq!(results.tokens.get_item_at_position(11), 2);
    assert_eq!(results.tokens.get_item_at_position(12), 3);
    assert_eq!(results.tokens.get_item_at_position(13), 4);
    assert_eq!(results.tokens.get_item_at_position(15), 4);
    assert_eq!(results.tokens.get_item_at_position(16), 5);
    assert_eq!(results.tokens.get_item_at_position(17), 6);
    assert_eq!(results.tokens.get_item_at_position(18), 8);

    assert_eq!(results.tokens.contains(17), true);
    assert_eq!(results.tokens.contains(18), false);
}

#[test]
fn octal_number() {
    let results = tokenize("1 0o4 0O0_7_7 -0o200 0o9 0oO");
    assert_eq!(results.tokens.count(), 9 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(0).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(1))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(1).length, 3);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(4))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(2).length, 7);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0o77))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(3).length, 1);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).length, 5);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0o200))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(5).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(5))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(5))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(6).length, 2);
    assert_eq!(
        IdentifierToken::cast(results.tokens.get_item_at(6))
            .unwrap()
            .d()
            .value.as_ref(),
        "o9"
    );

    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(7).length, 1);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(0))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(8).length, 2);
    assert_eq!(
        IdentifierToken::cast(results.tokens.get_item_at(8))
            .unwrap()
            .d()
            .value.as_ref(),
        "oO"
    );
}

#[test]
fn decimal_number() {
    let results = tokenize("-2147483647 ++2147483647");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(0).length, 1);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(1).length, 10);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(2147483647))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(2).length, 1);

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(3).length, 1);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).length, 10);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(2147483647))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .is_integer,
        true
    );

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 1);
    assert_eq!(results.tokens.get_item_at_position(11), 1);
    assert_eq!(results.tokens.get_item_at_position(12), 2);
    assert_eq!(results.tokens.get_item_at_position(13), 3);
    assert_eq!(results.tokens.get_item_at_position(14), 4);
    assert_eq!(results.tokens.get_item_at_position(23), 4);
    assert_eq!(results.tokens.get_item_at_position(24), 6);

    assert_eq!(results.tokens.contains(23), true);
    assert_eq!(results.tokens.contains(24), false);
}

#[test]
fn decimal_number_operator() {
    let results = tokenize("a[: -1]");
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).length, 1);
}

#[test]
fn floating_point_number() {
    let results = tokenize("3.0 .2 ++.3e+12 --.4e1 1e-4 0.01 01.0");
    assert_eq!(results.tokens.count(), 11 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(f64::from(3))
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(0).length, 3);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(0.2)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(1).length, 2);

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(2).length, 1);

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(3).length, 1);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(0.3e12)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(4))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(4).length, 6);

    assert_eq!(results.tokens.get_item_at(5).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(5).length, 1);

    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(6).length, 1);

    assert_eq!(results.tokens.get_item_at(7).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(0.4e1)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(7))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(7).length, 4);

    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(8))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(1e-4)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(8))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(8).length, 4);

    assert_eq!(results.tokens.get_item_at(9).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(9))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(0.01)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(9))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(9).length, 4);

    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(10))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(1.0)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(10))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(10).length, 4);
}

#[test]
fn floating_point_numbers_with_parens() {
    let results = tokenize("(3.0) (.2) (+.3e+12, .4e1; 0)");
    assert_eq!(results.tokens.count(), 14 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(1).length, 3);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).length, 2);

    assert_eq!(results.tokens.get_item_at(8).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(8).length, 6);

    assert_eq!(results.tokens.get_item_at(10).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(10).length, 4);

    assert_eq!(results.tokens.get_item_at(12).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(12).length, 1);
}

#[test]
fn floating_point_numbers_with_operators() {
    let results = tokenize("88.9/100.0*4.0-2.0,");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(0).length, 4);

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(2).length, 5);

    assert_eq!(results.tokens.get_item_at(4).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(4).length, 3);

    assert_eq!(results.tokens.get_item_at(6).type_, TokenType::Number);
    assert_eq!(results.tokens.get_item_at(6).length, 3);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(3), 0);
    assert_eq!(results.tokens.get_item_at_position(4), 1);
    assert_eq!(results.tokens.get_item_at_position(5), 2);
    assert_eq!(results.tokens.get_item_at_position(9), 2);
    assert_eq!(results.tokens.get_item_at_position(10), 3);
    assert_eq!(results.tokens.get_item_at_position(11), 4);
    assert_eq!(results.tokens.get_item_at_position(13), 4);
    assert_eq!(results.tokens.get_item_at_position(14), 5);
    assert_eq!(results.tokens.get_item_at_position(15), 6);
    assert_eq!(results.tokens.get_item_at_position(17), 6);
    assert_eq!(results.tokens.get_item_at_position(18), 7);
    assert_eq!(results.tokens.get_item_at_position(19), 9);

    assert_eq!(results.tokens.contains(18), true);
    assert_eq!(results.tokens.contains(19), false);
}

#[test]
fn floating_point_numbers_with_underscores() {
    let results = tokenize("20_000.0 1_000.5 1_0.3e1_0");
    assert_eq!(results.tokens.count(), 3 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(20000.0)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(0))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(0).length, 8);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(1000.5)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(1))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(1).length, 7);

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Number);
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .value,
        NumberValue::Number(10.3e10)
    );
    assert_eq!(
        NumberToken::cast(results.tokens.get_item_at(2))
            .unwrap()
            .d()
            .is_integer,
        false
    );
    assert_eq!(results.tokens.get_item_at(2).length, 9);
}

#[test]
fn imaginary_numbers() {
    let results = tokenize("88.9j/100.0J*4.0e-5j-2.0j,");
    assert_eq!(results.tokens.count(), 8 + IMPLICIT_TOKEN_COUNT);

    let token0 = results.tokens.get_item_at(0);
    assert_eq!(token0.type_, TokenType::Number);
    assert_eq!(token0.length, 5);
    assert!(NumberToken::cast(token0).unwrap().d().is_imaginary);

    let token2 = results.tokens.get_item_at(2);
    assert_eq!(token2.type_, TokenType::Number);
    assert_eq!(token2.length, 6);
    assert!(NumberToken::cast(token2).unwrap().d().is_imaginary);

    let token4 = results.tokens.get_item_at(4);
    assert_eq!(token4.type_, TokenType::Number);
    assert_eq!(token4.length, 7);
    assert!(NumberToken::cast(token4).unwrap().d().is_imaginary);

    let token6 = results.tokens.get_item_at(6);
    assert_eq!(token6.type_, TokenType::Number);
    assert_eq!(token6.length, 4);
    assert!(NumberToken::cast(token6).unwrap().d().is_imaginary);
}

#[test]
fn underscore_numbers() {
    let results =
        tokenize("1_0_0_0 0_0 .5_00_3e-4 0xC__A_FE_F00D 10_000_000.0 0b_0011_1111_0100_1110");
    let lengths = [7, 3, 10, 14, 12, 22];
    let is_integers = [true, true, false, true, false, true];
    assert_eq!(results.tokens.count(), 6 + IMPLICIT_TOKEN_COUNT);

    for i in 0..lengths.len() {
        assert_eq!(
            results.tokens.get_item_at(i as i32).type_,
            TokenType::Number
        );
        assert_eq!(results.tokens.get_item_at(i as i32).length, lengths[i]);
        assert_eq!(
            NumberToken::cast(results.tokens.get_item_at(i as i32))
                .unwrap()
                .d()
                .is_integer,
            is_integers[i]
        );
    }
}

#[test]
fn simple_expression_leading_minus() {
    let results = tokenize("x == -y");
    assert_eq!(results.tokens.count(), 4 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(0).length, 1);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(1).length, 2);

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Operator);
    assert_eq!(results.tokens.get_item_at(2).length, 1);

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(3).length, 1);
}

#[test]
fn operators() {
    let text = String::from("< << <<= ")
        + "== != > >> >>= >= <="
        + "+ - ~ %"
        + "* ** / // /= //="
        + "*= += -= %= **= "
        + "& &= | |= ^ ^= "
        + ":= <>";
    let results = tokenize(&text);
    let lengths = [
        1, 2, 3, 2, 2, 1, 2, 3, 2, 2, 1, 1, 1, 1, 1, 2, 1, 2, 2, 3, 2, 2, 2, 2, 3, 1, 2, 1, 2, 1,
        2, 2, 2,
    ];
    let operator_types = [
        OperatorType::LessThan,
        OperatorType::LeftShift,
        OperatorType::LeftShiftEqual,
        OperatorType::Equals,
        OperatorType::NotEquals,
        OperatorType::GreaterThan,
        OperatorType::RightShift,
        OperatorType::RightShiftEqual,
        OperatorType::GreaterThanOrEqual,
        OperatorType::LessThanOrEqual,
        OperatorType::Add,
        OperatorType::Subtract,
        OperatorType::BitwiseInvert,
        OperatorType::Mod,
        OperatorType::Multiply,
        OperatorType::Power,
        OperatorType::Divide,
        OperatorType::FloorDivide,
        OperatorType::DivideEqual,
        OperatorType::FloorDivideEqual,
        OperatorType::MultiplyEqual,
        OperatorType::AddEqual,
        OperatorType::SubtractEqual,
        OperatorType::ModEqual,
        OperatorType::PowerEqual,
        OperatorType::BitwiseAnd,
        OperatorType::BitwiseAndEqual,
        OperatorType::BitwiseOr,
        OperatorType::BitwiseOrEqual,
        OperatorType::BitwiseXor,
        OperatorType::BitwiseXorEqual,
        OperatorType::Walrus,
        OperatorType::LessOrGreaterThan,
    ];
    assert_eq!(
        results.tokens.count() - IMPLICIT_TOKEN_COUNT,
        lengths.len() as i32
    );
    assert_eq!(
        results.tokens.count() - IMPLICIT_TOKEN_COUNT,
        operator_types.len() as i32
    );
    for i in 0..lengths.len() {
        let t = results.tokens.get_item_at(i as i32);
        assert_eq!(
            t.type_,
            TokenType::Operator,
            "{:?} at {i} is not an operator",
            t.type_
        );
        assert_eq!(
            OperatorToken::cast(t).unwrap().d().operator_type,
            operator_types[i]
        );
        assert_eq!(
            t.length, lengths[i],
            "Length {} at {i}, expected {}",
            t.length, lengths[i]
        );
    }
}

#[test]
fn identifiers() {
    let results = tokenize("and __and __and__ and__");
    assert_eq!(results.tokens.count(), 4 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Keyword);
    assert_eq!(results.tokens.get_item_at(0).length, 3);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).length, 5);

    assert_eq!(results.tokens.get_item_at(2).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(2).length, 7);

    assert_eq!(results.tokens.get_item_at(3).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(3).length, 5);

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(3), 0);
    assert_eq!(results.tokens.get_item_at_position(4), 1);
    assert_eq!(results.tokens.get_item_at_position(9), 1);
    assert_eq!(results.tokens.get_item_at_position(10), 2);
    assert_eq!(results.tokens.get_item_at_position(17), 2);
    assert_eq!(results.tokens.get_item_at_position(18), 3);
    assert_eq!(results.tokens.get_item_at_position(22), 3);
    assert_eq!(results.tokens.get_item_at_position(23), 5);

    assert_eq!(results.tokens.contains(22), true);
    assert_eq!(results.tokens.contains(23), false);
}

#[test]
fn lines1() {
    let path = concat!(
        env!("HOME"),
        "/pyright/packages/pyright-internal/src/tests/samples/lines1.py"
    );
    let sample_text =
        std::fs::read_to_string(path).expect("read lines1.py from the pyright checkout");

    // Start with the line feed only. We don't know whether the
    // sample file was stored with CR/LF or just LF, so do
    // the replacement here.
    let sample_text_lf_only: Vec<u16> = sample_text.replace("\r\n", "\n").encode_utf16().collect();
    let sample_text_cr_lf: Vec<u16> = String::from_utf16(&sample_text_lf_only)
        .unwrap()
        .replace('\n', "\r\n")
        .encode_utf16()
        .collect();

    // The same tokenizer instance tokenizes both texts, as the TypeScript does.
    let mut t = Tokenizer::new();
    let results_lf = t.tokenize(&sample_text_lf_only, None, None, 0, false);
    assert_eq!(results_lf.lines.count(), 15);

    // Now replace the LF with CR/LF sequences.
    let results_cr_lf = t.tokenize(&sample_text_cr_lf, None, None, 0, false);
    assert_eq!(results_cr_lf.lines.count(), 15);
}

#[test]
fn comments1() {
    let text = "# hello\n# good bye\n\n\n\"\"\" test \"\"\" # another\n\n\npass";
    let results = tokenize(text);
    assert_eq!(results.tokens.count(), 4 + IMPLICIT_TOKEN_COUNT);

    let token0 = results.tokens.get_item_at(0);
    assert_eq!(token0.type_, TokenType::NewLine);
    assert_eq!(token0.comments().unwrap().len(), 1);
    assert_eq!(token0.comments().unwrap()[0].value, " hello");

    let token1 = results.tokens.get_item_at(1);
    assert_eq!(token1.type_, TokenType::String);
    assert_eq!(token1.comments().unwrap().len(), 1);
    assert_eq!(token1.comments().unwrap()[0].value, " good bye");

    let token2 = results.tokens.get_item_at(2);
    assert_eq!(token2.type_, TokenType::NewLine);
    assert_eq!(token2.comments().unwrap().len(), 1);
    assert_eq!(token2.comments().unwrap()[0].value, " another");

    assert_eq!(results.tokens.get_item_at_position(0), -1);
    assert_eq!(results.tokens.get_item_at_position(7), 0);
    assert_eq!(results.tokens.get_item_at_position(20), 0);
    assert_eq!(results.tokens.get_item_at_position(21), 1);
    assert_eq!(results.tokens.get_item_at_position(42), 1);
    assert_eq!(results.tokens.get_item_at_position(43), 2);
    assert_eq!(results.tokens.get_item_at_position(45), 2);
    assert_eq!(results.tokens.get_item_at_position(46), 3);
    assert_eq!(results.tokens.get_item_at_position(49), 3);
    assert_eq!(results.tokens.get_item_at_position(50), 5);

    assert_eq!(results.tokens.contains(49), true);
    assert_eq!(results.tokens.contains(50), false);
}

#[test]
fn comments2() {
    let text = "class A:\n    def func(self):\n        pass\n        # comment\n    ";
    let results = tokenize(text);
    assert_eq!(results.tokens.count(), 16 + IMPLICIT_TOKEN_COUNT);

    let token17 = results.tokens.get_item_at(17);
    assert_eq!(token17.type_, TokenType::EndOfStream);
    assert!(token17.comments().is_none());
    let start = token17.start;

    let token16 = results.tokens.get_item_at(16);
    assert_eq!(token16.type_, TokenType::Dedent);
    assert_eq!(token16.start, start);
    assert!(token16.comments().is_none());

    // When multiple tokens have the same start position (and 0-length)
    // comments, if any, are stored on the first such token.
    let token15 = results.tokens.get_item_at(15);
    assert_eq!(token15.type_, TokenType::Dedent);
    assert_eq!(token15.start, start);
    assert_eq!(token15.comments().unwrap().len(), 1);
    assert_eq!(token15.comments().unwrap()[0].value, " comment");

    let token14 = results.tokens.get_item_at(14);
    assert_ne!(token14.start, start);
}

#[test]
fn identifiers1() {
    let results = tokenize("배열 数値 лік Opciók 可選值");
    assert_eq!(results.tokens.count(), 5 + IMPLICIT_TOKEN_COUNT);

    // Korean (Hangul)
    let token0 = results.tokens.get_item_at(0);
    assert_eq!(token0.type_, TokenType::Identifier);

    // Japanese
    let token1 = results.tokens.get_item_at(1);
    assert_eq!(token1.type_, TokenType::Identifier);

    // Russian (Cyrillic)
    let token2 = results.tokens.get_item_at(2);
    assert_eq!(token2.type_, TokenType::Identifier);

    // Hungarian
    let token3 = results.tokens.get_item_at(3);
    assert_eq!(token3.type_, TokenType::Identifier);

    // Chinese
    let token4 = results.tokens.get_item_at(4);
    assert_eq!(token4.type_, TokenType::Identifier);
}

#[test]
fn type_ignore_all1() {
    let results = tokenize("\n#type:ignore\n\"test\"");
    assert!(results.type_ignore_all.is_some());
}

#[test]
fn type_ignore_all2() {
    let results = tokenize("\n#    type:     ignore ssss\n");
    assert!(results.type_ignore_all.is_some());
}

#[test]
fn type_ignore_all3() {
    let results = tokenize("\n#    type:     ignoreSsss\n");
    assert!(results.type_ignore_all.is_none());
}

#[test]
fn type_ignore_all3_2() {
    let results = tokenize("\n\"hello\"\n# type: ignore\n");
    assert!(results.type_ignore_all.is_none());
}

#[test]
fn type_ignore_line1() {
    let results = tokenize("\na = 3 # type: ignore\n\"test\" # type:ignore");
    assert_eq!(results.type_ignore_lines.len() as i32, 2);
    assert!(results.type_ignore_lines.contains_key(&1));
    assert!(results.type_ignore_lines.contains_key(&2));
}

#[test]
fn type_ignore_line2() {
    let results = tokenize("a = 3 # type: ignores\n\"test\" # type:ignore");
    assert_eq!(results.type_ignore_lines.len() as i32, 1);
    assert!(results.type_ignore_lines.contains_key(&1));

    assert_eq!(results.tokens.get_item_at_position(0), 0);
    assert_eq!(results.tokens.get_item_at_position(1), 0);
    assert_eq!(results.tokens.get_item_at_position(2), 1);
    assert_eq!(results.tokens.get_item_at_position(3), 1);
    assert_eq!(results.tokens.get_item_at_position(4), 2);
    assert_eq!(results.tokens.get_item_at_position(20), 2);
    assert_eq!(results.tokens.get_item_at_position(21), 3);
    assert_eq!(results.tokens.get_item_at_position(22), 4);
    assert_eq!(results.tokens.get_item_at_position(41), 4);
    assert_eq!(results.tokens.get_item_at_position(42), 6);

    assert_eq!(results.tokens.contains(41), true);
    assert_eq!(results.tokens.contains(42), false);
}

#[test]
fn type_ignore_line_malformed_bracket() {
    let results = tokenize("a = 3 # type: ignore[broken");
    assert_eq!(results.type_ignore_lines.len() as i32, 0);
}

// A space-separated unclosed bracket (e.g. `# type: ignore [broken`) is also
// rejected entirely. The tokenizer does not fall back to treating the
// directive as "ignore all" when the bracket list is present but malformed.
#[test]
fn type_ignore_line_malformed_bracket_with_space() {
    let results = tokenize("a = 3 # type: ignore [broken");
    assert_eq!(results.type_ignore_lines.len() as i32, 0);
}

// Regression test for https://github.com/microsoft/pyright/issues/11345.
// type: ignore comments containing tool-namespaced codes (e.g. "ty:rule-name")
// must be recognised as type: ignore comments.
#[test]
fn type_ignore_namespaced_code() {
    // Single namespaced code.
    let results = tokenize("a = 1  # type: ignore[ty:unresolved-reference]");
    assert_eq!(results.type_ignore_lines.len() as i32, 1);
    assert!(results.type_ignore_lines.contains_key(&0));
    let rules = results
        .type_ignore_lines
        .get(&0)
        .unwrap()
        .rules_list
        .as_ref();
    assert!(rules.is_some());
    let rules = rules.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].text, "ty:unresolved-reference");
}

#[test]
fn type_ignore_mixed_namespaced_codes() {
    // Mix of plain and namespaced codes.
    let results = tokenize("a = 1  # type: ignore[name-defined, ty:unresolved-reference]");
    assert_eq!(results.type_ignore_lines.len() as i32, 1);
    let rules = results
        .type_ignore_lines
        .get(&0)
        .unwrap()
        .rules_list
        .as_ref();
    assert!(rules.is_some());
    let rules = rules.unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].text, "name-defined");
    assert_eq!(rules[1].text, "ty:unresolved-reference");
}

#[test]
fn constructor() {
    let results = tokenize("def constructor");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    assert_eq!(results.tokens.get_item_at(0).type_, TokenType::Keyword);
    assert_eq!(results.tokens.get_item_at(0).length, 3);

    assert_eq!(results.tokens.get_item_at(1).type_, TokenType::Identifier);
    assert_eq!(results.tokens.get_item_at(1).length, 11);
}

#[test]
fn normalization() {
    let results = tokenize("ℝ 𝕽");
    assert_eq!(results.tokens.count(), 2 + IMPLICIT_TOKEN_COUNT);

    let mut id_token = IdentifierToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(id_token.type_, TokenType::Identifier);
    assert_eq!(id_token.length, 1);
    assert_eq!(&*id_token.d().value, "R");

    id_token = IdentifierToken::cast(results.tokens.get_item_at(1)).unwrap();
    assert_eq!(id_token.type_, TokenType::Identifier);
    assert_eq!(id_token.length, 2);
    assert_eq!(id_token.d().value.as_ref(), "R");
}

#[test]
fn last_empty_line() {
    let results = tokenize("\r\n");
    assert_eq!(results.tokens.count(), IMPLICIT_TOKEN_COUNT);

    let new_line_token = NewLineToken::cast(results.tokens.get_item_at(0)).unwrap();
    assert_eq!(new_line_token.type_, TokenType::NewLine);
    assert_eq!(new_line_token.length, 2);
    assert_eq!(
        new_line_token.d().new_line_type,
        NewLineType::CarriageReturnLineFeed
    );

    let eof_token = results.tokens.get_item_at(1);
    assert_eq!(eof_token.type_, TokenType::EndOfStream);
    assert_eq!(eof_token.length, 0);
}

#[path = "tokenizer_unescape_tests.rs"]
mod unescape_tests;

/// `Token::keep` copies a token out of its tokenizer's scratch region once, and
/// the copy, which owns the token's buffers, outlives the output.
#[test]
fn kept_token_outlives_its_output() {
    let output = tokenize("x = 123456789012345678901234567890\n");
    let number = output.tokens.get_item_at(2);
    let value = NumberToken::cast(number).unwrap().d().value.clone();
    assert!(matches!(value, NumberValue::BigInt(_)));
    let kept = crate::parser::tokenizer_types::Token::keep(number);
    assert_eq!(crate::parser::tokenizer_types::Token::keep(number), kept);
    drop(output);
    drop(tokenize(&"y = 98765432109876543210987654321\n".repeat(20_000)));
    assert_eq!(NumberToken::cast(kept).unwrap().d().value, value);
}

#[test]
fn kept_token_keeps_its_comments() {
    let output = tokenize("x = (  # first\n    1)\n");
    let number = output.tokens.get_item_at(3);
    let kept = crate::parser::tokenizer_types::Token::keep(number);
    drop(output);
    drop(tokenize(&"y = 2  # other\n".repeat(20_000)));
    let comments = kept.comments().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].value, " first");
    assert_eq!(NumberToken::cast(kept).unwrap().d().value, NumberValue::Number(1.0));
}
