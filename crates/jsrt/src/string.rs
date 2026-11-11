//! JS string semantics: UTF-16 code units, lone surrogates, and the
//! `String.prototype` methods whose results differ from Rust's `str` methods.
//!
//! [`JsStr`] is implemented for `str` and `[u16]` (and so for [`JsString`]).
//! Every index and length is in UTF-16 code units, as in JavaScript.

mod case;
mod case_tables;

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::ops::Deref;
use std::rc::Rc;

use unicode_normalization::UnicodeNormalization;

/// Immutable UTF-16 string that may hold lone surrogates, like a JS string.
///
/// `Ord` is JS `<`: code unit order. Build one in a `Vec<u16>`, then convert.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(jsrt::Trace)]
pub struct JsString(Rc<[u16]>);

impl JsString {
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    /// Decodes to a `String`. A lone surrogate becomes U+FFFD, which keeps the
    /// UTF-16 length.
    pub fn to_string_lossy(&self) -> String {
        string_from_utf16_lossy(&self.0)
    }

    /// JS `isWellFormed`: true when the string holds no lone surrogate.
    pub fn is_well_formed(&self) -> bool {
        char::decode_utf16(self.0.iter().copied()).all(|c| c.is_ok())
    }

    /// JS `a + b`.
    pub fn concat(&self, other: &[u16]) -> JsString {
        let mut units = Vec::with_capacity(self.0.len() + other.len());
        units.extend_from_slice(&self.0);
        units.extend_from_slice(other);
        JsString::from(units)
    }
}

impl Deref for JsString {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        &self.0
    }
}

impl AsRef<[u16]> for JsString {
    fn as_ref(&self) -> &[u16] {
        &self.0
    }
}

impl From<&[u16]> for JsString {
    fn from(units: &[u16]) -> Self {
        JsString(Rc::from(units))
    }
}

impl From<Vec<u16>> for JsString {
    fn from(units: Vec<u16>) -> Self {
        JsString(Rc::from(units))
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        // ASCII text, such as most source files, widens byte by byte.
        if s.is_ascii() {
            return JsString(s.bytes().map(u16::from).collect());
        }
        // Text with a few other chars widens its ASCII runs byte by byte too.
        let mut units = Vec::with_capacity(s.len());
        for segment in Utf8Segments::new(s) {
            match segment {
                Utf8Segment::Ascii(run) => units.extend(run.iter().map(|&b| u16::from(b))),
                Utf8Segment::Other(c) => units.extend_from_slice(c.encode_utf16(&mut [0; 2])),
            }
        }
        JsString::from(units)
    }
}

/// `String::from_utf16_lossy`, which decodes one char at a time; ASCII text
/// narrows in bulk.
pub fn string_from_utf16_lossy(units: &[u16]) -> String {
    if units.iter().fold(0, |all, &unit| all | unit) < 0x80 {
        let bytes = units.iter().map(|&unit| unit.to_le_bytes()[0]).collect();
        if let Ok(text) = String::from_utf8(bytes) {
            return text;
        }
    }
    String::from_utf16_lossy(units)
}

/// A piece of a `str`: a run of ASCII bytes, or one other char.
pub enum Utf8Segment<'a> {
    Ascii(&'a [u8]),
    Other(char),
}

/// The ASCII runs and other chars of a `str`, in order. Scanning 16 bytes at a
/// time, it lets text that is nearly all ASCII take the ASCII path.
pub struct Utf8Segments<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Utf8Segments<'a> {
    pub fn new(s: &'a str) -> Self {
        Utf8Segments { s, i: 0 }
    }
}

impl<'a> Iterator for Utf8Segments<'a> {
    type Item = Utf8Segment<'a>;

    fn next(&mut self) -> Option<Utf8Segment<'a>> {
        let bytes = self.s.as_bytes();
        let start = self.i;
        let first = *bytes.get(start)?;
        if first >= 0x80 {
            let c = self.s[start..].chars().next()?;
            self.i += c.len_utf8();
            return Some(Utf8Segment::Other(c));
        }
        let mut i = start;
        while let Some(chunk) = bytes.get(i..i + 16).and_then(|c| <[u8; 16]>::try_from(c).ok())
            && u128::from_ne_bytes(chunk) & 0x8080_8080_8080_8080_8080_8080_8080_8080 == 0
        {
            i += 16;
        }
        while bytes.get(i).is_some_and(|&b| b < 0x80) {
            i += 1;
        }
        self.i = i;
        Some(Utf8Segment::Ascii(&bytes[start..i]))
    }
}

