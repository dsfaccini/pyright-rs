//! Port of `packages/pyright-internal/src/parser/tokenizer.ts`.
//!
//! The tokenizer reads UTF-16 source text (`&[u16]`) like `charCodeAt`; every
//! offset and length is a UTF-16 offset.

use std::rc::Rc;
use std::sync::OnceLock;

use jsrt::number::{math_round, parse_bigint, parse_float, parse_int};
use jsrt::{JsMap, JsStr, JsString, NumberValue, P};

use super::character_stream::CharacterStream;
use super::characters::{
    is_binary, is_decimal, is_hex, is_identifier_char, is_identifier_start_char, is_octal,
    is_surrogate_char,
};
use super::tokenizer_types::{
    TokenScratch,
    Comment, CommentType, DedentToken, FStringEndToken, FStringMiddleToken, FStringStartToken,
    IdentifierToken, IndentToken, KeywordToken, KeywordType, NewLineToken, NewLineType,
    NumberToken, OperatorFlags, OperatorToken, OperatorType, StringToken, StringTokenFlags, Token,
    TokenType,
};
use crate::common::char_codes::Char;
use crate::common::text_range::TextRange;
use crate::common::text_range_collection::TextRangeCollection;

// This must be a Map, as operations like {}["constructor"] succeed.
// parser/tokenizer.ts:51
const _KEYWORDS: [(&str, KeywordType); 40] = [
    ("and", KeywordType::And),
    ("as", KeywordType::As),
    ("assert", KeywordType::Assert),
    ("async", KeywordType::Async),
    ("await", KeywordType::Await),
    ("break", KeywordType::Break),
    ("case", KeywordType::Case),
    ("class", KeywordType::Class),
    ("continue", KeywordType::Continue),
    ("__debug__", KeywordType::Debug),
    ("def", KeywordType::Def),
    ("del", KeywordType::Del),
    ("elif", KeywordType::Elif),
    ("else", KeywordType::Else),
    ("except", KeywordType::Except),
    ("finally", KeywordType::Finally),
    ("for", KeywordType::For),
    ("from", KeywordType::From),
    ("global", KeywordType::Global),
    ("if", KeywordType::If),
    ("import", KeywordType::Import),
    ("in", KeywordType::In),
    ("is", KeywordType::Is),
    ("lambda", KeywordType::Lambda),
    ("lazy", KeywordType::Lazy),
    ("match", KeywordType::Match),
    ("nonlocal", KeywordType::Nonlocal),
    ("not", KeywordType::Not),
    ("or", KeywordType::Or),
    ("pass", KeywordType::Pass),
    ("raise", KeywordType::Raise),
    ("return", KeywordType::Return),
    ("try", KeywordType::Try),
    ("type", KeywordType::Type),
    ("while", KeywordType::While),
    ("with", KeywordType::With),
    ("yield", KeywordType::Yield),
    ("False", KeywordType::False),
    ("None", KeywordType::None),
    ("True", KeywordType::True),
];

/// `_keywords.get(name)`.
fn _keywords_get(name: &str) -> Option<KeywordType> {
    _KEYWORDS
        .iter()
        .find(|(text, _)| *text == name)
        .map(|&(_, t)| t)
}

// parser/tokenizer.ts:94
const _SOFT_KEYWORDS: [&str; 4] = ["match", "case", "type", "lazy"];

// parser/tokenizer.ts:110
const _KEYWORD_MIN_LEN: i32 = 2;
// parser/tokenizer.ts:111
const _KEYWORD_MAX_LEN: i32 = 9; // __debug__

// parser/tokenizer.ts:113
struct KeywordEntry {
    text: &'static str,
    type_: KeywordType,
}

/// The tables `tokenizer.ts` builds at module load.
struct TokenizerTables {
    // parser/tokenizer.ts:99
    keyword_first_char_table: [bool; 128],
    // parser/tokenizer.ts:120
    keyword_entries_by_first_char: Vec<Option<Vec<KeywordEntry>>>,
    // parser/tokenizer.ts:298
    ascii_identifier_continue: [bool; 128],
    // parser/tokenizer.ts:308
    ascii_identifier_start: [bool; 128],
}

static TABLES: OnceLock<TokenizerTables> = OnceLock::new();

fn tables() -> &'static TokenizerTables {
    TABLES.get_or_init(|| {
        let mut keyword_first_char_table = [false; 128];
        for (kw, _) in _KEYWORDS {
            let code = kw.as_bytes()[0] as usize;
            if code < 128 {
                keyword_first_char_table[code] = true;
            }
        }

        let mut keyword_entries_by_first_char: Vec<Option<Vec<KeywordEntry>>> =
            (0..128).map(|_| None).collect();
        for (text, type_) in _KEYWORDS {
            let first_char_code = text.as_bytes()[0] as usize;
            if first_char_code < 128 {
                keyword_entries_by_first_char[first_char_code]
                    .get_or_insert_with(Vec::new)
                    .push(KeywordEntry { text, type_ });
            }
        }

        let mut ascii_identifier_continue = [false; 128];
        for i in 0..128u16 {
            if is_identifier_char(i, None) {
                ascii_identifier_continue[i as usize] = true;
            }
        }

        let mut ascii_identifier_start = [false; 128];
        for i in 0..128u16 {
            if is_identifier_start_char(i, None) {
                ascii_identifier_start[i as usize] = true;
            }
        }

        TokenizerTables {
            keyword_first_char_table,
            keyword_entries_by_first_char,
            ascii_identifier_continue,
            ascii_identifier_start,
        }
    })
}

/// `text.startsWith(ascii, position)` for an ASCII `ascii`.
fn starts_with_ascii(text: &[u16], position: usize, ascii: &str) -> bool {
    let bytes = ascii.as_bytes();
    position + bytes.len() <= text.len()
        && bytes
            .iter()
            .enumerate()
            .all(|(i, &b)| text[position + i] == b as u16)
}

/// `text.charCodeAt(i)`; `None` is `NaN`.
fn char_code_at(text: &[u16], i: i32) -> Option<u16> {
    if i >= 0 && (i as usize) < text.len() {
        Some(text[i as usize])
    } else {
        None
    }
}

// parser/tokenizer.ts:132
fn get_keyword_type_from_text_slice(text: &[u16], start: i32, length: i32) -> Option<KeywordType> {
    if length < _KEYWORD_MIN_LEN || length > _KEYWORD_MAX_LEN {
        return None;
    }

    let tables = tables();
    let first_char_code = text[start as usize];
    if first_char_code >= 128 || !tables.keyword_first_char_table[first_char_code as usize] {
        return None;
    }

    let candidates = tables.keyword_entries_by_first_char[first_char_code as usize].as_ref()?;

    for candidate in candidates {
        if candidate.text.len() as i32 == length
            && starts_with_ascii(text, start as usize, candidate.text)
        {
            return Some(candidate.type_);
        }
    }

    None
}

/// `_operatorInfo[operatorType]`; `None` is `undefined` (only `Walrus`).
// parser/tokenizer.ts:156
fn _operator_info(operator_type: OperatorType) -> Option<OperatorFlags> {
    use OperatorType as O;
    let flags = match operator_type {
        O::Add => OperatorFlags::Unary | OperatorFlags::Binary,
        O::AddEqual => OperatorFlags::Assignment,
        O::Assign => OperatorFlags::Assignment,
        O::BitwiseAnd => OperatorFlags::Binary,
        O::BitwiseAndEqual => OperatorFlags::Assignment,
        O::BitwiseInvert => OperatorFlags::Unary,
        O::BitwiseOr => OperatorFlags::Binary,
        O::BitwiseOrEqual => OperatorFlags::Assignment,
        O::BitwiseXor => OperatorFlags::Binary,
        O::BitwiseXorEqual => OperatorFlags::Assignment,
        O::Divide => OperatorFlags::Binary,
        O::DivideEqual => OperatorFlags::Assignment,
        O::Equals => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::FloorDivide => OperatorFlags::Binary,
        O::FloorDivideEqual => OperatorFlags::Assignment,
        O::GreaterThan => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::GreaterThanOrEqual => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::LeftShift => OperatorFlags::Binary,
        O::LeftShiftEqual => OperatorFlags::Assignment,
        O::LessOrGreaterThan => {
            OperatorFlags::Binary | OperatorFlags::Comparison | OperatorFlags::Deprecated
        }
        O::LessThan => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::LessThanOrEqual => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::MatrixMultiply => OperatorFlags::Binary,
        O::MatrixMultiplyEqual => OperatorFlags::Assignment,
        O::Mod => OperatorFlags::Binary,
        O::ModEqual => OperatorFlags::Assignment,
        O::Multiply => OperatorFlags::Binary,
        O::MultiplyEqual => OperatorFlags::Assignment,
        O::NotEquals => OperatorFlags::Binary | OperatorFlags::Comparison,
        O::Power => OperatorFlags::Binary,
        O::PowerEqual => OperatorFlags::Assignment,
        O::RightShift => OperatorFlags::Binary,
        O::RightShiftEqual => OperatorFlags::Assignment,
        O::Subtract => OperatorFlags::Binary,
        O::SubtractEqual => OperatorFlags::Assignment,

        O::And => OperatorFlags::Binary,
        O::Or => OperatorFlags::Binary,
        O::Not => OperatorFlags::Unary,
        O::Is => OperatorFlags::Binary,
        O::IsNot => OperatorFlags::Binary,
        O::In => OperatorFlags::Binary,
        O::NotIn => OperatorFlags::Binary,

        O::Walrus => return None,
    };
    Some(flags)
}

// parser/tokenizer.ts:202
const _UNSET_SINGLE_CHAR_OPERATOR_TYPE: i16 = -1;

// parser/tokenizer.ts:203
const _SINGLE_CHAR_OPERATOR_TYPE_TABLE: [i16; 128] = {
    let mut table = [_UNSET_SINGLE_CHAR_OPERATOR_TYPE; 128];
    table[Char::Equal as usize] = OperatorType::Assign as i16;
    table[Char::Plus as usize] = OperatorType::Add as i16;
    table[Char::Hyphen as usize] = OperatorType::Subtract as i16;
    table[Char::Asterisk as usize] = OperatorType::Multiply as i16;
    table[Char::Slash as usize] = OperatorType::Divide as i16;
    table[Char::Ampersand as usize] = OperatorType::BitwiseAnd as i16;
    table[Char::Bar as usize] = OperatorType::BitwiseOr as i16;
    table[Char::Caret as usize] = OperatorType::BitwiseXor as i16;
    table[Char::Percent as usize] = OperatorType::Mod as i16;
    table[Char::Tilde as usize] = OperatorType::BitwiseInvert as i16;
    table[Char::At as usize] = OperatorType::MatrixMultiply as i16;
    table[Char::Less as usize] = OperatorType::LessThan as i16;
    table[Char::Greater as usize] = OperatorType::GreaterThan as i16;
    table
};

