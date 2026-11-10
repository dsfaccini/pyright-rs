//! Rust side of the `tokens` dump stage. Format: `oracle/FORMAT.md`.
//!
//! `dump_tokenizer_output` writes a whole tokens-stage file. `token_line` and
//! `write_token` print one token (and its comments) wherever a token appears,
//! so the tree stage uses them too.

use super::format::{self, DumpWriter};
use crate::parser::tokenizer::{IgnoreComment, TokenizerOutput};
use crate::parser::tokenizer_types::{Token, TokenDetails, TokenType};

/// `TokenType` member names, by value.
pub fn token_type_name(token_type: TokenType) -> &'static str {
    match token_type {
        TokenType::Invalid => "Invalid",
        TokenType::EndOfStream => "EndOfStream",
        TokenType::NewLine => "NewLine",
        TokenType::Indent => "Indent",
        TokenType::Dedent => "Dedent",
        TokenType::String => "String",
        TokenType::Number => "Number",
        TokenType::Identifier => "Identifier",
        TokenType::Keyword => "Keyword",
        TokenType::Operator => "Operator",
        TokenType::Colon => "Colon",
        TokenType::Semicolon => "Semicolon",
        TokenType::Comma => "Comma",
        TokenType::OpenParenthesis => "OpenParenthesis",
        TokenType::CloseParenthesis => "CloseParenthesis",
        TokenType::OpenBracket => "OpenBracket",
        TokenType::CloseBracket => "CloseBracket",
        TokenType::OpenCurlyBrace => "OpenCurlyBrace",
        TokenType::CloseCurlyBrace => "CloseCurlyBrace",
        TokenType::Ellipsis => "Ellipsis",
        TokenType::Dot => "Dot",
        TokenType::Arrow => "Arrow",
        TokenType::Backtick => "Backtick",
        TokenType::ExclamationMark => "ExclamationMark",
        TokenType::FStringStart => "FStringStart",
        TokenType::FStringMiddle => "FStringMiddle",
        TokenType::FStringEnd => "FStringEnd",
    }
}

/// The one-line form of a token, without its comments:
/// `<TokenTypeName> <start>+<length>[ <field>=<scalar>]...`, fields sorted by name.
pub fn token_line(token: &Token) -> String {
    let mut fields: Vec<(&str, String)> = vec![];
    match &token.d {
        TokenDetails::None => {}
        TokenDetails::Indent(d) => {
            fields.push(("indentAmount", format::number(d.indent_amount as f64)));
            fields.push((
                "isIndentAmbiguous",
                format::boolean(d.is_indent_ambiguous).to_string(),
            ));
        }
        TokenDetails::Dedent(d) => {
            fields.push(("indentAmount", format::number(d.indent_amount as f64)));
            fields.push((
                "isDedentAmbiguous",
                format::boolean(d.is_dedent_ambiguous).to_string(),
            ));
            fields.push((
                "matchesIndent",
                format::boolean(d.matches_indent).to_string(),
            ));
        }
        TokenDetails::NewLine(d) => {
            fields.push(("newLineType", format::number(d.new_line_type as i32 as f64)));
        }
        TokenDetails::Keyword(d) => {
            fields.push(("keywordType", format::number(d.keyword_type as i32 as f64)));
        }
        TokenDetails::Operator(d) => {
            fields.push((
                "operatorType",
                format::number(d.operator_type as i32 as f64),
            ));
        }
        TokenDetails::Identifier(d) => {
            fields.push(("value", format::str(&d.value)));
        }
        TokenDetails::Number(d) => {
            fields.push(("isImaginary", format::boolean(d.is_imaginary).to_string()));
            fields.push(("isInteger", format::boolean(d.is_integer).to_string()));
            fields.push(("value", format::number_value(&d.value)));
        }
        TokenDetails::String(d) => {
            fields.push(("escapedValue", format::string(&d.escaped_value)));
            fields.push(("flags", format::number(d.flags.0 as f64)));
            fields.push(("prefixLength", format::number(d.prefix_length as f64)));
            fields.push((
                "quoteMarkLength",
                format::number(d.quote_mark_length as f64),
            ));
        }
        TokenDetails::FStringStart(d) => {
            fields.push(("flags", format::number(d.flags.0 as f64)));
            fields.push(("prefixLength", format::number(d.prefix_length as f64)));
            fields.push((
                "quoteMarkLength",
                format::number(d.quote_mark_length as f64),
            ));
        }
        TokenDetails::FStringMiddle(d) => {
            fields.push(("escapedValue", format::string(&d.escaped_value)));
            fields.push(("flags", format::number(d.flags.0 as f64)));
        }
        TokenDetails::FStringEnd(d) => {
            fields.push(("flags", format::number(d.flags.0 as f64)));
        }
    }

    let mut line = format!(
        "{} {}",
        token_type_name(token.type_),
        format::text_range(token)
    );
    for (name, value) in fields {
        line.push(' ');
        line.push_str(name);
        line.push('=');
        line.push_str(&value);
    }
    line
}