impl From<String> for JsString {
    fn from(s: String) -> Self {
        JsString::from(s.as_str())
    }
}

impl From<&String> for JsString {
    fn from(s: &String) -> Self {
        JsString::from(s.as_str())
    }
}

impl FromIterator<u16> for JsString {
    fn from_iter<I: IntoIterator<Item = u16>>(iter: I) -> Self {
        JsString(iter.into_iter().collect())
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

impl PartialEq<JsString> for str {
    fn eq(&self, other: &JsString) -> bool {
        *other == *self
    }
}

impl PartialEq<JsString> for &str {
    fn eq(&self, other: &JsString) -> bool {
        *other == **self
    }
}

/// Lossy: a lone surrogate prints as U+FFFD.
impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in char::decode_utf16(self.0.iter().copied()) {
            fmt::Write::write_char(f, c.unwrap_or(char::REPLACEMENT_CHARACTER))?;
        }
        Ok(())
    }
}

/// Prints `JSON.stringify` of the string, so lone surrogates stay visible.
impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.js_json_stringify())
    }
}

/// JS `String.fromCharCode(code)`: keeps the low 16 bits.
pub fn from_char_code(code: u32) -> u16 {
    let [_, _, hi, lo] = code.to_be_bytes();
    u16::from_be_bytes([hi, lo])
}

/// The forms of JS `String.prototype.normalize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NormalizationForm {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

/// JS `WhiteSpace` and `LineTerminator`: the set `trim` removes.
///
/// Differs from Rust's `char::is_whitespace`: it includes U+FEFF and excludes
/// U+0085.
pub fn is_js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x09..=0x0D
            | 0x20
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

/// JS `WhiteSpace` and `LineTerminator` for a `char`, which is also JS regex
/// `\s`. Every member is in the BMP.
pub fn is_js_whitespace_char(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_js_whitespace)
}

/// `&'static [u16]` holding the UTF-16 encoding of a string literal, built at
/// compile time: `utf16!("abc")`.
#[macro_export]
macro_rules! utf16 {
    ($s:expr) => {{
        const S: &str = $s;
        const N: usize = $crate::string::utf16_len_const(S);
        const UNITS: [u16; N] = $crate::string::utf16_encode_const::<N>(S);
        const SLICE: &[u16] = &UNITS;
        SLICE
    }};
}

#[doc(hidden)]
pub const fn utf16_len_const(s: &str) -> usize {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b & 0xC0 != 0x80 {
            n += if b >= 0xF0 { 2 } else { 1 };
        }
        i += 1;
    }
    n
}

#[doc(hidden)]
pub const fn utf16_encode_const<const N: usize>(s: &str) -> [u16; N] {
    // `From` is not callable in a const fn, so this widens bytes with `as`.
    let bytes = s.as_bytes();
    let mut out = [0u16; N];
    let mut i = 0;
    let mut o = 0;
    while i < bytes.len() {
        let b = bytes[i] as u32;
        let (cp, width) = if b < 0x80 {
            (b, 1)
        } else if b < 0xE0 {
            (((b & 0x1F) << 6) | (bytes[i + 1] as u32 & 0x3F), 2)
        } else if b < 0xF0 {
            (
                ((b & 0x0F) << 12)
                    | ((bytes[i + 1] as u32 & 0x3F) << 6)
                    | (bytes[i + 2] as u32 & 0x3F),
                3,
            )
        } else {
            (
                ((b & 0x07) << 18)
                    | ((bytes[i + 1] as u32 & 0x3F) << 12)
                    | ((bytes[i + 2] as u32 & 0x3F) << 6)
                    | (bytes[i + 3] as u32 & 0x3F),
                4,
            )
        };
        if cp >= 0x10000 {
            let v = cp - 0x10000;
            out[o] = (0xD800 | (v >> 10)) as u16;
            out[o + 1] = (0xDC00 | (v & 0x3FF)) as u16;
            o += 2;
        } else {
            out[o] = cp as u16;
            o += 1;
        }
        i += width;
    }
    out
}

