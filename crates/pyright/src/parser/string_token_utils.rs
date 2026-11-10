//! Port of `packages/pyright-internal/src/parser/stringTokenUtils.ts`.

use jsrt::JsString;
use jsrt::string::from_char_code;

use super::tokenizer_types::{StringTokenFlags, Token, TokenDetails};
use crate::common::char_codes::Char;

// parser/stringTokenUtils.ts:14
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnescapeErrorType {
    InvalidEscapeSequence,
}

// parser/stringTokenUtils.ts:18
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnescapeError {
    // Offset within the unescaped string where
    // this error begins.
    pub offset: i32,

    // Length of section associated with error.
    pub length: i32,

    // Type of error.
    pub error_type: UnescapeErrorType,
}

// parser/stringTokenUtils.ts:30
#[derive(Clone, Debug)]
pub struct UnescapedString {
    pub value: JsString,
    pub unescape_errors: Vec<UnescapeError>,
    pub non_ascii_in_bytes: bool,
}

// parser/stringTokenUtils.ts:36
struct IncompleteUnescapedString {
    value_parts: Vec<u16>,
    unescape_errors: Vec<UnescapeError>,
    non_ascii_in_bytes: bool,
}

// parser/stringTokenUtils.ts:42
fn complete_unescaped_string(
    incomplete: IncompleteUnescapedString,
    original_string: &JsString,
) -> UnescapedString {
    let new_value = JsString::from(incomplete.value_parts);
    // Use the original string if it's identical.
    let value = if **original_string != *new_value {
        new_value
    } else {
        original_string.clone()
    };
    UnescapedString {
        value,
        unescape_errors: incomplete.unescape_errors,
        non_ascii_in_bytes: incomplete.non_ascii_in_bytes,
    }
}

/// The token's `flags` and `escapedValue`. Only `StringToken` and
/// `FStringMiddleToken` reach `getUnescapedString`.
fn flags_and_escaped_value(token: &Token) -> (StringTokenFlags, &JsString) {
    match &token.d {
        TokenDetails::String(d) => (d.flags, &d.escaped_value),
        TokenDetails::FStringMiddle(d) => (d.flags, &d.escaped_value),
        _ => unreachable!("getUnescapedString takes a StringToken or FStringMiddleToken"),
    }
}