// parser/tokenizer.ts:222
const _SINGLE_CHAR_EQUAL_OPERATOR_TYPE_TABLE: [i16; 128] = {
    let mut table = [_UNSET_SINGLE_CHAR_OPERATOR_TYPE; 128];
    table[Char::Plus as usize] = OperatorType::AddEqual as i16;
    table[Char::Hyphen as usize] = OperatorType::SubtractEqual as i16;
    table[Char::Asterisk as usize] = OperatorType::MultiplyEqual as i16;
    table[Char::Slash as usize] = OperatorType::DivideEqual as i16;
    table[Char::Ampersand as usize] = OperatorType::BitwiseAndEqual as i16;
    table[Char::Bar as usize] = OperatorType::BitwiseOrEqual as i16;
    table[Char::Caret as usize] = OperatorType::BitwiseXorEqual as i16;
    table[Char::Percent as usize] = OperatorType::ModEqual as i16;
    table[Char::At as usize] = OperatorType::MatrixMultiplyEqual as i16;
    table
};

// parser/tokenizer.ts:237
const fn get_two_char_key(char1: u16, char2: u16) -> u32 {
    ((char1 as u32) << 8) | char2 as u32
}

// parser/tokenizer.ts:243
const _TWO_CHAR_OPERATOR_TYPE_MAP: [(u32, OperatorType); 5] = [
    (
        get_two_char_key(Char::Equal, Char::Equal),
        OperatorType::Equals,
    ),
    (
        get_two_char_key(Char::ExclamationMark, Char::Equal),
        OperatorType::NotEquals,
    ),
    (
        get_two_char_key(Char::Less, Char::Equal),
        OperatorType::LessThanOrEqual,
    ),
    (
        get_two_char_key(Char::Greater, Char::Equal),
        OperatorType::GreaterThanOrEqual,
    ),
    (
        get_two_char_key(Char::Less, Char::Greater),
        OperatorType::LessOrGreaterThan,
    ),
];

// parser/tokenizer.ts:251
const _TWO_CHAR_SPECIAL_TOKEN_TYPE_MAP: [(u32, TokenType); 1] = [(
    get_two_char_key(Char::Hyphen, Char::Greater),
    TokenType::Arrow,
)];

// parser/tokenizer.ts:255
const _REPEATED_CHAR_OPERATOR_TYPE_TABLE: [i16; 128] = {
    let mut table = [_UNSET_SINGLE_CHAR_OPERATOR_TYPE; 128];
    table[Char::Asterisk as usize] = OperatorType::Power as i16;
    table[Char::Slash as usize] = OperatorType::FloorDivide as i16;
    table[Char::Less as usize] = OperatorType::LeftShift as i16;
    table[Char::Greater as usize] = OperatorType::RightShift as i16;
    table
};

// parser/tokenizer.ts:265
const _REPEATED_CHAR_EQUAL_OPERATOR_TYPE_TABLE: [i16; 128] = {
    let mut table = [_UNSET_SINGLE_CHAR_OPERATOR_TYPE; 128];
    table[Char::Asterisk as usize] = OperatorType::PowerEqual as i16;
    table[Char::Slash as usize] = OperatorType::FloorDivideEqual as i16;
    table[Char::Less as usize] = OperatorType::LeftShiftEqual as i16;
    table[Char::Greater as usize] = OperatorType::RightShiftEqual as i16;
    table
};

fn operator_type_from_table(value: i16) -> OperatorType {
    OperatorType::ALL[value as usize]
}

// parser/tokenizer.ts:275
const _BYTE_ORDER_MARKER: u16 = 0xfeff;

// parser/tokenizer.ts:277
const DEFAULT_TAB_SIZE: i32 = 8;

// Fast-reject table: only these ASCII chars can begin a string literal.
// parser/tokenizer.ts:283
const _CAN_START_STRING: [bool; 128] = {
    let mut table = [false; 128];
    table[Char::SingleQuote as usize] = true;
    table[Char::DoubleQuote as usize] = true;
    let prefixes = [
        Char::f,
        Char::F,
        Char::r,
        Char::R,
        Char::b,
        Char::B,
        Char::u,
        Char::U,
        Char::t,
        Char::T,
    ];
    let mut i = 0;
    while i < prefixes.len() {
        table[prefixes[i] as usize] = true;
        i += 1;
    }
    table
};

// Create a detached copy of a source text range.
// parser/tokenizer.ts:324
fn detach_substring(text: &[u16], start: i32, end: i32) -> String {
    String::from_utf16_lossy(&text[start as usize..end as usize])
}

// Strip underscore characters from a source text range without first creating
// an intermediate substring.
// parser/tokenizer.ts:334
fn remove_underscores_from_range(text: &[u16], start: i32, end: i32) -> String {
    let mut first_underscore_index = -1;
    for i in start..end {
        if text[i as usize] == Char::Underscore {
            first_underscore_index = i;
            break;
        }
    }

    if first_underscore_index < 0 {
        return String::from_utf16_lossy(&text[start as usize..end as usize]);
    }

    let mut result =
        String::from_utf16_lossy(&text[start as usize..first_underscore_index as usize]);
    let mut i = first_underscore_index + 1;
    while i < end {
        if text[i as usize] != Char::Underscore {
            result.push_str(&String::from_utf16_lossy(&text[i as usize..i as usize + 1]));
        }
        i += 1;
    }
    result
}

// Manual replacement for magicsRegEx = /\\\s*$/
// parser/tokenizer.ts:359
fn ends_with_backslash_continuation(text: &[u16], start: i32, end: i32) -> bool {
    let mut i = end - 1;
    // Skip trailing whitespace
    while i >= start {
        let ch = text[i as usize];
        if ch == Char::Space || ch == Char::Tab || ch == Char::FormFeed {
            i -= 1;
        } else {
            break;
        }
    }
    i >= start && text[i as usize] == Char::Backslash
}

// parser/tokenizer.ts:374
struct IgnoreDirectiveMatch<'a> {
    full_match: &'a [u16],              // group 0: full matched text
    prefix: &'a [u16],                  // group 1: prefix before directive keyword
    bracket_content: Option<&'a [u16]>, // group 4: content inside [...] if present
    index: i32,                         // match position within the input string
}

// Parses a bracketed rule list starting at `pos` (which must point at '[').
// Returns the bracket content (without brackets) and the position just past ']',
// or undefined if the bracket is malformed.
// parser/tokenizer.ts:385
fn parse_ignore_bracket_content(
    text: &[u16],
    mut pos: i32,
    range_end: i32,
    allow_colon: bool,
) -> Option<(&[u16], i32)> {
    pos += 1; // skip '['
    let bracket_start = pos;
    while pos < range_end && text[pos as usize] != Char::CloseBracket {
        // Only allow valid bracket content chars: \s, \w, -, ,
        // (plus ':' for type: ignore to support tool-namespaced codes)
        let bc = text[pos as usize];
        if (bc >= Char::a && bc <= Char::z)
            || (bc >= Char::A && bc <= Char::Z)
            || (bc >= Char::_0 && bc <= Char::_9)
            || bc == Char::Underscore
            || bc == Char::Hyphen
            || bc == Char::Comma
            || bc == Char::Space
            || bc == Char::Tab
            || (allow_colon && bc == Char::Colon)
        {
            pos += 1;
        } else {
            break;
        }
    }
    if pos < range_end && text[pos as usize] == Char::CloseBracket {
        return Some((&text[bracket_start as usize..pos as usize], pos + 1));
    }
    None
}

// Manual replacement for typeIgnoreCommentRegEx / pyrightIgnoreCommentRegEx.
// Scans `text` within [rangeStart, rangeEnd) for `<directive>: ignore [rules]`
// where directive is 'type' or 'pyright'.
// parser/tokenizer.ts:423
fn match_ignore_directive<'t>(
    text: &'t [u16],
    range_start: i32,
    range_end: i32,
    directive: &str,
) -> Option<IgnoreDirectiveMatch<'t>> {
    let directive: Vec<u16> = directive.encode_utf16().collect();
    let directive_length = directive.len() as i32;
    // type: ignore allows tool-namespaced codes (e.g. "ty:rule-name") in brackets;
    // pyright: ignore does not.
    let allow_colon_in_bracket = directive == "type".encode_utf16().collect::<Vec<u16>>();
    let mut search_from = range_start;

    while search_from < range_end {
        // Find the next occurrence of the directive keyword, bounded by rangeEnd.
        let first_char_code = directive[0];
        let mut directive_idx = -1;
        let scan_limit = range_end - directive_length;
        let mut i = search_from;
        while i <= scan_limit {
            if char_code_at(text, i) == Some(first_char_code) {
                let mut found = true;
                for d in 1..directive_length {
                    if char_code_at(text, i + d) != Some(directive[d as usize]) {
                        found = false;
                        break;
                    }
                }
                if found {
                    directive_idx = i;
                    break;
                }
            }
            i += 1;
        }
        if directive_idx < 0 {
            return None;
        }

        // Determine the prefix: scan backward from directiveIdx to find
        // the `#` or start-of-range, collecting whitespace.
        let mut prefix_start = directive_idx;
        let mut found_anchor = false;

        // Walk backward over spaces/tabs
        let mut j = directive_idx - 1;
        while j >= range_start
            && (char_code_at(text, j) == Some(Char::Space)
                || char_code_at(text, j) == Some(Char::Tab))
        {
            j -= 1;
        }

        if j < range_start {
            // At start of range
            prefix_start = range_start;
            found_anchor = true;
        } else if char_code_at(text, j) == Some(Char::Hash) {
            prefix_start = j;
            found_anchor = true;
        }

        if !found_anchor {
            search_from = directive_idx + 1;
            continue;
        }

        // After directive keyword, expect ':'
        let mut pos = directive_idx + directive_length;
        if pos >= range_end || char_code_at(text, pos) != Some(Char::Colon) {
            search_from = directive_idx + 1;
            continue;
        }
        pos += 1; // skip ':'

        // Skip optional whitespace after ':'
        while pos < range_end
            && (char_code_at(text, pos) == Some(Char::Space)
                || char_code_at(text, pos) == Some(Char::Tab))
        {
            pos += 1;
        }

        // Expect 'ignore'
        let ignore_str = "ignore";
        if pos + ignore_str.len() as i32 > range_end {
            search_from = directive_idx + 1;
            continue;
        }

        let mut matched = true;
        for (k, b) in ignore_str.bytes().enumerate() {
            if char_code_at(text, pos + k as i32) != Some(b as u16) {
                matched = false;
                break;
            }
        }
        if !matched {
            search_from = directive_idx + 1;
            continue;
        }
        pos += ignore_str.len() as i32;

        // After 'ignore', expect whitespace, '[', or end-of-range
        let mut bracket_content: Option<&[u16]> = None;

        if pos >= range_end {
            // End of range — valid
        } else {
            let ch = char_code_at(text, pos);
            if ch == Some(Char::Space) || ch == Some(Char::Tab) {
                // Skip whitespace to check for optional bracket
                while pos < range_end
                    && (char_code_at(text, pos) == Some(Char::Space)
                        || char_code_at(text, pos) == Some(Char::Tab))
                {
                    pos += 1;
                }
                if pos < range_end && char_code_at(text, pos) == Some(Char::OpenBracket) {
                    let Some(parsed) =
                        parse_ignore_bracket_content(text, pos, range_end, allow_colon_in_bracket)
                    else {
                        search_from = directive_idx + 1;
                        continue;
                    };
                    bracket_content = Some(parsed.0);
                    pos = parsed.1;
                }
            } else if ch == Some(Char::OpenBracket) {
                // Bracket immediately after 'ignore'
                let Some(parsed) =
                    parse_ignore_bracket_content(text, pos, range_end, allow_colon_in_bracket)
                else {
                    search_from = directive_idx + 1;
                    continue;
                };
                bracket_content = Some(parsed.0);
                pos = parsed.1;
            } else {
                // No space, no bracket — not a valid match
                search_from = directive_idx + 1;
                continue;
            }
        }

        let prefix = &text[prefix_start as usize..directive_idx as usize];
        let full_match = &text[prefix_start as usize..pos as usize];

        return Some(IgnoreDirectiveMatch {
            full_match,
            prefix,
            bracket_content,
            index: prefix_start,
        });
    }

    None
}