/// JS string methods with JS results: UTF-16 indices, JS index clamping, the
/// JS whitespace set, V8's case mapping and ICU's normalization.
///
/// On `str`, a result that may split a surrogate pair is a `Cow<str>`; a split
/// half becomes U+FFFD, which keeps the UTF-16 length. On `[u16]` every result
/// is exact.
pub trait JsStr {
    /// A borrowed piece of the string: `Cow<str>` or `&[u16]`.
    type Piece<'a>
    where
        Self: 'a;
    /// A new string: `String` or `JsString`.
    type Owned;

    /// JS `length`.
    fn js_len(&self) -> i32;
    /// JS `charCodeAt`. `None` is JS `NaN`.
    fn js_char_code_at(&self, index: i32) -> Option<u16>;
    /// JS `codePointAt`. `None` is JS `undefined`.
    fn js_code_point_at(&self, index: i32) -> Option<u32>;
    /// JS `slice(start, end)`. Pass `None` for an omitted `end`.
    fn js_slice(&self, start: i32, end: impl Into<Option<i32>>) -> Self::Piece<'_>;
    /// JS `substring(start, end)`. Pass `None` for an omitted `end`.
    fn js_substring(&self, start: i32, end: impl Into<Option<i32>>) -> Self::Piece<'_>;
    /// JS `substr(start, length)`. Pass `None` for an omitted `length`.
    fn js_substr(&self, start: i32, length: impl Into<Option<i32>>) -> Self::Piece<'_>;
    /// JS `<`, `>`, `===` on strings: UTF-16 code unit order.
    fn js_cmp(&self, other: &Self) -> Ordering;
    /// JS `trim`.
    fn js_trim(&self) -> &Self;
    /// JS `trimStart`.
    fn js_trim_start(&self) -> &Self;
    /// JS `trimEnd`.
    fn js_trim_end(&self) -> &Self;
    /// JS `toLowerCase`, as V8 does it: full case mapping and final sigma.
    fn js_to_lower_case(&self) -> Self::Owned;
    /// JS `toUpperCase`, as V8 does it: full case mapping.
    fn js_to_upper_case(&self) -> Self::Owned;
    /// JS `split(separator, limit)` with a string separator. Pass `None` for
    /// an omitted `limit`.
    fn js_split(&self, separator: &Self, limit: impl Into<Option<u32>>) -> Vec<Self::Piece<'_>>;
    /// JS `indexOf(search, position)`. Returns -1 when absent.
    fn js_index_of(&self, search: &Self, position: i32) -> i32;
    /// JS `lastIndexOf(search, position)`. Pass `None` for an omitted
    /// `position`.
    fn js_last_index_of(&self, search: &Self, position: impl Into<Option<i32>>) -> i32;
    /// JS `includes(search, position)`.
    fn js_includes(&self, search: &Self, position: i32) -> bool {
        self.js_index_of(search, position) >= 0
    }
    /// JS `startsWith(search, position)`.
    fn js_starts_with(&self, search: &Self, position: i32) -> bool;
    /// JS `endsWith(search, endPosition)`. Pass `None` for an omitted
    /// `endPosition`.
    fn js_ends_with(&self, search: &Self, end_position: impl Into<Option<i32>>) -> bool;
    /// JS `normalize(form)`, with the Unicode version of Node 26's ICU.
    fn js_normalize(&self, form: NormalizationForm) -> Self::Owned;
    /// JS `JSON.stringify(string)`: the quoted JSON text. A lone surrogate
    /// becomes `\udxxx`.
    fn js_json_stringify(&self) -> String;
    /// JS `replace(pattern, replacement)` with a string pattern: replaces the
    /// first match and expands `$$`, `$&`, `` $` `` and `$'`.
    fn js_replace(&self, pattern: &Self, replacement: &Self) -> Self::Owned;
    /// JS `replaceAll(pattern, replacement)` with a string pattern.
    fn js_replace_all(&self, pattern: &Self, replacement: &Self) -> Self::Owned;
}

// Index arithmetic shared by both implementations. Lengths fit in i32: V8
// strings are shorter than 2^30 units.