// parser/stringTokenUtils.ts:54
pub fn get_unescaped_string(string_token: &Token, elide_crlf: bool) -> UnescapedString {
    let (flags, escaped_string) = flags_and_escaped_value(string_token);
    let is_raw = flags.any(StringTokenFlags::Raw);

    if is_raw {
        return UnescapedString {
            value: escaped_string.clone(),
            unescape_errors: vec![],
            non_ascii_in_bytes: false,
        };
    }

    let is_bytes = flags.any(StringTokenFlags::Bytes);

    // Scan once for the characters that force the slow (unescaping) path.
    let mut has_escape_or_new_line = false;
    let mut non_ascii_in_bytes = false;
    for &cur_char in escaped_string.iter() {
        if cur_char == Char::CarriageReturn
            || cur_char == Char::LineFeed
            || cur_char == Char::Backslash
        {
            has_escape_or_new_line = true;
            break;
        }
        if is_bytes && cur_char >= 128 {
            non_ascii_in_bytes = true;
        }
    }

    // Handle the common case in an expedited manner.
    if !has_escape_or_new_line {
        return UnescapedString {
            value: escaped_string.clone(),
            unescape_errors: vec![],
            non_ascii_in_bytes,
        };
    }

    let mut str_offset: i32 = 0;
    let mut output = IncompleteUnescapedString {
        value_parts: vec![],
        unescape_errors: vec![],
        non_ascii_in_bytes: false,
    };

    let add_invalid_escape_offset = |output: &mut IncompleteUnescapedString, str_offset: i32| {
        // Invalid escapes are not reported for raw strings.
        if !is_raw {
            output.unescape_errors.push(UnescapeError {
                offset: str_offset - 1,
                length: 2,
                error_type: UnescapeErrorType::InvalidEscapeSequence,
            });
        }
    };

    let get_escaped_character = |str_offset: i32, offset: i32| -> u16 {
        if str_offset + offset >= escaped_string.len() as i32 {
            return Char::EndOfText;
        }

        escaped_string[(str_offset + offset) as usize]
    };

    let scan_hex_escape = |output: &mut IncompleteUnescapedString,
                           str_offset: &mut i32,
                           digit_count: i32|
     -> Vec<u16> {
        let mut found_illegal_hex_digit = false;
        let mut hex_value: f64 = 0.0;
        let local_value: Vec<u16>;

        for i in 0..digit_count {
            let char_code = get_escaped_character(*str_offset, 1 + i);
            if !_is_hex_char_code(char_code) {
                found_illegal_hex_digit = true;
                break;
            }
            hex_value = 16.0 * hex_value + _get_hex_digit_value(char_code) as f64;
        }

        if found_illegal_hex_digit {
            add_invalid_escape_offset(output, *str_offset);
            local_value = vec![Char::Backslash, get_escaped_character(*str_offset, 0)];
            *str_offset += 1;
        } else {
            local_value = vec![from_char_code(hex_value as u32)];
            *str_offset += 1 + digit_count;
        }

        local_value
    };

    loop {
        let mut cur_char = get_escaped_character(str_offset, 0);
        if cur_char == Char::EndOfText {
            return complete_unescaped_string(output, escaped_string);
        }

        if cur_char == Char::Backslash {
            // Move past the escape (backslash) character.
            str_offset += 1;

            if is_raw {
                output.value_parts.push(cur_char);
                continue;
            }

            cur_char = get_escaped_character(str_offset, 0);
            let mut local_value: Vec<u16> = vec![];

            if cur_char == Char::CarriageReturn || cur_char == Char::LineFeed {
                if cur_char == Char::CarriageReturn
                    && get_escaped_character(str_offset, 1) == Char::LineFeed
                {
                    if is_raw {
                        local_value.push(cur_char);
                    }
                    str_offset += 1;
                    cur_char = get_escaped_character(str_offset, 0);
                }
                if is_raw {
                    let mut raw = vec![Char::Backslash];
                    raw.extend_from_slice(&local_value);
                    raw.push(cur_char);
                    local_value = raw;
                }
                str_offset += 1;
            } else {
                if is_raw {
                    local_value = vec![Char::Backslash, cur_char];
                    str_offset += 1;
                } else {
                    match cur_char {
                        Char::Backslash | Char::SingleQuote | Char::DoubleQuote => {
                            local_value = vec![cur_char];
                            str_offset += 1;
                        }

                        Char::a => {
                            local_value = vec![0x07];
                            str_offset += 1;
                        }

                        Char::b => {
                            local_value = vec![0x08];
                            str_offset += 1;
                        }

                        Char::f => {
                            local_value = vec![0x0c];
                            str_offset += 1;
                        }

                        Char::n => {
                            local_value = vec![0x0a];
                            str_offset += 1;
                        }

                        Char::r => {
                            local_value = vec![0x0d];
                            str_offset += 1;
                        }

                        Char::t => {
                            local_value = vec![0x09];
                            str_offset += 1;
                        }

                        Char::v => {
                            local_value = vec![0x0b];
                            str_offset += 1;
                        }

                        Char::x => {
                            local_value = scan_hex_escape(&mut output, &mut str_offset, 2);
                        }

                        Char::N => {
                            let mut found_illegal_char = false;
                            let mut char_count = 1;

                            // This type of escape isn't allowed for bytes.
                            if is_bytes {
                                found_illegal_char = true;
                            }

                            if get_escaped_character(str_offset, char_count) != Char::OpenBrace {
                                found_illegal_char = true;
                            } else {
                                char_count += 1;
                                loop {
                                    let lookahead_char =
                                        get_escaped_character(str_offset, char_count);
                                    if lookahead_char == Char::CloseBrace {
                                        break;
                                    } else if !_is_alpha_numeric_char(lookahead_char)
                                        && lookahead_char != Char::Hyphen
                                        && !_is_whitespace_char(lookahead_char)
                                    {
                                        found_illegal_char = true;
                                        break;
                                    } else {
                                        char_count += 1;
                                    }
                                }
                            }

                            if found_illegal_char {
                                add_invalid_escape_offset(&mut output, str_offset);
                                local_value = vec![Char::Backslash, cur_char];
                                str_offset += 1;
                            } else {
                                // We don't have the Unicode name database handy, so
                                // assume that the name is valid and use a '-' as a
                                // replacement character.
                                local_value = vec![Char::Hyphen];
                                str_offset += 1 + char_count;
                            }
                        }

                        Char::u | Char::U => {
                            // This type of escape isn't allowed for bytes.
                            if is_bytes {
                                add_invalid_escape_offset(&mut output, str_offset);
                            }
                            local_value = scan_hex_escape(
                                &mut output,
                                &mut str_offset,
                                if cur_char == Char::u { 4 } else { 8 },
                            );
                        }

                        _ => {
                            if _is_octal_char_code(cur_char) {
                                let mut octal_code = (cur_char - Char::_0) as u32;
                                str_offset += 1;
                                cur_char = get_escaped_character(str_offset, 0);
                                if _is_octal_char_code(cur_char) {
                                    octal_code = octal_code * 8 + (cur_char - Char::_0) as u32;
                                    str_offset += 1;
                                    cur_char = get_escaped_character(str_offset, 0);

                                    if _is_octal_char_code(cur_char) {
                                        octal_code = octal_code * 8 + (cur_char - Char::_0) as u32;
                                        str_offset += 1;
                                    }
                                }

                                local_value = vec![from_char_code(octal_code)];
                            } else {
                                local_value = vec![Char::Backslash];
                                add_invalid_escape_offset(&mut output, str_offset);
                            }
                        }
                    }
                }
            }

            output.value_parts.extend_from_slice(&local_value);
        } else if cur_char == Char::LineFeed || cur_char == Char::CarriageReturn {
            // Skip over the escaped new line (either one or two characters).
            if cur_char == Char::CarriageReturn
                && get_escaped_character(str_offset, 1) == Char::LineFeed
            {
                if !elide_crlf {
                    output.value_parts.push(cur_char);
                }
                str_offset += 1;
                cur_char = get_escaped_character(str_offset, 0);
            }

            output.value_parts.push(cur_char);
            str_offset += 1;
        } else {
            // There's nothing to unescape, so output the escaped character directly.
            if is_bytes && cur_char >= 128 {
                output.non_ascii_in_bytes = true;
            }

            output.value_parts.push(cur_char);
            str_offset += 1;
        }
    }
}