/// `<key>: <token line>`, then the token's comments nested below it.
pub fn write_token(w: &mut DumpWriter, depth: usize, key: &str, token: &Token) {
    w.field(depth, key, &token_line(token));
    if let Some(comments) = token.comments {
        w.field(depth + 1, "comments", &format::array_header(comments.len()));
        for (i, comment) in comments.iter().enumerate() {
            w.field(
                depth + 2,
                &format::index_key(i),
                &format!(
                    "{} type={} value={}",
                    format::text_range(comment),
                    format::number(comment.type_ as i32 as f64),
                    format::str(&comment.value)
                ),
            );
        }
    }
}

fn write_ignore_comment(w: &mut DumpWriter, depth: usize, comment: &IgnoreComment) {
    w.field(depth, "range", &format::text_range(&comment.range));
    if let Some(rules_list) = &comment.rules_list {
        w.field(depth, "rulesList", &format::array_header(rules_list.len()));
        for (j, rule) in rules_list.iter().enumerate() {
            w.open(depth + 1, &format::index_key(j));
            w.field(depth + 2, "range", &format::text_range(&rule.range));
            w.field(depth + 2, "text", &format::str(&rule.text));
        }
    }
}

fn write_ignore_lines(w: &mut DumpWriter, key: &str, lines: &jsrt::JsMap<i32, IgnoreComment>) {
    w.field(0, key, &format::map_header(lines.len()));
    for (line, comment) in lines {
        w.open(1, &format::number(*line as f64));
        write_ignore_comment(w, 2, comment);
    }
}

/// The tokens-stage file for one source file: the `TokenizerOutput` the parser returned.
pub fn dump_tokenizer_output(output: &TokenizerOutput) -> String {
    let mut w = DumpWriter::new();

    w.field(
        0,
        "hasPredominantTabSequence",
        format::boolean(output.has_predominant_tab_sequence),
    );

    w.field(
        0,
        "lines",
        &format::array_header(output.lines.count() as usize),
    );
    for (i, line) in output.lines.items().iter().enumerate() {
        w.field(1, &format::index_key(i), &format::text_range(line));
    }

    w.field(
        0,
        "predominantEndOfLineSequence",
        &format::str(&output.predominant_end_of_line_sequence),
    );
    w.field(
        0,
        "predominantSingleQuoteCharacter",
        &format::str(&output.predominant_single_quote_character),
    );
    w.field(
        0,
        "predominantTabSequence",
        &format::str(&output.predominant_tab_sequence),
    );

    write_ignore_lines(&mut w, "pyrightIgnoreLines", &output.pyright_ignore_lines);

    w.field(
        0,
        "tokens",
        &format::array_header(output.tokens.count() as usize),
    );
    for (i, token) in output.tokens.items().iter().enumerate() {
        write_token(&mut w, 1, &format::index_key(i), token);
    }

    if let Some(type_ignore_all) = &output.type_ignore_all {
        w.open(0, "typeIgnoreAll");
        write_ignore_comment(&mut w, 1, type_ignore_all);
    }

    write_ignore_lines(&mut w, "typeIgnoreLines", &output.type_ignore_lines);

    w.finish()
}