fn to_i32(n: usize) -> i32 {
    i32::try_from(n).expect("JS strings are shorter than 2^31 code units")
}

fn to_usize(n: i32) -> usize {
    usize::try_from(n).expect("index was clamped to be non-negative")
}

/// `slice` and `substr` start: negative counts from the end.
fn relative_index(index: i32, len: i32) -> i32 {
    if index < 0 {
        (len + index).max(0)
    } else {
        index.min(len)
    }
}

pub(crate) fn slice_range(start: i32, end: Option<i32>, len: i32) -> (usize, usize) {
    let from = relative_index(start, len);
    let to = relative_index(end.unwrap_or(len), len);
    if from >= to {
        (0, 0)
    } else {
        (to_usize(from), to_usize(to))
    }
}

fn substring_range(start: i32, end: Option<i32>, len: i32) -> (usize, usize) {
    let a = start.clamp(0, len);
    let b = end.unwrap_or(len).clamp(0, len);
    (to_usize(a.min(b)), to_usize(a.max(b)))
}

fn substr_range(start: i32, length: Option<i32>, len: i32) -> (usize, usize) {
    let from = relative_index(start, len);
    let length = length.unwrap_or(len).clamp(0, len);
    let to = from.saturating_add(length).min(len);
    if from >= to {
        (0, 0)
    } else {
        (to_usize(from), to_usize(to))
    }
}

/// GetSubstitution with no captures: only `$$`, `$&`, `` $` `` and `$'` expand.
fn substitute<'a, T: Copy + PartialEq + From<u8>>(
    out: &mut Vec<T>,
    replacement: &'a [T],
    matched: &'a [T],
    before: &'a [T],
    after: &'a [T],
) {
    let dollar = T::from(b'$');
    let mut i = 0;
    while i < replacement.len() {
        let c = replacement[i];
        if c == dollar && i + 1 < replacement.len() {
            let next = replacement[i + 1];
            let expansion: Option<&[T]> = if next == dollar {
                Some(&replacement[i..i + 1])
            } else if next == T::from(b'&') {
                Some(matched)
            } else if next == T::from(b'`') {
                Some(before)
            } else if next == T::from(b'\'') {
                Some(after)
            } else {
                None
            };
            if let Some(expansion) = expansion {
                out.extend_from_slice(expansion);
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
}

fn json_escape_unit(out: &mut String, unit: u16) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        out.push(char::from(HEX[usize::from((unit >> shift) & 0xF)]));
    }
}

fn json_stringify_chars(out: &mut String, chars: impl Iterator<Item = Result<char, u16>>) {
    out.push('"');
    for c in chars {
        match c {
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\t') => out.push_str("\\t"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\r') => out.push_str("\\r"),
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok(c) if c < ' ' => json_escape_unit(out, u16::from(u8::try_from(c).unwrap_or(0))),
            Ok(c) => out.push(c),
            Err(lone) => json_escape_unit(out, lone),
        }
    }
    out.push('"');
}

// `[u16]`: exact JS semantics.

fn find_units(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= haystack.len()).then_some(from);
    }
    if haystack.len() < needle.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| haystack[i..].starts_with(needle))
}

fn utf16_code_points(units: &[u16]) -> impl Iterator<Item = Result<char, u16>> + '_ {
    char::decode_utf16(units.iter().copied()).map(|r| r.map_err(|e| e.unpaired_surrogate()))
}

impl JsStr for [u16] {
    type Piece<'a> = &'a [u16];
    type Owned = JsString;

    fn js_len(&self) -> i32 {
        to_i32(self.len())
    }