// parser/tokenizer.ts:574
#[derive(jsrt::Trace)]
pub struct TokenizerOutput {
    // List of all tokens.
    pub tokens: TextRangeCollection<P<Token>>,

    // List of ranges that comprise the lines.
    /// On the program heap: the source file keeps the lines after the output drops.
    pub lines: P<TextRangeCollection<TextRange>>,

    // Map of all line numbers that end in a "type: ignore" comment.
    pub type_ignore_lines: JsMap<i32, IgnoreComment>,

    // Map of all line numbers that end in a "pyright: ignore" comment.
    pub pyright_ignore_lines: JsMap<i32, IgnoreComment>,

    // Program starts with a "type: ignore" comment.
    pub type_ignore_all: Option<IgnoreComment>,

    // Line-end sequence ('/n', '/r', or '/r/n').
    pub predominant_end_of_line_sequence: String,

    // True if the tokenizer was able to identify the file's predominant
    // tab sequence. False if predominantTabSequence is set to our default.
    pub has_predominant_tab_sequence: bool,

    // Tab sequence ('/t or consecutive spaces).
    pub predominant_tab_sequence: String,

    // Does the code mostly use single or double quote
    // characters for string literals?
    pub predominant_single_quote_character: String,

    /// Where the tokens live; dropping the output frees those no parse node keeps.
    #[trace(skip)]
    pub scratch: Option<TokenScratch>,
}

// parser/tokenizer.ts:605
struct StringScannerOutput {
    escaped_value: JsString,
    flags: StringTokenFlags,
}

// parser/tokenizer.ts:610
#[derive(Clone, Copy)]
struct IndentInfo {
    tab1_spaces: i32,
    tab8_spaces: i32,
    is_space_present: bool,
    is_tab_present: bool,
}

// parser/tokenizer.ts:617
#[derive(Clone, Debug, PartialEq)]
#[derive(jsrt::Trace)]
pub struct IgnoreCommentRule {
    pub text: String,
    pub range: TextRange,
}

// parser/tokenizer.ts:622
#[derive(Clone, Debug, PartialEq)]
#[derive(jsrt::Trace)]
pub struct IgnoreComment {
    pub range: TextRange,
    pub rules_list: Option<Vec<IgnoreCommentRule>>,
}

// parser/tokenizer.ts:627
#[derive(Clone, Copy)]
struct FStringReplacementFieldContext {
    in_format_specifier: bool,
    paren_depth: i32,
}

// parser/tokenizer.ts:632
struct FStringContext {
    start_token: FStringStartToken,
    replacement_field_stack: Vec<FStringReplacementFieldContext>,
    active_replacement_field: Option<FStringReplacementFieldContext>,
}

// parser/tokenizer.ts:638
#[derive(Clone, Copy, PartialEq, Eq)]
enum MagicsKind {
    None,
    Line,
    Cell,
}

// parser/tokenizer.ts:693
const _IDENTIFIER_CACHE_SIZE: usize = 2048;
// parser/tokenizer.ts:694
const _IDENTIFIER_CACHE_MASK: i32 = _IDENTIFIER_CACHE_SIZE as i32 - 1;

// parser/tokenizer.ts:644
pub struct Tokenizer<'a> {
    _cs: CharacterStream<'a>,
    _tokens: Vec<P<Token>>,
    _prev_line_start: i32,
    _paren_depth: i32,
    _line_ranges: Vec<TextRange>,
    _indent_amounts: Vec<IndentInfo>,
    _type_ignore_all: Option<IgnoreComment>,
    // Cached answer to "are there any non-trivial tokens yet?"
    _has_token_before_ignore_all: bool,
    _type_ignore_lines: JsMap<i32, IgnoreComment>,
    _pyright_ignore_lines: JsMap<i32, IgnoreComment>,
    _comments: Option<Vec<Comment>>,
    _f_string_stack: Vec<FStringContext>,
    _active_f_string: Option<FStringContext>,

    // Total times CR, CR/LF, and LF are used to terminate
    // lines. Used to determine the predominant line ending.
    _cr_count: i32,
    _cr_lf_count: i32,
    _lf_count: i32,

    // Number of times an indent token is emitted.
    _indent_count: i32,

    // Number of times an indent token is emitted and a tab character
    // is present (used to determine predominant tab sequence).
    _indent_tab_count: i32,

    // Number of spaces that are added for an indent token
    // (used to determine predominant tab sequence).
    _indent_spaces_total: i32,

    // Number of single or double quote string literals found
    // in the code.
    _single_quote_count: i32,
    _double_quote_count: i32,

    // Assume Jupyter notebook tokenization rules?
    _use_notebook_mode: bool,

    // Direct-mapped identifier intern cache: the UTF-16 text and its string.
    _identifier_cache: Vec<Option<(Box<[u16]>, Rc<str>)>>,
}

impl Default for Tokenizer<'_> {
    fn default() -> Self {
        Tokenizer::new()
    }
}