// parser/stringTokenUtils.ts:341
fn _is_whitespace_char(char_code: u16) -> bool {
    char_code == Char::Space || char_code == Char::Tab
}

// parser/stringTokenUtils.ts:345
fn _is_alpha_numeric_char(char_code: u16) -> bool {
    if char_code >= Char::_0 && char_code <= Char::_9 {
        return true;
    }

    if char_code >= Char::a && char_code <= Char::z {
        return true;
    }

    if char_code >= Char::A && char_code <= Char::Z {
        return true;
    }

    false
}

// parser/stringTokenUtils.ts:361
fn _is_octal_char_code(char_code: u16) -> bool {
    char_code >= Char::_0 && char_code <= Char::_7
}

// parser/stringTokenUtils.ts:365
fn _is_hex_char_code(char_code: u16) -> bool {
    if char_code >= Char::_0 && char_code <= Char::_9 {
        return true;
    }

    if char_code >= Char::a && char_code <= Char::f {
        return true;
    }

    if char_code >= Char::A && char_code <= Char::F {
        return true;
    }

    false
}

// parser/stringTokenUtils.ts:381
fn _get_hex_digit_value(char_code: u16) -> u32 {
    if char_code >= Char::_0 && char_code <= Char::_9 {
        return (char_code - Char::_0) as u32;
    }

    if char_code >= Char::a && char_code <= Char::f {
        return (char_code - Char::a + 10) as u32;
    }

    if char_code >= Char::A && char_code <= Char::F {
        return (char_code - Char::A + 10) as u32;
    }

    0
}