    fn js_char_code_at(&self, index: i32) -> Option<u16> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.get(i).copied())
    }

    fn js_code_point_at(&self, index: i32) -> Option<u32> {
        let i = usize::try_from(index).ok()?;
        let first = *self.get(i)?;
        if (0xD800..0xDC00).contains(&first)
            && let Some(&second) = self.get(i + 1)
            && (0xDC00..0xE000).contains(&second)
        {
            return Some(
                0x10000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00),
            );
        }
        Some(u32::from(first))
    }

    fn js_slice(&self, start: i32, end: impl Into<Option<i32>>) -> &[u16] {
        let (a, b) = slice_range(start, end.into(), self.js_len());
        &self[a..b]
    }

    fn js_substring(&self, start: i32, end: impl Into<Option<i32>>) -> &[u16] {
        let (a, b) = substring_range(start, end.into(), self.js_len());
        &self[a..b]
    }

    fn js_substr(&self, start: i32, length: impl Into<Option<i32>>) -> &[u16] {
        let (a, b) = substr_range(start, length.into(), self.js_len());
        &self[a..b]
    }

    fn js_cmp(&self, other: &[u16]) -> Ordering {
        self.cmp(other)
    }

    fn js_trim(&self) -> &[u16] {
        self.js_trim_start().js_trim_end()
    }

    fn js_trim_start(&self) -> &[u16] {
        let start = self
            .iter()
            .position(|&u| !is_js_whitespace(u))
            .unwrap_or(self.len());
        &self[start..]
    }

    fn js_trim_end(&self) -> &[u16] {
        let end = self
            .iter()
            .rposition(|&u| !is_js_whitespace(u))
            .map_or(0, |i| i + 1);
        &self[..end]
    }

    fn js_to_lower_case(&self) -> JsString {
        JsString::from(case::to_lower_units(self))
    }

    fn js_to_upper_case(&self) -> JsString {
        JsString::from(case::to_upper_units(self))
    }

    fn js_split(&self, separator: &[u16], limit: impl Into<Option<u32>>) -> Vec<&[u16]> {
        let limit = usize::try_from(limit.into().unwrap_or(u32::MAX)).unwrap_or(usize::MAX);
        if limit == 0 {
            return Vec::new();
        }
        if separator.is_empty() {
            return self.chunks(1).take(limit).collect();
        }
        if self.is_empty() {
            return vec![self];
        }
        let mut pieces = Vec::new();
        let mut i = 0;
        while let Some(j) = find_units(self, separator, i) {
            pieces.push(&self[i..j]);
            if pieces.len() == limit {
                return pieces;
            }
            i = j + separator.len();
        }
        pieces.push(&self[i..]);
        pieces
    }

    fn js_index_of(&self, search: &[u16], position: i32) -> i32 {
        let from = to_usize(position.clamp(0, self.js_len()));
        find_units(self, search, from).map_or(-1, to_i32)
    }

    fn js_last_index_of(&self, search: &[u16], position: impl Into<Option<i32>>) -> i32 {
        let Some(max_start) = self.len().checked_sub(search.len()) else {
            return -1;
        };
        let start = position
            .into()
            .map_or(max_start, |p| to_usize(p.max(0)).min(max_start));
        (0..=start)
            .rev()
            .find(|&i| self[i..].starts_with(search))
            .map_or(-1, to_i32)
    }

    fn js_starts_with(&self, search: &[u16], position: i32) -> bool {
        let start = to_usize(position.clamp(0, self.js_len()));
        self[start..].starts_with(search)
    }

    fn js_ends_with(&self, search: &[u16], end_position: impl Into<Option<i32>>) -> bool {
        let len = self.js_len();
        let end = to_usize(end_position.into().unwrap_or(len).clamp(0, len));
        self[..end].ends_with(search)
    }

    fn js_normalize(&self, form: NormalizationForm) -> JsString {
        // A lone surrogate is a starter that never composes, so each run of
        // valid UTF-16 between lone surrogates normalizes on its own.
        let mut out = Vec::with_capacity(self.len());
        let mut run = String::new();
        for c in utf16_code_points(self) {
            match c {
                Ok(c) => run.push(c),
                Err(lone) => {
                    out.extend(run.js_normalize(form).encode_utf16());
                    run.clear();
                    out.push(lone);
                }
            }
        }
        out.extend(run.js_normalize(form).encode_utf16());
        JsString::from(out)
    }

    fn js_json_stringify(&self) -> String {
        let mut out = String::with_capacity(self.len() + 2);
        json_stringify_chars(&mut out, utf16_code_points(self));
        out
    }

    fn js_replace(&self, pattern: &[u16], replacement: &[u16]) -> JsString {
        let Some(p) = find_units(self, pattern, 0) else {
            return JsString::from(self);
        };
        let end = p + pattern.len();
        let mut out = Vec::with_capacity(self.len() + replacement.len());
        out.extend_from_slice(&self[..p]);
        substitute(&mut out, replacement, pattern, &self[..p], &self[end..]);
        out.extend_from_slice(&self[end..]);
        JsString::from(out)
    }

    fn js_replace_all(&self, pattern: &[u16], replacement: &[u16]) -> JsString {
        let advance = pattern.len().max(1);
        let mut out = Vec::with_capacity(self.len());
        let mut end_of_last = 0;
        let mut found = find_units(self, pattern, 0);
        while let Some(p) = found {
            out.extend_from_slice(&self[end_of_last..p]);
            substitute(
                &mut out,
                replacement,
                pattern,
                &self[..p],
                &self[p + pattern.len()..],
            );
            end_of_last = p + pattern.len();
            found = find_units(self, pattern, p + advance);
        }
        out.extend_from_slice(&self[end_of_last..]);
        JsString::from(out)
    }
}