impl<'a> Tokenizer<'a> {
    pub fn new() -> Tokenizer<'a> {
        Tokenizer {
            _cs: CharacterStream::new(&[]),
            _tokens: vec![],
            _prev_line_start: 0,
            _paren_depth: 0,
            _line_ranges: vec![],
            _indent_amounts: vec![],
            _type_ignore_all: None,
            _has_token_before_ignore_all: false,
            _type_ignore_lines: JsMap::default(),
            _pyright_ignore_lines: JsMap::default(),
            _comments: None,
            _f_string_stack: vec![],
            _active_f_string: None,
            _cr_count: 0,
            _cr_lf_count: 0,
            _lf_count: 0,
            _indent_count: 0,
            _indent_tab_count: 0,
            _indent_spaces_total: 0,
            _single_quote_count: 0,
            _double_quote_count: 0,
            _use_notebook_mode: false,
            _identifier_cache: vec![None; _IDENTIFIER_CACHE_SIZE],
        }
    }

    // parser/tokenizer.ts:697
    #[allow(clippy::manual_clamp)]
    pub fn tokenize(
        &mut self,
        text: &'a [u16],
        start: Option<i32>,
        length: Option<i32>,
        initial_paren_depth: i32,
        use_notebook_mode: bool,
    ) -> TokenizerOutput {
        let scratch = TokenScratch::new();
        let _entered = scratch.as_ref().map(TokenScratch::enter);
        let text_length = text.len() as i32;
        let start = match start {
            None => 0,
            Some(start) => {
                if start < 0 || start > text_length {
                    panic!("Invalid range start (start={start}, text.length={text_length})");
                }
                start
            }
        };

        let mut text = text;
        let length = match length {
            None => text_length,
            Some(length) => {
                if length < 0 || start + length > text_length {
                    panic!(
                        "Invalid range length (start={start}, length={length}, text.length={text_length})"
                    );
                } else if start + length < text_length {
                    text = &text[..(start + length) as usize];
                }
                length
            }
        };

        self._cs = CharacterStream::new(text);
        self._cs.set_position(start);
        self._tokens = vec![];
        self._prev_line_start = 0;
        self._paren_depth = initial_paren_depth;
        self._line_ranges = vec![];
        self._indent_amounts = vec![];
        self._use_notebook_mode = use_notebook_mode;
        // Clear per-source identifier intern cache.
        self._identifier_cache.fill(None);

        let end = start + length;

        if start == 0 {
            self._read_indentation_after_new_line();
        }

        while !self._cs.is_end_of_stream() {
            self._add_next_token();

            if self._cs.position() >= end {
                break;
            }
        }

        // Insert any implied FStringEnd tokens.
        while let Some(active_f_string) = &self._active_f_string {
            let flags = active_f_string.start_token.d().flags | StringTokenFlags::Unterminated;
            self._tokens
                .push(FStringEndToken::create(self._cs.position(), 0, flags).0);
            self._active_f_string = self._f_string_stack.pop();
        }

        // Insert an implied new line to make parsing easier.
        if self._tokens.is_empty()
            || self._tokens[self._tokens.len() - 1].type_ != TokenType::NewLine
        {
            if self._paren_depth == 0 {
                let comments = self._get_comments();
                self._tokens.push(
                    NewLineToken::create(self._cs.position(), 0, NewLineType::Implied, comments).0,
                );
            }
        }

        // Insert any implied dedent tokens.
        self._set_indent(
            self._cs.position(),
            0,
            0,
            /* is_space_present */ false,
            /* is_tab_present */ false,
        );

        // Add a final end-of-stream token to make parsing easier.
        let comments = self._get_comments();
        self._tokens.push(Token::create(
            TokenType::EndOfStream,
            self._cs.position(),
            0,
            comments,
        ));

        // Add the final line range.
        self._add_line_range();

        // If the last line ended in a line-end character, add an empty line.
        if !self._line_ranges.is_empty() {
            let last_line = self._line_ranges[self._line_ranges.len() - 1];
            let last_char_of_last_line = char_code_at(text, last_line.start + last_line.length - 1);
            if last_char_of_last_line == Some(Char::CarriageReturn)
                || last_char_of_last_line == Some(Char::LineFeed)
            {
                self._line_ranges.push(TextRange {
                    start: self._cs.position(),
                    length: 0,
                });
            }
        }

        let mut predominant_end_of_line_sequence = "\n";
        if self._cr_count > self._cr_lf_count && self._cr_count > self._lf_count {
            predominant_end_of_line_sequence = "\r";
        } else if self._cr_lf_count > self._cr_count && self._cr_lf_count > self._lf_count {
            predominant_end_of_line_sequence = "\r\n";
        }

        let mut predominant_tab_sequence = String::from("    ");
        let mut has_predominant_tab_sequence = false;
        // If more than half of the indents use tab sequences,
        // assume we're using tabs rather than spaces.
        if self._indent_tab_count as f64 > self._indent_count as f64 / 2.0 {
            has_predominant_tab_sequence = true;
            predominant_tab_sequence = String::from("\t");
        } else if self._indent_count > 0 {
            has_predominant_tab_sequence = true;
            // Compute the average number of spaces per indent
            // to estimate the predominant tab value.
            let mut average_space_per_indent =
                math_round(self._indent_spaces_total as f64 / self._indent_count as f64) as i32;
            if average_space_per_indent < 1 {
                average_space_per_indent = 1;
            } else if average_space_per_indent > DEFAULT_TAB_SIZE {
                average_space_per_indent = DEFAULT_TAB_SIZE;
            }
            predominant_tab_sequence = String::new();
            for _ in 0..average_space_per_indent {
                predominant_tab_sequence.push(' ');
            }
        }

        TokenizerOutput {
            tokens: TextRangeCollection::new(std::mem::take(&mut self._tokens)),
            lines: P::new(TextRangeCollection::new(std::mem::take(&mut self._line_ranges))),
            type_ignore_lines: self._type_ignore_lines.clone(),
            type_ignore_all: self._type_ignore_all.clone(),
            pyright_ignore_lines: self._pyright_ignore_lines.clone(),
            predominant_end_of_line_sequence: predominant_end_of_line_sequence.to_string(),
            has_predominant_tab_sequence,
            predominant_tab_sequence,
            predominant_single_quote_character: if self._single_quote_count
                >= self._double_quote_count
            {
                "'".to_string()
            } else {
                "\"".to_string()
            },
            scratch,
        }
    }

    /// `None` is `undefined`: `Walrus` has no entry.
    // parser/tokenizer.ts:823
    pub fn get_operator_info(operator_type: OperatorType) -> Option<OperatorFlags> {
        _operator_info(operator_type)
    }

    // parser/tokenizer.ts:827
    pub fn is_whitespace(token: &Token) -> bool {
        token.type_ == TokenType::NewLine
            || token.type_ == TokenType::Indent
            || token.type_ == TokenType::Dedent
    }

    // parser/tokenizer.ts:831
    pub fn is_python_keyword(name: &str, include_soft_keywords: bool) -> bool {
        let keyword = _keywords_get(name);
        // `!keyword` is also true for `KeywordType.And`, which is 0.
        if keyword.is_none() || keyword == Some(KeywordType::And) {
            return false;
        }

        if include_soft_keywords {
            return true;
        }

        !_SOFT_KEYWORDS.contains(&name)
    }

    // parser/tokenizer.ts:844
    pub fn is_python_identifier(value: &[u16]) -> bool {
        for (i, &c) in value.iter().enumerate() {
            if if i == 0 {
                !is_identifier_start_char(c, None)
            } else {
                !is_identifier_char(c, None)
            } {
                return false;
            }
        }

        true
    }

    // parser/tokenizer.ts:854
    pub fn is_operator_assignment(operator_type: Option<OperatorType>) -> bool {
        let Some(operator_type) = operator_type else {
            return false;
        };
        let Some(info) = _operator_info(operator_type) else {
            return false;
        };
        info.any(OperatorFlags::Assignment)
    }

    // parser/tokenizer.ts:861
    pub fn is_operator_comparison(operator_type: Option<OperatorType>) -> bool {
        let Some(operator_type) = operator_type else {
            return false;
        };
        let Some(info) = _operator_info(operator_type) else {
            return false;
        };
        info.any(OperatorFlags::Comparison)
    }

    // parser/tokenizer.ts:868
    fn _add_next_token(&mut self) {
        // Are we in the middle of an f-string but not in a replacement field?
        if let Some(active_f_string) = &self._active_f_string
            && (active_f_string.active_replacement_field.is_none()
                || active_f_string
                    .active_replacement_field
                    .unwrap()
                    .in_format_specifier)
        {
            self._handle_f_string_middle();
        } else {
            self._cs.skip_whitespace();
        }

        if self._cs.is_end_of_stream() {
            return;
        }

        if !self._handle_character() {
            self._cs.move_next();
        }
    }

    // Consumes one or more characters from the character stream and pushes
    // tokens onto the token list. Returns true if the caller should advance
    // to the next character.
    // parser/tokenizer.ts:892
    fn _handle_character(&mut self) -> bool {
        // f-strings, b-strings, etc — only check if current char can start a string
        let current_char = self._cs.current_char();
        if current_char < 128 && _CAN_START_STRING[current_char as usize] {
            let string_prefix_length = self._get_string_prefix_length();

            if string_prefix_length >= 0 {
                let mut string_prefix: &[u16] = &[];
                if string_prefix_length > 0 {
                    let position = self._cs.position() as usize;
                    string_prefix =
                        &self._cs.get_text()[position..position + string_prefix_length as usize];
                    // Indeed a string
                    self._cs.advance(string_prefix_length);
                }

                let quote_type_flags = self._get_quote_type_flags(string_prefix);
                if quote_type_flags != StringTokenFlags::None {
                    self._handle_string(quote_type_flags, string_prefix_length);
                    return true;
                }
            }
        }

        if self._cs.current_char() == Char::Hash {
            self._handle_comment();
            return true;
        }

        if self._use_notebook_mode {
            let kind = self._get_i_python_magics_kind();
            if kind == MagicsKind::Line {
                self._handle_i_python_magics(if self._cs.current_char() == Char::Percent {
                    CommentType::IPythonMagic
                } else {
                    CommentType::IPythonShellEscape
                });
                return true;
            }

            if kind == MagicsKind::Cell {
                self._handle_i_python_magics(if self._cs.current_char() == Char::Percent {
                    CommentType::IPythonCellMagic
                } else {
                    CommentType::IPythonCellShellEscape
                });
                return true;
            }
        }

        match self._cs.current_char() {
            _BYTE_ORDER_MARKER => {
                // Skip the BOM if it's at the start of the file.
                if self._cs.position() == 0 {
                    return false;
                }
                return self._handle_invalid();
            }

            Char::CarriageReturn => {
                let length = if self._cs.next_char() == Char::LineFeed {
                    2
                } else {
                    1
                };
                let new_line_type = if length == 2 {
                    NewLineType::CarriageReturnLineFeed
                } else {
                    NewLineType::CarriageReturn
                };
                self._handle_new_line(length, new_line_type);
                return true;
            }

            Char::LineFeed => {
                self._handle_new_line(1, NewLineType::LineFeed);
                return true;
            }

            Char::Backslash => {
                if self._cs.next_char() == Char::CarriageReturn {
                    let advance = if self._cs.look_ahead(2) == Char::LineFeed {
                        3
                    } else {
                        2
                    };

                    // If a line continuation (\\ + CR[LF]) appears at EOF, it's an error.
                    if self._cs.position() + advance >= self._cs.length() {
                        return self._handle_invalid();
                    }

                    self._cs.advance(advance);
                    self._add_line_range();

                    if !self._tokens.is_empty()
                        && self._tokens[self._tokens.len() - 1].type_ == TokenType::NewLine
                    {
                        self._read_indentation_after_new_line();
                    }
                    return true;
                }

                if self._cs.next_char() == Char::LineFeed {
                    let advance = 2;

                    // If a line continuation (\\ + LF) appears at EOF, it's an error.
                    if self._cs.position() + advance >= self._cs.length() {
                        return self._handle_invalid();
                    }

                    self._cs.advance(advance);
                    self._add_line_range();

                    if !self._tokens.is_empty()
                        && self._tokens[self._tokens.len() - 1].type_ == TokenType::NewLine
                    {
                        self._read_indentation_after_new_line();
                    }
                    return true;
                }

                return self._handle_invalid();
            }

            Char::OpenParenthesis => {
                self._paren_depth += 1;
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::OpenParenthesis,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::CloseParenthesis => {
                if self._paren_depth > 0 {
                    self._paren_depth -= 1;
                }
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::CloseParenthesis,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::OpenBracket => {
                self._paren_depth += 1;
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::OpenBracket,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::CloseBracket => {
                if self._paren_depth > 0 {
                    self._paren_depth -= 1;
                }
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::CloseBracket,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::OpenBrace => {
                self._paren_depth += 1;
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::OpenCurlyBrace,
                    self._cs.position(),
                    1,
                    comments,
                ));

                let paren_depth = self._paren_depth;
                if let Some(active_f_string) = &mut self._active_f_string {
                    // Are we starting a new replacement field?
                    if active_f_string.active_replacement_field.is_none()
                        || active_f_string
                            .active_replacement_field
                            .unwrap()
                            .in_format_specifier
                    {
                        // If there is already an active replacement field, push it
                        // on the stack so we can pop it later.
                        if let Some(active_replacement_field) =
                            active_f_string.active_replacement_field
                        {
                            active_f_string
                                .replacement_field_stack
                                .push(active_replacement_field);
                        }

                        // Create a new active replacement field context.
                        active_f_string.active_replacement_field =
                            Some(FStringReplacementFieldContext {
                                in_format_specifier: false,
                                paren_depth,
                            });
                    }
                }
            }

            Char::CloseBrace => {
                let paren_depth = self._paren_depth;
                if let Some(active_f_string) = &mut self._active_f_string
                    && active_f_string
                        .active_replacement_field
                        .map(|f| f.paren_depth)
                        == Some(paren_depth)
                {
                    active_f_string.active_replacement_field =
                        active_f_string.replacement_field_stack.pop();
                }

                if self._paren_depth > 0 {
                    self._paren_depth -= 1;
                }
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::CloseCurlyBrace,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::Comma => {
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::Comma,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::Backtick => {
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::Backtick,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::Semicolon => {
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::Semicolon,
                    self._cs.position(),
                    1,
                    comments,
                ));
            }

            Char::Colon => {
                if self._cs.next_char() == Char::Equal {
                    let is_walrus = match &self._active_f_string {
                        None => true,
                        Some(active_f_string) => match active_f_string.active_replacement_field {
                            None => true,
                            Some(field) => field.paren_depth != self._paren_depth,
                        },
                    };
                    if is_walrus {
                        let comments = self._get_comments();
                        self._tokens.push(
                            OperatorToken::create(
                                self._cs.position(),
                                2,
                                OperatorType::Walrus,
                                comments,
                            )
                            .0,
                        );
                        self._cs.advance(1);
                        return false;
                    }
                }

                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    TokenType::Colon,
                    self._cs.position(),
                    1,
                    comments,
                ));

                let paren_depth = self._paren_depth;
                if let Some(active_f_string) = &mut self._active_f_string
                    && let Some(field) = &mut active_f_string.active_replacement_field
                    && paren_depth == field.paren_depth
                {
                    field.in_format_specifier = true;
                }
            }

            _ => {
                if self._is_possible_number() {
                    if self._try_number() {
                        return true;
                    }
                }

                if self._cs.current_char() == Char::Period {
                    if self._cs.next_char() == Char::Period
                        && self._cs.look_ahead(2) == Char::Period
                    {
                        let comments = self._get_comments();
                        self._tokens.push(Token::create(
                            TokenType::Ellipsis,
                            self._cs.position(),
                            3,
                            comments,
                        ));
                        self._cs.advance(3);
                        return true;
                    }
                    let comments = self._get_comments();
                    self._tokens.push(Token::create(
                        TokenType::Dot,
                        self._cs.position(),
                        1,
                        comments,
                    ));
                    return false;
                }

                if !self._try_identifier() {
                    if !self._try_operator() {
                        return self._handle_invalid();
                    }
                }
                return true;
            }
        }
        false
    }

    // parser/tokenizer.ts:1135
    fn _add_line_range(&mut self) {
        let line_length = self._cs.position() - self._prev_line_start;
        if line_length > 0 {
            self._line_ranges.push(TextRange {
                start: self._prev_line_start,
                length: line_length,
            });
        }

        self._prev_line_start = self._cs.position();
    }

    // parser/tokenizer.ts:1144
    fn _handle_new_line(&mut self, length: i32, new_line_type: NewLineType) {
        if self._paren_depth == 0 && new_line_type != NewLineType::Implied {
            // New lines are ignored within parentheses.
            // We'll also avoid adding multiple newlines in a row to simplify parsing.
            if self._tokens.is_empty()
                || self._tokens[self._tokens.len() - 1].type_ != TokenType::NewLine
            {
                let comments = self._get_comments();
                self._tokens.push(
                    NewLineToken::create(self._cs.position(), length, new_line_type, comments).0,
                );
            }
        }
        if new_line_type == NewLineType::CarriageReturn {
            self._cr_count += 1;
        } else if new_line_type == NewLineType::CarriageReturnLineFeed {
            self._cr_lf_count += 1;
        } else {
            self._lf_count += 1;
        }
        self._cs.advance(length);
        self._add_line_range();
        self._read_indentation_after_new_line();
    }

    // parser/tokenizer.ts:1164
    fn _read_indentation_after_new_line(&mut self) {
        let mut tab1_spaces = 0;
        let mut tab8_spaces = 0;
        let mut is_tab_present = false;
        let mut is_space_present = false;

        let start_offset = self._cs.position();

        while !self._cs.is_end_of_stream() {
            match self._cs.current_char() {
                Char::Space => {
                    tab1_spaces += 1;
                    tab8_spaces += 1;
                    is_space_present = true;
                    self._cs.move_next();
                }

                Char::Tab => {
                    // Translate tabs into spaces assuming both 1-space
                    // and 8-space tab stops.
                    tab1_spaces += 1;
                    tab8_spaces += DEFAULT_TAB_SIZE - (tab8_spaces % DEFAULT_TAB_SIZE);
                    is_tab_present = true;
                    self._cs.move_next();
                }

                Char::FormFeed => {
                    tab1_spaces = 0;
                    tab8_spaces = 0;
                    is_tab_present = false;
                    is_space_present = false;
                    self._cs.move_next();
                }

                Char::Hash | Char::LineFeed | Char::CarriageReturn => {
                    // Blank line -- no need to adjust indentation.
                    return;
                }

                _ => {
                    // Non-blank line. Set the current indent level.
                    self._set_indent(
                        start_offset,
                        tab1_spaces,
                        tab8_spaces,
                        is_space_present,
                        is_tab_present,
                    );
                    return;
                }
            }
        }
    }

    // The caller must specify two space count values. The first assumes
    // that tabs are translated into one-space tab stops. The second assumes
    // that tabs are translated into eight-space tab stops.
    // parser/tokenizer.ts:1215
    fn _set_indent(
        &mut self,
        start_offset: i32,
        tab1_spaces: i32,
        tab8_spaces: i32,
        is_space_present: bool,
        is_tab_present: bool,
    ) {
        // Indentations are ignored within a parenthesized clause.
        if self._paren_depth > 0 {
            return;
        }

        // Insert indent or dedent tokens as necessary.
        if self._indent_amounts.is_empty() {
            if tab8_spaces > 0 {
                self._indent_count += 1;
                if is_tab_present {
                    self._indent_tab_count += 1;
                }
                self._indent_spaces_total += tab8_spaces;

                self._indent_amounts.push(IndentInfo {
                    tab1_spaces,
                    tab8_spaces,
                    is_space_present,
                    is_tab_present,
                });
                let comments = self._get_comments();
                self._tokens.push(
                    IndentToken::create(start_offset, tab1_spaces, tab8_spaces, false, comments).0,
                );
            }
        } else {
            let prev_tab_info = self._indent_amounts[self._indent_amounts.len() - 1];
            if prev_tab_info.tab8_spaces < tab8_spaces {
                // The Python spec says that if there is ambiguity about how tabs should
                // be translated into spaces because the user has intermixed tabs and
                // spaces, it should be an error. We'll record this condition in the token
                // so the parser can later report it.
                let is_indent_ambiguous = ((prev_tab_info.is_space_present && is_tab_present)
                    || (prev_tab_info.is_tab_present && is_space_present))
                    && prev_tab_info.tab1_spaces >= tab1_spaces;

                self._indent_count += 1;
                if is_tab_present {
                    self._indent_tab_count += 1;
                }
                self._indent_spaces_total +=
                    tab8_spaces - self._indent_amounts[self._indent_amounts.len() - 1].tab8_spaces;

                self._indent_amounts.push(IndentInfo {
                    tab1_spaces,
                    tab8_spaces,
                    is_space_present,
                    is_tab_present,
                });

                let comments = self._get_comments();
                self._tokens.push(
                    IndentToken::create(
                        start_offset,
                        tab1_spaces,
                        tab8_spaces,
                        is_indent_ambiguous,
                        comments,
                    )
                    .0,
                );
            } else if prev_tab_info.tab8_spaces == tab8_spaces {
                // The Python spec says that if there is ambiguity about how tabs should
                // be translated into spaces because the user has intermixed tabs and
                // spaces, it should be an error. We'll record this condition in the token
                // so the parser can later report it.
                if (prev_tab_info.is_space_present && is_tab_present)
                    || (prev_tab_info.is_tab_present && is_space_present)
                {
                    let comments = self._get_comments();
                    self._tokens.push(
                        IndentToken::create(start_offset, tab1_spaces, tab8_spaces, true, comments)
                            .0,
                    );
                }
            } else {
                // The Python spec says that if there is ambiguity about how tabs should
                // be translated into spaces because the user has intermixed tabs and
                // spaces, it should be an error. We'll record this condition in the token
                // so the parser can later report it.
                let mut is_dedent_ambiguous = (prev_tab_info.is_space_present && is_tab_present)
                    || (prev_tab_info.is_tab_present && is_space_present);

                // The Python spec says that dedent amounts need to match the indent
                // amount exactly. An error is generated at runtime if it doesn't.
                // We'll record that error condition within the token, allowing the
                // parser to report it later.
                let mut dedent_points: Vec<i32> = vec![];
                while !self._indent_amounts.is_empty()
                    && self._indent_amounts[self._indent_amounts.len() - 1].tab8_spaces
                        > tab8_spaces
                {
                    dedent_points.push(if self._indent_amounts.len() > 1 {
                        self._indent_amounts[self._indent_amounts.len() - 2].tab8_spaces
                    } else {
                        0
                    });
                    self._indent_amounts.pop();
                }

                for (index, &dedent_amount) in dedent_points.iter().enumerate() {
                    let matches_indent =
                        index < dedent_points.len() - 1 || dedent_amount == tab8_spaces;
                    let actual_dedent_amount = if index < dedent_points.len() - 1 {
                        dedent_amount
                    } else {
                        tab8_spaces
                    };
                    let comments = self._get_comments();
                    self._tokens.push(
                        DedentToken::create(
                            self._cs.position(),
                            0,
                            actual_dedent_amount,
                            matches_indent,
                            is_dedent_ambiguous,
                            comments,
                        )
                        .0,
                    );

                    is_dedent_ambiguous = false;
                }
            }
        }
    }

    // parser/tokenizer.ts:1326
    fn _try_identifier(&mut self) -> bool {
        let tables = tables();
        let text = self._cs.get_text();
        let text_len = text.len() as i32;
        let start = self._cs.position();

        // Fast path for ASCII identifier start.
        let first_char = self._cs.current_char();
        let mut pos = start;
        if first_char < 128 {
            if !tables.ascii_identifier_start[first_char as usize] {
                // Not an identifier start and not a surrogate candidate.
                return false;
            }
            pos += 1;

            // Tight loop: advance while we're still in ASCII identifier chars.
            while pos < text_len {
                let ch = text[pos as usize];
                if ch < 128 && tables.ascii_identifier_continue[ch as usize] {
                    pos += 1;
                } else {
                    break;
                }
            }

            // If we hit a non-ASCII char, fall back to the generic loop to
            // handle possible unicode identifier continue / surrogate pairs.
            if pos < text_len && text[pos as usize] >= 128 {
                self._cs.advance(pos - start);
                self._swallow_non_ascii_identifier_chars();
                pos = self._cs.position();
            } else {
                self._cs.advance(pos - start);
            }
        } else {
            // Non-ASCII start: use the generic path (supports surrogates).
            if is_identifier_start_char(first_char, None) {
                self._cs.move_next();
            } else if is_identifier_start_char(first_char, Some(self._cs.next_char())) {
                self._cs.move_next();
                self._cs.move_next();
            } else {
                return false;
            }
            self._swallow_non_ascii_identifier_chars();
            pos = self._cs.position();
        }

        if pos > start {
            let end = pos;
            let length = end - start;
            let keyword_type = get_keyword_type_from_text_slice(text, start, length);

            if let Some(keyword_type) = keyword_type {
                let comments = self._get_comments();
                self._tokens
                    .push(KeywordToken::create(start, length, keyword_type, comments).0);
            } else {
                let value = self._intern_identifier(text, start, end, length);
                let comments = self._get_comments();
                self._tokens
                    .push(IdentifierToken::create(start, length, value, comments).0);
            }
            return true;
        }
        false
    }

    // Per-tokenize identifier intern cache. Direct-mapped, so collisions
    // simply replace the slot.
    // parser/tokenizer.ts:1397
    fn _intern_identifier(&mut self, text: &[u16], start: i32, end: i32, length: i32) -> Rc<str> {
        let first_char = text[start as usize] as i32;
        let last_char = text[(end - 1) as usize] as i32;
        // Hash mixes length, first and last char.
        let hash = ((first_char * 31 + last_char * 7 + length) & _IDENTIFIER_CACHE_MASK) as usize;
        if let Some((cached_text, cached)) = &self._identifier_cache[hash]
            && cached_text.len() as i32 == length
            && text[start as usize..].starts_with(cached_text)
        {
            return cached.clone();
        }
        let value: Rc<str> = detach_substring(text, start, end).into();
        self._identifier_cache[hash] =
            Some((text[start as usize..end as usize].into(), value.clone()));
        value
    }

    // Generic identifier-continue loop that handles unicode + surrogate pairs.
    // parser/tokenizer.ts:1414
    fn _swallow_non_ascii_identifier_chars(&mut self) {
        loop {
            if is_identifier_char(self._cs.current_char(), None) {
                self._cs.move_next();
            } else if is_identifier_char(self._cs.current_char(), Some(self._cs.next_char())) {
                self._cs.move_next();
                self._cs.move_next();
            } else {
                break;
            }
        }
    }

    // parser/tokenizer.ts:1427
    fn _is_possible_number(&self) -> bool {
        if is_decimal(self._cs.current_char()) {
            return true;
        }

        if self._cs.current_char() == Char::Period && is_decimal(self._cs.next_char()) {
            return true;
        }

        false
    }

    // parser/tokenizer.ts:1439
    fn _try_number(&mut self) -> bool {
        let start = self._cs.position();

        if self._cs.current_char() == Char::_0 {
            let mut radix = 0;
            let mut leading_chars = 0;

            // Try hex => hexinteger: "0" ("x" | "X") (["_"] hexdigit)+
            if (self._cs.next_char() == Char::x || self._cs.next_char() == Char::X)
                && is_hex(self._cs.look_ahead(2))
            {
                self._cs.advance(2);
                leading_chars = 2;
                while is_hex(self._cs.current_char()) {
                    self._cs.move_next();
                }
                radix = 16;
            }
            // Try binary => bininteger: "0" ("b" | "B") (["_"] bindigit)+
            else if (self._cs.next_char() == Char::b || self._cs.next_char() == Char::B)
                && is_binary(self._cs.look_ahead(2))
            {
                self._cs.advance(2);
                leading_chars = 2;
                while is_binary(self._cs.current_char()) {
                    self._cs.move_next();
                }
                radix = 2;
            }
            // Try octal => octinteger: "0" ("o" | "O") (["_"] octdigit)+
            else if (self._cs.next_char() == Char::o || self._cs.next_char() == Char::O)
                && is_octal(self._cs.look_ahead(2))
            {
                self._cs.advance(2);
                leading_chars = 2;
                while is_octal(self._cs.current_char()) {
                    self._cs.move_next();
                }
                radix = 8;
            }

            if radix > 0 {
                let end = self._cs.position();
                let text = self._cs.get_text();
                let simple_int_text = remove_underscores_from_range(text, start, end);
                let int_value = parse_int(&simple_int_text[leading_chars..], radix);

                if !int_value.is_nan() {
                    let big_int_value =
                        parse_bigint(&simple_int_text).expect("BigInt of a validated literal");
                    let int_value = if !int_value.is_finite()
                        || int_value < MIN_SAFE_INTEGER
                        || int_value > MAX_SAFE_INTEGER
                    {
                        NumberValue::BigInt(big_int_value)
                    } else {
                        NumberValue::Number(int_value)
                    };

                    let comments = self._get_comments();
                    self._tokens.push(
                        NumberToken::create(start, end - start, int_value, true, false, comments).0,
                    );
                    return true;
                }
            }
        }

        let mut is_decimal_integer = false;
        let mut might_be_floating_point = false;
        // Try decimal int =>
        //    decinteger: nonzerodigit (["_"] digit)* | "0" (["_"] "0")*
        //    nonzerodigit: "1"..."9"
        //    digit: "0"..."9"
        if self._cs.current_char() >= Char::_1 && self._cs.current_char() <= Char::_9 {
            while is_decimal(self._cs.current_char()) {
                might_be_floating_point = true;
                self._cs.move_next();
            }
            is_decimal_integer = self._cs.current_char() != Char::Period
                && self._cs.current_char() != Char::e
                && self._cs.current_char() != Char::E;
        }

        // "0" (["_"] "0")*
        if self._cs.current_char() == Char::_0 {
            might_be_floating_point = true;
            while self._cs.current_char() == Char::_0 || self._cs.current_char() == Char::Underscore
            {
                self._cs.move_next();
            }
            is_decimal_integer = self._cs.current_char() != Char::Period
                && self._cs.current_char() != Char::e
                && self._cs.current_char() != Char::E
                && (self._cs.current_char() < Char::_1 || self._cs.current_char() > Char::_9);
        }

        if is_decimal_integer {
            let text_end = self._cs.position();
            let source_text = self._cs.get_text();
            let simple_int_text = remove_underscores_from_range(source_text, start, text_end);
            let int_value = parse_int(&simple_int_text, 10);

            if !int_value.is_nan() {
                let mut is_imaginary = false;
                let mut token_length = text_end - start;

                let big_int_value =
                    parse_bigint(&simple_int_text).expect("BigInt of a validated literal");
                let int_value = if !int_value.is_finite()
                    || big_int_value < MIN_SAFE_INTEGER
                    || big_int_value > MAX_SAFE_INTEGER
                {
                    NumberValue::BigInt(big_int_value)
                } else {
                    NumberValue::Number(int_value)
                };

                if self._cs.current_char() == Char::j || self._cs.current_char() == Char::J {
                    is_imaginary = true;
                    self._cs.move_next();
                    token_length += 1;
                }

                let comments = self._get_comments();
                self._tokens.push(
                    NumberToken::create(
                        start,
                        token_length,
                        int_value,
                        true,
                        is_imaginary,
                        comments,
                    )
                    .0,
                );
                return true;
            }
        }

        // Floating point. Sign and leading digits were already skipped over.
        self._cs.set_position(start);
        if might_be_floating_point
            || (self._cs.current_char() == Char::Period
                && self._cs.next_char() >= Char::_0
                && self._cs.next_char() <= Char::_9)
        {
            if self._skip_floating_point_candidate() {
                let float_end = self._cs.position();
                let float_text =
                    remove_underscores_from_range(self._cs.get_text(), start, float_end);
                let value = parse_float(&float_text);
                if !value.is_nan() {
                    let mut is_imaginary = false;
                    let mut token_length = float_end - start;
                    if self._cs.current_char() == Char::j || self._cs.current_char() == Char::J {
                        is_imaginary = true;
                        self._cs.move_next();
                        token_length += 1;
                    }
                    let comments = self._get_comments();
                    self._tokens.push(
                        NumberToken::create(
                            start,
                            token_length,
                            NumberValue::Number(value),
                            false,
                            is_imaginary,
                            comments,
                        )
                        .0,
                    );
                    return true;
                }
            }
        }

        self._cs.set_position(start);
        false
    }

    // parser/tokenizer.ts:1595
    fn _try_operator(&mut self) -> bool {
        let current_char = self._cs.current_char();
        let length;
        let next_char = self._cs.next_char();
        let operator_type: OperatorType;

        if current_char < 128 && next_char < 128 {
            let two_char_key = get_two_char_key(current_char, next_char);
            let special_token_type = _TWO_CHAR_SPECIAL_TOKEN_TYPE_MAP
                .iter()
                .find(|(k, _)| *k == two_char_key)
                .map(|&(_, t)| t);
            if let Some(special_token_type) = special_token_type {
                let comments = self._get_comments();
                self._tokens.push(Token::create(
                    special_token_type,
                    self._cs.position(),
                    2,
                    comments,
                ));
                self._cs.advance(2);
                return true;
            }

            let two_char_operator_type = _TWO_CHAR_OPERATOR_TYPE_MAP
                .iter()
                .find(|(k, _)| *k == two_char_key)
                .map(|&(_, t)| t);
            if let Some(two_char_operator_type) = two_char_operator_type {
                let comments = self._get_comments();
                self._tokens.push(
                    OperatorToken::create(self._cs.position(), 2, two_char_operator_type, comments)
                        .0,
                );
                self._cs.advance(2);
                return true;
            }

            if current_char == next_char {
                let repeated_operator_type =
                    _REPEATED_CHAR_OPERATOR_TYPE_TABLE[current_char as usize];
                if repeated_operator_type != _UNSET_SINGLE_CHAR_OPERATOR_TYPE {
                    let has_trailing_equal = self._cs.look_ahead(2) == Char::Equal;
                    let repeated_length = if has_trailing_equal { 3 } else { 2 };
                    let operator_type = if has_trailing_equal {
                        _REPEATED_CHAR_EQUAL_OPERATOR_TYPE_TABLE[current_char as usize]
                    } else {
                        repeated_operator_type
                    };
                    let comments = self._get_comments();
                    self._tokens.push(
                        OperatorToken::create(
                            self._cs.position(),
                            repeated_length,
                            operator_type_from_table(operator_type),
                            comments,
                        )
                        .0,
                    );
                    self._cs.advance(repeated_length);
                    return true;
                }
            }
        }

        if current_char < 128 {
            let single_char_operator_type = _SINGLE_CHAR_OPERATOR_TYPE_TABLE[current_char as usize];
            if single_char_operator_type != _UNSET_SINGLE_CHAR_OPERATOR_TYPE {
                let equal_operator_type =
                    _SINGLE_CHAR_EQUAL_OPERATOR_TYPE_TABLE[current_char as usize];
                if next_char == Char::Equal
                    && equal_operator_type != _UNSET_SINGLE_CHAR_OPERATOR_TYPE
                {
                    length = 2;
                    operator_type = operator_type_from_table(equal_operator_type);
                } else {
                    length = 1;
                    operator_type = operator_type_from_table(single_char_operator_type);
                }

                let comments = self._get_comments();
                self._tokens.push(
                    OperatorToken::create(self._cs.position(), length, operator_type, comments).0,
                );
                self._cs.advance(length);
                return true;
            }
        }

        // `!=` is handled by the 2-char fast path above.
        if current_char == Char::ExclamationMark && self._active_f_string.is_some() {
            // Handle the conversion separator (!) within an f-string.
            let comments = self._get_comments();
            self._tokens.push(Token::create(
                TokenType::ExclamationMark,
                self._cs.position(),
                1,
                comments,
            ));
            self._cs.advance(1);
            return true;
        }

        false
    }

    // parser/tokenizer.ts:1668
    fn _handle_invalid(&mut self) -> bool {
        let start = self._cs.position();
        loop {
            if self._cs.current_char() == Char::LineFeed
                || self._cs.current_char() == Char::CarriageReturn
                || self._cs.is_at_white_space()
                || self._cs.is_end_of_stream()
            {
                break;
            }

            if is_surrogate_char(self._cs.current_char()) {
                self._cs.move_next();
                self._cs.move_next();
            } else {
                self._cs.move_next();
            }
        }
        let length = self._cs.position() - start;
        if length > 0 {
            let comments = self._get_comments();
            self._tokens
                .push(Token::create(TokenType::Invalid, start, length, comments));
            return true;
        }
        false
    }

    // parser/tokenizer.ts:1695
    fn _get_comments(&mut self) -> Option<Vec<Comment>> {
        self._comments.take()
    }

    // parser/tokenizer.ts:1701
    fn _get_i_python_magics_kind(&mut self) -> MagicsKind {
        let cur_char = self._cs.current_char();
        if cur_char != Char::Percent && cur_char != Char::ExclamationMark {
            return MagicsKind::None;
        }

        let prev_token = self._tokens.last();
        if let Some(prev_token) = prev_token
            && !Tokenizer::is_whitespace(prev_token)
        {
            return MagicsKind::None;
        }

        if self._cs.next_char() == cur_char {
            // Eat up next magic char.
            self._cs.move_next();
            return MagicsKind::Cell;
        }

        MagicsKind::Line
    }

    // parser/tokenizer.ts:1721
    fn _handle_i_python_magics(&mut self, type_: CommentType) {
        let start = self._cs.position() + 1;
        let source_text = self._cs.get_text();

        let mut begin = start;
        loop {
            self._cs.skip_to_eol();

            if type_ == CommentType::IPythonMagic || type_ == CommentType::IPythonShellEscape {
                // is it multiline magics?
                // %magic command \
                //        next arguments
                if !ends_with_backslash_continuation(source_text, begin, self._cs.position()) {
                    break;
                }
            }

            self._cs.move_next();
            begin = self._cs.position() + 1;

            if self._cs.is_end_of_stream() {
                break;
            }
        }

        let length = self._cs.position() - start;
        let comment = Comment::create(
            start,
            length,
            js_slice_lossy(source_text, start, start + length),
            Some(type_),
        );
        self._add_comments(comment);
    }

    // parser/tokenizer.ts:1751
    fn _handle_comment(&mut self) {
        let start = self._cs.position() + 1;
        self._cs.skip_to_eol();

        let length = self._cs.position() - start;
        let source_text = self._cs.get_text();
        let end = start + length;
        let comment_value = &source_text[start as usize..end as usize];
        let comment = Comment::create(start, length, String::from_utf16_lossy(comment_value), None);

        // Fast pre-filter: any ignore directive must contain the substring 'ignore'.
        // Search only the current comment so comment-heavy files remain linear in size.
        if comment_value.js_includes(&"ignore".encode_utf16().collect::<Vec<u16>>(), 0) {
            if let Some(type_ignore_match) = match_ignore_directive(source_text, start, end, "type")
            {
                let comment_start = type_ignore_match.index;
                let text_range = TextRange {
                    start: comment_start + type_ignore_match.prefix.len() as i32,
                    length: type_ignore_match.full_match.len() as i32
                        - type_ignore_match.prefix.len() as i32,
                };
                let ignore_comment = IgnoreComment {
                    range: text_range,
                    rules_list: self
                        ._get_ignore_comment_rules_list(comment_start, &type_ignore_match),
                };

                let mut is_ignore_all = false;
                if !self._has_token_before_ignore_all {
                    // Are there any tokens other than NewLine / Indent yet?
                    let has_other = self
                        ._tokens
                        .iter()
                        .any(|t| t.type_ != TokenType::NewLine && t.type_ != TokenType::Indent);
                    if has_other {
                        self._has_token_before_ignore_all = true;
                    } else {
                        is_ignore_all = true;
                    }
                }

                if is_ignore_all {
                    self._type_ignore_all = Some(ignore_comment);
                } else {
                    self._type_ignore_lines
                        .insert(self._line_ranges.len() as i32, ignore_comment);
                }
            }

            if let Some(pyright_ignore_match) =
                match_ignore_directive(source_text, start, end, "pyright")
            {
                let comment_start = pyright_ignore_match.index;
                let text_range = TextRange {
                    start: comment_start + pyright_ignore_match.prefix.len() as i32,
                    length: pyright_ignore_match.full_match.len() as i32
                        - pyright_ignore_match.prefix.len() as i32,
                };
                let ignore_comment = IgnoreComment {
                    range: text_range,
                    rules_list: self
                        ._get_ignore_comment_rules_list(comment_start, &pyright_ignore_match),
                };
                self._pyright_ignore_lines
                    .insert(self._line_ranges.len() as i32, ignore_comment);
            }
        }

        self._add_comments(comment);
    }

    // Extracts the individual rules within a "type: ignore [x, y, z]" comment.
    // parser/tokenizer.ts:1814
    fn _get_ignore_comment_rules_list(
        &self,
        start: i32,
        m: &IgnoreDirectiveMatch<'_>,
    ) -> Option<Vec<IgnoreCommentRule>> {
        let bracket_content = m.bracket_content?;

        let comma: [u16; 1] = [Char::Comma];
        let open_bracket: [u16; 1] = [Char::OpenBracket];
        let split_elements = bracket_content.js_split(&comma, None);
        let mut comment_rules: Vec<IgnoreCommentRule> = vec![];
        let mut current_offset = start + m.full_match.js_index_of(&open_bracket, 0) + 1;

        for element in split_elements {
            let front_trimmed = element.js_trim_start();
            current_offset += element.len() as i32 - front_trimmed.len() as i32;
            let end_trimmed = front_trimmed.js_trim_end();

            if !end_trimmed.is_empty() {
                comment_rules.push(IgnoreCommentRule {
                    range: TextRange {
                        start: current_offset,
                        length: end_trimmed.len() as i32,
                    },
                    text: String::from_utf16_lossy(end_trimmed),
                });
            }

            current_offset += front_trimmed.len() as i32 + 1;
        }

        Some(comment_rules)
    }

    // parser/tokenizer.ts:1841
    fn _add_comments(&mut self, comment: Comment) {
        if let Some(comments) = &mut self._comments {
            comments.push(comment);
        } else {
            self._comments = Some(vec![comment]);
        }
    }

    // parser/tokenizer.ts:1849
    fn _get_string_prefix_length(&self) -> i32 {
        if self._cs.current_char() == Char::SingleQuote
            || self._cs.current_char() == Char::DoubleQuote
        {
            // Simple string, no prefix
            return 0;
        }

        if self._cs.next_char() == Char::SingleQuote || self._cs.next_char() == Char::DoubleQuote {
            match self._cs.current_char() {
                Char::f
                | Char::F
                | Char::r
                | Char::R
                | Char::b
                | Char::B
                | Char::u
                | Char::U
                | Char::t
                | Char::T => {
                    // Single-char prefix like u"" or r""
                    return 1;
                }
                _ => {}
            }
        }

        if self._cs.look_ahead(2) == Char::SingleQuote
            || self._cs.look_ahead(2) == Char::DoubleQuote
        {
            let position = self._cs.position() as usize;
            let text = self._cs.get_text();
            let prefix = text[position..(position + 2).min(text.len())].js_to_lower_case();
            for candidate in ["rf", "fr", "rt", "tr", "br", "rb"] {
                if prefix == *candidate {
                    return 2;
                }
            }
        }
        -1
    }

    // parser/tokenizer.ts:1894
    fn _get_quote_type_flags(&self, prefix: &[u16]) -> StringTokenFlags {
        let mut flags = StringTokenFlags::None;

        let prefix = prefix.js_to_lower_case();
        for &c in prefix.iter() {
            match c {
                Char::u => flags |= StringTokenFlags::Unicode,
                Char::b => flags |= StringTokenFlags::Bytes,
                Char::r => flags |= StringTokenFlags::Raw,
                Char::f => flags |= StringTokenFlags::Format,
                Char::t => flags |= StringTokenFlags::Template,
                _ => {}
            }
        }

        if self._cs.current_char() == Char::SingleQuote {
            flags |= StringTokenFlags::SingleQuote;
            if self._cs.next_char() == Char::SingleQuote
                && self._cs.look_ahead(2) == Char::SingleQuote
            {
                flags |= StringTokenFlags::Triplicate;
            }
        } else if self._cs.current_char() == Char::DoubleQuote {
            flags |= StringTokenFlags::DoubleQuote;
            if self._cs.next_char() == Char::DoubleQuote
                && self._cs.look_ahead(2) == Char::DoubleQuote
            {
                flags |= StringTokenFlags::Triplicate;
            }
        }

        flags
    }

    // parser/tokenizer.ts:1937
    fn _handle_string(&mut self, flags: StringTokenFlags, string_prefix_length: i32) {
        let start = self._cs.position() - string_prefix_length;

        if flags.any(StringTokenFlags::Format | StringTokenFlags::Template) {
            if flags.any(StringTokenFlags::Triplicate) {
                self._cs.advance(3);
            } else {
                self._cs.move_next();
            }

            let end = self._cs.position();

            let comments = self._get_comments();
            let f_string_start_token = FStringStartToken::create(
                start,
                end - start,
                flags,
                string_prefix_length,
                comments,
            );

            // Create a new f-string context and push it on the stack.
            let f_string_context = FStringContext {
                start_token: f_string_start_token,
                replacement_field_stack: vec![],
                active_replacement_field: None,
            };

            if let Some(active_f_string) = self._active_f_string.take() {
                self._f_string_stack.push(active_f_string);
            }
            self._active_f_string = Some(f_string_context);

            self._tokens.push(f_string_start_token.0);
        } else {
            if flags.any(StringTokenFlags::Triplicate) {
                self._cs.advance(3);
            } else {
                self._cs.move_next();

                if flags.any(StringTokenFlags::SingleQuote) {
                    self._single_quote_count += 1;
                } else {
                    self._double_quote_count += 1;
                }
            }

            let string_literal_info = self._skip_to_end_of_string_literal(flags, false);
            let end = self._cs.position();

            // If this is an unterminated string, see if it matches the string type
            // of an active f-string. If so, we'll treat it as an f-string end
            // token rather than an unterminated regular string. This helps with
            // parse error recovery if a closing bracket is missing in an f-string.
            if string_literal_info
                .flags
                .any(StringTokenFlags::Unterminated)
                && let Some(active_f_string) = &mut self._active_f_string
                && active_f_string.active_replacement_field.is_some()
            {
                if (flags
                    & (StringTokenFlags::Bytes
                        | StringTokenFlags::Unicode
                        | StringTokenFlags::Raw
                        | StringTokenFlags::Format
                        | StringTokenFlags::Template))
                    == StringTokenFlags::None
                {
                    let quote_type_mask = StringTokenFlags::Triplicate
                        | StringTokenFlags::DoubleQuote
                        | StringTokenFlags::SingleQuote;
                    if (active_f_string.start_token.d().flags & quote_type_mask)
                        == (flags & quote_type_mask)
                    {
                        // Unwind to the start of this string token and terminate any replacement fields
                        // that are active. This will cause the tokenizer to re-process the quote as an
                        // FStringEnd token.
                        self._cs.set_position(start);
                        while !active_f_string.replacement_field_stack.is_empty() {
                            active_f_string.active_replacement_field =
                                active_f_string.replacement_field_stack.pop();
                        }
                        self._paren_depth = active_f_string
                            .active_replacement_field
                            .unwrap()
                            .paren_depth
                            - 1;
                        active_f_string.active_replacement_field = None;
                        return;
                    }
                }
            }

            let comments = self._get_comments();
            self._tokens.push(
                StringToken::create(
                    start,
                    end - start,
                    string_literal_info.flags,
                    string_literal_info.escaped_value,
                    string_prefix_length,
                    comments,
                )
                .0,
            );
        }
    }

    // Scans for either the FString end token or a replacement field.
    // parser/tokenizer.ts:2034
    fn _handle_f_string_middle(&mut self) {
        let active_f_string = self._active_f_string.as_ref().unwrap();
        let in_format_specifier = active_f_string
            .active_replacement_field
            .is_some_and(|field| field.in_format_specifier);
        let start_token = active_f_string.start_token;
        let start = self._cs.position();
        let flags = start_token.d().flags;
        let string_literal_info = self._skip_to_end_of_string_literal(flags, in_format_specifier);
        let end = self._cs.position();

        let is_unterminated = string_literal_info
            .flags
            .any(StringTokenFlags::Unterminated);
        let saw_replacement_field_start = string_literal_info
            .flags
            .any(StringTokenFlags::ReplacementFieldStart);
        let saw_replacement_field_end = string_literal_info
            .flags
            .any(StringTokenFlags::ReplacementFieldEnd);
        let saw_end_quote =
            !is_unterminated && !saw_replacement_field_start && !saw_replacement_field_end;

        let mut middle_token_length = end - start;
        if saw_end_quote {
            middle_token_length -= start_token.d().quote_mark_length;
        }

        if middle_token_length > 0 || is_unterminated {
            self._tokens.push(
                FStringMiddleToken::create(
                    start,
                    middle_token_length,
                    string_literal_info.flags,
                    string_literal_info.escaped_value,
                )
                .0,
            );
        }

        if saw_end_quote {
            self._tokens.push(
                FStringEndToken::create(
                    start + middle_token_length,
                    start_token.d().quote_mark_length,
                    string_literal_info.flags,
                )
                .0,
            );

            self._active_f_string = self._f_string_stack.pop();
        } else if is_unterminated {
            self._active_f_string = self._f_string_stack.pop();
        }
    }

    // parser/tokenizer.ts:2078
    fn _skip_to_end_of_string_literal(
        &mut self,
        mut flags: StringTokenFlags,
        in_format_specifier: bool,
    ) -> StringScannerOutput {
        let quote_char = if flags.any(StringTokenFlags::SingleQuote) {
            Char::SingleQuote
        } else {
            Char::DoubleQuote
        };
        let is_triplicate = flags.any(StringTokenFlags::Triplicate);
        let is_f_string = flags.any(StringTokenFlags::Format | StringTokenFlags::Template);
        let mut is_in_named_unicode_escape = false;
        let start = self._cs.position();
        let mut escaped_value_length = 0;
        let get_escaped_value = |cs: &CharacterStream<'_>, escaped_value_length: i32| {
            JsString::from(&cs.get_text()[start as usize..(start + escaped_value_length) as usize])
        };

        loop {
            if self._cs.is_end_of_stream() {
                // Hit the end of file without a termination.
                flags |= StringTokenFlags::Unterminated;
                return StringScannerOutput {
                    escaped_value: get_escaped_value(&self._cs, escaped_value_length),
                    flags,
                };
            }

            if self._cs.current_char() == Char::Backslash {
                escaped_value_length += 1;

                // Move past the escape (backslash) character.
                self._cs.move_next();

                // Handle the special escape sequence /N{name} for unicode characters.
                if !is_in_named_unicode_escape
                    && self._cs.get_current_char() == Char::N
                    && self._cs.next_char() == Char::OpenBrace
                {
                    flags |= StringTokenFlags::NamedUnicodeEscape;
                    is_in_named_unicode_escape = true;
                } else {
                    // If this is an f-string, the only escapes that are allowed is for
                    // a single or double quote symbol or a newline/carriage return.
                    let is_escaped_quote = self._cs.get_current_char() == Char::SingleQuote
                        || self._cs.get_current_char() == Char::DoubleQuote;
                    let is_escaped_new_line = self._cs.get_current_char() == Char::CarriageReturn
                        || self._cs.get_current_char() == Char::LineFeed;
                    let is_escaped_backslash = self._cs.get_current_char() == Char::Backslash;

                    if !is_f_string
                        || is_escaped_backslash
                        || is_escaped_quote
                        || is_escaped_new_line
                    {
                        if is_escaped_new_line {
                            if self._cs.get_current_char() == Char::CarriageReturn
                                && self._cs.next_char() == Char::LineFeed
                            {
                                escaped_value_length += 1;
                                self._cs.move_next();
                            }
                            escaped_value_length += 1;
                            self._cs.move_next();
                            self._add_line_range();
                        } else {
                            escaped_value_length += 1;
                            self._cs.move_next();
                        }
                    }
                }
            } else if self._cs.current_char() == Char::LineFeed
                || self._cs.current_char() == Char::CarriageReturn
            {
                if !is_triplicate {
                    let in_replacement_field = self
                        ._active_f_string
                        .as_ref()
                        .is_some_and(|f| f.active_replacement_field.is_some());
                    if !is_f_string || !in_replacement_field {
                        // Unterminated single-line string
                        flags |= StringTokenFlags::Unterminated;
                        return StringScannerOutput {
                            escaped_value: get_escaped_value(&self._cs, escaped_value_length),
                            flags,
                        };
                    }
                }

                // Skip over the new line (either one or two characters).
                if self._cs.current_char() == Char::CarriageReturn
                    && self._cs.next_char() == Char::LineFeed
                {
                    escaped_value_length += 1;
                    self._cs.move_next();
                }

                escaped_value_length += 1;
                self._cs.move_next();
                self._add_line_range();
            } else if !is_triplicate && self._cs.current_char() == quote_char {
                self._cs.move_next();
                break;
            } else if is_triplicate
                && self._cs.current_char() == quote_char
                && self._cs.next_char() == quote_char
                && self._cs.look_ahead(2) == quote_char
            {
                self._cs.advance(3);
                break;
            } else if !is_in_named_unicode_escape
                && is_f_string
                && self._cs.current_char() == Char::OpenBrace
            {
                if in_format_specifier || self._cs.next_char() != Char::OpenBrace {
                    flags |= StringTokenFlags::ReplacementFieldStart;
                    break;
                } else {
                    escaped_value_length += 1;
                    self._cs.move_next();
                    escaped_value_length += 1;
                    self._cs.move_next();
                }
            } else if is_in_named_unicode_escape && self._cs.current_char() == Char::CloseBrace {
                is_in_named_unicode_escape = false;
                escaped_value_length += 1;
                self._cs.move_next();
            } else if is_f_string && self._cs.current_char() == Char::CloseBrace {
                if in_format_specifier || self._cs.next_char() != Char::CloseBrace {
                    flags |= StringTokenFlags::ReplacementFieldEnd;
                    break;
                } else {
                    escaped_value_length += 1;
                    self._cs.move_next();
                    escaped_value_length += 1;
                    self._cs.move_next();
                }
            } else {
                escaped_value_length += 1;
                self._cs.move_next();
            }
        }

        StringScannerOutput {
            escaped_value: get_escaped_value(&self._cs, escaped_value_length),
            flags,
        }
    }

    // parser/tokenizer.ts:2208
    fn _skip_floating_point_candidate(&mut self) -> bool {
        // Determine end of the potential floating point number
        let start = self._cs.position();
        self._skip_fractional_number();
        if self._cs.position() > start {
            // Optional exponent sign
            if self._cs.current_char() == Char::e || self._cs.current_char() == Char::E {
                self._cs.move_next();

                // Skip exponent value
                self._skip_decimal_number(/* allow_sign */ true);
            }
        }
        self._cs.position() > start
    }

    // parser/tokenizer.ts:2224
    fn _skip_fractional_number(&mut self) {
        self._skip_decimal_number(false);
        if self._cs.current_char() == Char::Period {
            // Optional period
            self._cs.move_next();
        }
        self._skip_decimal_number(false);
    }

    // parser/tokenizer.ts:2233
    fn _skip_decimal_number(&mut self, allow_sign: bool) {
        if allow_sign
            && (self._cs.current_char() == Char::Hyphen || self._cs.current_char() == Char::Plus)
        {
            // Optional sign
            self._cs.move_next();
        }
        while is_decimal(self._cs.current_char()) {
            // Skip integer part
            self._cs.move_next();
        }
    }
}

/// `Number.MIN_SAFE_INTEGER`.
const MIN_SAFE_INTEGER: f64 = -9007199254740991.0;
/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: f64 = 9007199254740991.0;

/// `text.slice(start, end)` as a `String`; a lone surrogate becomes U+FFFD.
fn js_slice_lossy(text: &[u16], start: i32, end: i32) -> String {
    let len = text.len() as i32;
    let start = start.clamp(0, len) as usize;
    let end = end.clamp(0, len) as usize;
    if start >= end {
        return String::new();
    }
    String::from_utf16_lossy(&text[start..end])
}

#[cfg(test)]
#[path = "tokenizer_tests.rs"]
mod tests;