// `str`: UTF-16 indices mapped onto UTF-8 byte offsets.

/// UTF-16 length of a `str`.
fn units_in(s: &str) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    // One unit per char, and a second for each char past U+FFFF: those start
    // with a byte of 0xF0 or more.
    s.chars().count() + s.bytes().filter(|&b| b >= 0xF0).count()
}

/// Byte offset of the char holding UTF-16 unit `index`, and whether `index` is
/// the low half of a surrogate pair. `index == len` maps to `(s.len(), false)`.
fn locate(s: &str, index: usize) -> (usize, bool) {
    let mut unit = 0;
    for (byte, c) in s.char_indices() {
        if unit == index {
            return (byte, false);
        }
        let width = c.len_utf16();
        if unit + width > index {
            return (byte, true);
        }
        unit += width;
    }
    (s.len(), false)
}

/// The UTF-16 range `[start, end)` of `s`, with split surrogate halves as U+FFFD.
fn str_units_range(s: &str, start: usize, end: usize) -> Cow<'_, str> {
    if start >= end {
        return Cow::Borrowed("");
    }
    let (sb, start_split) = locate(s, start);
    let (eb, end_split) = locate(s, end);
    if !start_split && !end_split {
        return Cow::Borrowed(&s[sb..eb]);
    }
    let mut out = String::with_capacity(eb - sb + 6);
    let body_start = if start_split {
        out.push(char::REPLACEMENT_CHARACTER);
        sb + 4
    } else {
        sb
    };
    if body_start <= eb {
        out.push_str(&s[body_start..eb]);
    }
    if end_split {
        out.push(char::REPLACEMENT_CHARACTER);
    }
    Cow::Owned(out)
}

fn str_utf16_unit(c: char, low_half: bool) -> u16 {
    let mut buf = [0u16; 2];
    let encoded = c.encode_utf16(&mut buf);
    encoded[usize::from(low_half)]
}

fn str_find(s: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() {
        return (from <= units_in(s)).then_some(from);
    }
    let (b, split) = locate(s, from);
    let b = if split { b + 4 } else { b };
    s.get(b..)?.find(needle).map(|i| units_in(&s[..b + i]))
}

impl JsStr for str {
    type Piece<'a> = Cow<'a, str>;
    type Owned = String;

    fn js_len(&self) -> i32 {
        to_i32(units_in(self))
    }

    fn js_char_code_at(&self, index: i32) -> Option<u16> {
        let index = usize::try_from(index).ok()?;
        let (b, low_half) = locate(self, index);
        self[b..]
            .chars()
            .next()
            .map(|c| str_utf16_unit(c, low_half))
    }

    fn js_code_point_at(&self, index: i32) -> Option<u32> {
        let index = usize::try_from(index).ok()?;
        let (b, low_half) = locate(self, index);
        let c = self[b..].chars().next()?;
        Some(if low_half {
            u32::from(str_utf16_unit(c, true))
        } else {
            u32::from(c)
        })
    }

    fn js_slice(&self, start: i32, end: impl Into<Option<i32>>) -> Cow<'_, str> {
        let (a, b) = slice_range(start, end.into(), self.js_len());
        str_units_range(self, a, b)
    }

    fn js_substring(&self, start: i32, end: impl Into<Option<i32>>) -> Cow<'_, str> {
        let (a, b) = substring_range(start, end.into(), self.js_len());
        str_units_range(self, a, b)
    }

    fn js_substr(&self, start: i32, length: impl Into<Option<i32>>) -> Cow<'_, str> {
        let (a, b) = substr_range(start, length.into(), self.js_len());
        str_units_range(self, a, b)
    }

    fn js_cmp(&self, other: &str) -> Ordering {
        // Byte order is code point order. It differs from UTF-16 order only at
        // the first differing char, so compare that char's UTF-16 encoding.
        let common = self
            .bytes()
            .zip(other.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let mut boundary = common;
        while !self.is_char_boundary(boundary) {
            boundary -= 1;
        }
        let a = self[boundary..].chars().next();
        let b = other[boundary..].chars().next();
        match (a, b) {
            (Some(a), Some(b)) => {
                let mut ba = [0u16; 2];
                let mut bb = [0u16; 2];
                a.encode_utf16(&mut ba).cmp(&b.encode_utf16(&mut bb))
            }
            (a, b) => a.is_some().cmp(&b.is_some()),
        }
    }

    fn js_trim(&self) -> &str {
        self.js_trim_start().js_trim_end()
    }

    fn js_trim_start(&self) -> &str {
        self.trim_start_matches(is_js_whitespace_char)
    }

    fn js_trim_end(&self) -> &str {
        self.trim_end_matches(is_js_whitespace_char)
    }

    fn js_to_lower_case(&self) -> String {
        if self.is_ascii() {
            return self.to_ascii_lowercase();
        }
        String::from_utf16_lossy(&case::to_lower_units(
            &self.encode_utf16().collect::<Vec<_>>(),
        ))
    }

    fn js_to_upper_case(&self) -> String {
        if self.is_ascii() {
            return self.to_ascii_uppercase();
        }
        String::from_utf16_lossy(&case::to_upper_units(
            &self.encode_utf16().collect::<Vec<_>>(),
        ))
    }

    fn js_split(&self, separator: &str, limit: impl Into<Option<u32>>) -> Vec<Cow<'_, str>> {
        let limit = usize::try_from(limit.into().unwrap_or(u32::MAX)).unwrap_or(usize::MAX);
        if limit == 0 {
            return Vec::new();
        }
        if separator.is_empty() {
            let len = units_in(self);
            return (0..len.min(limit))
                .map(|i| str_units_range(self, i, i + 1))
                .collect();
        }
        // A valid separator only matches at char boundaries.
        let mut pieces = Vec::new();
        for piece in self.split(separator) {
            pieces.push(Cow::Borrowed(piece));
            if pieces.len() == limit {
                break;
            }
        }
        pieces
    }

    fn js_index_of(&self, search: &str, position: i32) -> i32 {
        let from = to_usize(position.clamp(0, self.js_len()));
        str_find(self, search, from).map_or(-1, to_i32)
    }

    fn js_last_index_of(&self, search: &str, position: impl Into<Option<i32>>) -> i32 {
        let Some(max_start) = units_in(self).checked_sub(units_in(search)) else {
            return -1;
        };
        let start = position
            .into()
            .map_or(max_start, |p| to_usize(p.max(0)).min(max_start));
        if search.is_empty() {
            return to_i32(start);
        }
        // A match must end at or before unit `start + search_len`, on a char
        // boundary.
        let (end_byte, _) = locate(self, start + units_in(search));
        self[..end_byte]
            .rfind(search)
            .map_or(-1, |b| to_i32(units_in(&self[..b])))
    }

    fn js_starts_with(&self, search: &str, position: i32) -> bool {
        let start = to_usize(position.clamp(0, self.js_len()));
        if search.is_empty() {
            return true;
        }
        let (b, split) = locate(self, start);
        !split && self[b..].starts_with(search)
    }

    fn js_ends_with(&self, search: &str, end_position: impl Into<Option<i32>>) -> bool {
        let len = self.js_len();
        let end = to_usize(end_position.into().unwrap_or(len).clamp(0, len));
        if search.is_empty() {
            return true;
        }
        let (b, split) = locate(self, end);
        !split && self[..b].ends_with(search)
    }

    fn js_normalize(&self, form: NormalizationForm) -> String {
        match form {
            NormalizationForm::Nfc => self.nfc().collect(),
            NormalizationForm::Nfd => self.nfd().collect(),
            NormalizationForm::Nfkc => self.nfkc().collect(),
            NormalizationForm::Nfkd => self.nfkd().collect(),
        }
    }

    fn js_json_stringify(&self) -> String {
        let mut out = String::with_capacity(self.len() + 2);
        json_stringify_chars(&mut out, self.chars().map(Ok));
        out
    }

    fn js_replace(&self, pattern: &str, replacement: &str) -> String {
        let Some(b) = self.find(pattern) else {
            return self.to_owned();
        };
        let end = b + pattern.len();
        let mut out = Vec::with_capacity(self.len() + replacement.len());
        out.extend_from_slice(&self.as_bytes()[..b]);
        substitute(
            &mut out,
            replacement.as_bytes(),
            pattern.as_bytes(),
            &self.as_bytes()[..b],
            &self.as_bytes()[end..],
        );
        out.extend_from_slice(&self.as_bytes()[end..]);
        String::from_utf8(out).expect("substitution joins whole chars")
    }

    fn js_replace_all(&self, pattern: &str, replacement: &str) -> String {
        if pattern.is_empty() {
            // An empty pattern matches between code units, splitting surrogate
            // pairs, so this case runs on UTF-16.
            let units: Vec<u16> = self.encode_utf16().collect();
            let pattern: Vec<u16> = Vec::new();
            let replacement: Vec<u16> = replacement.encode_utf16().collect();
            return units
                .js_replace_all(&pattern, &replacement)
                .to_string_lossy();
        }
        let bytes = self.as_bytes();
        let mut out = Vec::with_capacity(self.len());
        let mut end_of_last = 0;
        for (b, _) in self.match_indices(pattern) {
            out.extend_from_slice(&bytes[end_of_last..b]);
            let end = b + pattern.len();
            substitute(
                &mut out,
                replacement.as_bytes(),
                pattern.as_bytes(),
                &bytes[..b],
                &bytes[end..],
            );
            end_of_last = end;
        }
        out.extend_from_slice(&bytes[end_of_last..]);
        String::from_utf8(out).expect("substitution joins whole chars")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_and_length_match_encode_utf16() {
        let long = "0123456789abcdef".repeat(5);
        for text in [
            "",
            "ascii only",
            "\u{e9}",
            "a\u{e9}b",
            "\u{1F600}",
            &format!("{long}\u{2026}{long}x\u{1F600}\u{7F}\u{80}"),
            &format!("\u{10FFFF}{long}"),
        ] {
            let units: Vec<u16> = text.encode_utf16().collect();
            assert_eq!(&*JsString::from(text), &units[..], "{text:?}");
            assert_eq!(string_from_utf16_lossy(&units), text);
            assert_eq!(text.js_len() as usize, units.len(), "{text:?}");
            let mut joined = Vec::new();
            for segment in Utf8Segments::new(text) {
                match segment {
                    Utf8Segment::Ascii(run) => {
                        assert!(!run.is_empty() && run.is_ascii());
                        joined.extend_from_slice(run);
                    }
                    Utf8Segment::Other(c) => {
                        assert!(!c.is_ascii());
                        joined.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                }
            }
            assert_eq!(joined, text.as_bytes());
        }
    }

    #[test]
    fn utf16_macro() {
        assert_eq!(utf16!("a\u{e9}\u{1F600}"), &[0x61, 0xE9, 0xD83D, 0xDE00]);
        assert_eq!(utf16!(""), &[] as &[u16]);
    }

    #[test]
    fn js_string_eq_str() {
        let s = JsString::from("h\u{e9}llo");
        assert!(s == "h\u{e9}llo");
        assert_eq!(s.js_len(), 5);
        let lone = JsString::from(vec![0x61, 0xD800]);
        assert_eq!(lone.to_string_lossy(), "a\u{FFFD}");
        assert_eq!(format!("{lone:?}"), "\"a\\ud800\"");
        assert!(!lone.is_well_formed());
    }

    #[test]
    fn from_char_code_truncates() {
        assert_eq!(from_char_code(0x1_0041), 0x41);
    }
}
