//! Port of `packages/pyright-internal/src/parser/characters.ts`.
//!
//! TypeScript builds the fast table at module load and the full maps on the
//! first lookup past it. Every build writes the same values, so here each is
//! built lazily on its first use: `fast_table()`, then `tables()`.

use std::sync::OnceLock;

use jsrt::FxHashMap;

use super::unicode::{self, UnicodeRange, UnicodeRangeTable, UnicodeSurrogateRangeTable};
use crate::common::char_codes::Char;

// parser/characters.ts:16
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum CharCategory {
    // Character cannot appear in identifier
    NotIdentifierChar = 0,

    // Character can appear at beginning or within identifier
    StartIdentifierChar = 1,

    // Character can appear only within identifier, not at beginning
    IdentifierChar = 2,

    // Character is a surrogate, meaning that additional character
    // needs to be consulted.
    SurrogateChar = 3,
}

// Table of first 256 character codes (the most common cases).
// parser/characters.ts:32
const _IDENTIFIER_CHAR_FAST_TABLE_SIZE: u32 = 256;

/// `CharCategoryMap`: code to category. A missing code is `undefined`.
type CharCategoryMap = FxHashMap<u32, CharCategory>;

struct IdentifierTables {
    // parser/characters.ts:33
    fast_table: [CharCategory; _IDENTIFIER_CHAR_FAST_TABLE_SIZE as usize],

    // Map of remaining characters that can appear within identifier.
    // parser/characters.ts:37
    identifier_char_map: CharCategoryMap,

    // Secondary character map based on the primary (surrogate) character.
    // parser/characters.ts:40
    surrogate_char_map: FxHashMap<u32, CharCategoryMap>,
}

static FAST_TABLE: OnceLock<[CharCategory; _IDENTIFIER_CHAR_FAST_TABLE_SIZE as usize]> = OnceLock::new();
static TABLES: OnceLock<IdentifierTables> = OnceLock::new();

/// The table TypeScript builds at module load, `_buildIdentifierLookupTable(true)`.
fn fast_table() -> &'static [CharCategory; _IDENTIFIER_CHAR_FAST_TABLE_SIZE as usize] {
    FAST_TABLE.get_or_init(|| _build_identifier_lookup_table(true).fast_table)
}

/// All tables, which TypeScript builds on the first lookup past the fast table.
fn tables() -> &'static IdentifierTables {
    TABLES.get_or_init(|| _build_identifier_lookup_table(false))
}

// parser/characters.ts:45
pub fn is_identifier_start_char(char: u16, next_char: Option<u16>) -> bool {
    if (char as u32) < _IDENTIFIER_CHAR_FAST_TABLE_SIZE {
        return fast_table()[char as usize] == CharCategory::StartIdentifierChar;
    }

    let tables = tables();

    let char_category = match next_char {
        Some(next_char) => _look_up_surrogate(tables, char, next_char),
        None => tables.identifier_char_map.get(&(char as u32)).copied(),
    };

    char_category == Some(CharCategory::StartIdentifierChar)
}

// parser/characters.ts:66
pub fn is_identifier_char(char: u16, next_char: Option<u16>) -> bool {
    if (char as u32) < _IDENTIFIER_CHAR_FAST_TABLE_SIZE {
        let fast_table = fast_table();
        return fast_table[char as usize] == CharCategory::StartIdentifierChar
            || fast_table[char as usize] == CharCategory::IdentifierChar;
    }

    let tables = tables();

    let char_category = match next_char {
        Some(next_char) => _look_up_surrogate(tables, char, next_char),
        None => tables.identifier_char_map.get(&(char as u32)).copied(),
    };

    char_category == Some(CharCategory::StartIdentifierChar)
        || char_category == Some(CharCategory::IdentifierChar)
}

// parser/characters.ts:90
pub fn is_surrogate_char(char: u16) -> bool {
    if (char as u32) < _IDENTIFIER_CHAR_FAST_TABLE_SIZE {
        return false;
    }

    tables().identifier_char_map.get(&(char as u32)) == Some(&CharCategory::SurrogateChar)
}

// parser/characters.ts:104
pub fn is_white_space(ch: u16) -> bool {
    ch == Char::Space || ch == Char::Tab || ch == Char::FormFeed
}

// parser/characters.ts:108
pub fn is_line_break(ch: u16) -> bool {
    ch == Char::CarriageReturn || ch == Char::LineFeed
}

// parser/characters.ts:112
pub fn is_number(ch: u16) -> bool {
    (ch >= Char::_0 && ch <= Char::_9) || ch == Char::Underscore
}

// parser/characters.ts:116
pub fn is_decimal(ch: u16) -> bool {
    (ch >= Char::_0 && ch <= Char::_9) || ch == Char::Underscore
}

// parser/characters.ts:120
pub fn is_hex(ch: u16) -> bool {
    is_decimal(ch)
        || (ch >= Char::a && ch <= Char::f)
        || (ch >= Char::A && ch <= Char::F)
        || ch == Char::Underscore
}

// parser/characters.ts:124
pub fn is_octal(ch: u16) -> bool {
    (ch >= Char::_0 && ch <= Char::_7) || ch == Char::Underscore
}

// parser/characters.ts:128
pub fn is_binary(ch: u16) -> bool {
    ch == Char::_0 || ch == Char::_1 || ch == Char::Underscore
}

/// `None` is `undefined`: the low surrogate is not in the table.
// parser/characters.ts:132
fn _look_up_surrogate(
    tables: &IdentifierTables,
    char: u16,
    next_char: u16,
) -> Option<CharCategory> {
    if tables.identifier_char_map.get(&(char as u32)) != Some(&CharCategory::SurrogateChar) {
        return Some(CharCategory::NotIdentifierChar);
    }

    let Some(surrogate_table) = tables.surrogate_char_map.get(&(char as u32)) else {
        return Some(CharCategory::NotIdentifierChar);
    };

    surrogate_table.get(&(next_char as u32)).copied()
}

// Underscore is explicitly allowed to start an identifier.
// Characters with the Other_ID_Start property.
// parser/characters.ts:147
const _SPECIAL_START_IDENTIFIER_CHARS: UnicodeRangeTable = &[
    UnicodeRange::S(Char::Underscore as u32),
    UnicodeRange::S(0x1885),
    UnicodeRange::S(0x1886),
    UnicodeRange::S(0x2118),
    UnicodeRange::S(0x212e),
    UnicodeRange::S(0x309b),
    UnicodeRange::S(0x309c),
];

// parser/characters.ts:157
const _START_IDENTIFIER_CHAR_RANGES: [UnicodeRangeTable; 7] = [
    _SPECIAL_START_IDENTIFIER_CHARS,
    unicode::UNICODE_LU,
    unicode::UNICODE_LL,
    unicode::UNICODE_LT,
    unicode::UNICODE_LO,
    unicode::UNICODE_LM,
    unicode::UNICODE_NL,
];

// parser/characters.ts:167
const _START_CHAR_SURROGATE_RANGES: [UnicodeSurrogateRangeTable; 5] = [
    unicode::UNICODE_LU_SURROGATE,
    unicode::UNICODE_LL_SURROGATE,
    unicode::UNICODE_LO_SURROGATE,
    unicode::UNICODE_LM_SURROGATE,
    unicode::UNICODE_NL_SURROGATE,
];

// Characters with the Other_ID_Start property.
// parser/characters.ts:176
const _SPECIAL_IDENTIFIER_CHARS: UnicodeRangeTable = &[
    UnicodeRange::S(0x00b7),
    UnicodeRange::S(0x0387),
    UnicodeRange::S(0x1369),
    UnicodeRange::S(0x136a),
    UnicodeRange::S(0x136b),
    UnicodeRange::S(0x136c),
    UnicodeRange::S(0x136d),
    UnicodeRange::S(0x136e),
    UnicodeRange::S(0x136f),
    UnicodeRange::S(0x1370),
    UnicodeRange::S(0x1371),
    UnicodeRange::S(0x19da),
];

// parser/characters.ts:180
const _IDENTIFIER_CHAR_RANGES: [UnicodeRangeTable; 5] = [
    _SPECIAL_IDENTIFIER_CHARS,
    unicode::UNICODE_MN,
    unicode::UNICODE_MC,
    unicode::UNICODE_ND,
    unicode::UNICODE_PC,
];

// parser/characters.ts:188
const _IDENTIFIER_CHAR_SURROGATE_RANGES: [UnicodeSurrogateRangeTable; 3] = [
    unicode::UNICODE_MN_SURROGATE,
    unicode::UNICODE_MC_SURROGATE,
    unicode::UNICODE_ND_SURROGATE,
];

/// Where `_buildIdentifierLookupTableFromUnicodeRangeTable` writes: the fast
/// table plus the full map, or (for a surrogate table) one map for both.
enum TableTarget<'a> {
    FastAndFull(&'a mut [CharCategory; 256], &'a mut CharCategoryMap),
    Single(&'a mut CharCategoryMap),
}

// parser/characters.ts:194
fn _build_identifier_lookup_table_from_unicode_range_table(
    table: UnicodeRangeTable,
    category: CharCategory,
    fast_table_only: bool,
    target: &mut TableTarget<'_>,
) {
    for entry in table {
        let (range_start, range_end) = match *entry {
            UnicodeRange::R(start, end) => (start, end),
            UnicodeRange::S(code) => (code, code),
        };

        for i in range_start..=range_end {
            if i < _IDENTIFIER_CHAR_FAST_TABLE_SIZE {
                match target {
                    TableTarget::FastAndFull(fast_table, _) => fast_table[i as usize] = category,
                    TableTarget::Single(map) => {
                        map.insert(i, category);
                    }
                }
            } else {
                match target {
                    TableTarget::FastAndFull(_, full_table) => {
                        full_table.insert(i, category);
                    }
                    TableTarget::Single(map) => {
                        map.insert(i, category);
                    }
                }
            }
        }

        if fast_table_only && range_start >= _IDENTIFIER_CHAR_FAST_TABLE_SIZE {
            break;
        }
    }
}

// parser/characters.ts:227
#[allow(clippy::map_entry)]
fn _build_identifier_lookup_table_from_surrogate_range_table(
    tables: &mut IdentifierTables,
    surrogate_table: UnicodeSurrogateRangeTable,
    category: CharCategory,
) {
    for &(surrogate_char, ranges) in surrogate_table {
        if !tables.surrogate_char_map.contains_key(&surrogate_char) {
            tables
                .surrogate_char_map
                .insert(surrogate_char, CharCategoryMap::default());
            tables
                .identifier_char_map
                .insert(surrogate_char, CharCategory::SurrogateChar);
        }

        let map = tables.surrogate_char_map.get_mut(&surrogate_char).unwrap();
        _build_identifier_lookup_table_from_unicode_range_table(
            ranges,
            category,
            /* fast_table_only */ false,
            &mut TableTarget::Single(map),
        );
    }
}

// Build a lookup table for to speed up tokenization of identifiers.
// parser/characters.ts:248
fn _build_identifier_lookup_table(fast_table_only: bool) -> IdentifierTables {
    let mut tables = IdentifierTables {
        fast_table: [CharCategory::NotIdentifierChar; 256],
        identifier_char_map: CharCategoryMap::default(),
        surrogate_char_map: FxHashMap::default(),
    };

    for table in _IDENTIFIER_CHAR_RANGES {
        _build_identifier_lookup_table_from_unicode_range_table(
            table,
            CharCategory::IdentifierChar,
            fast_table_only,
            &mut TableTarget::FastAndFull(&mut tables.fast_table, &mut tables.identifier_char_map),
        );
    }

    for table in _START_IDENTIFIER_CHAR_RANGES {
        _build_identifier_lookup_table_from_unicode_range_table(
            table,
            CharCategory::StartIdentifierChar,
            fast_table_only,
            &mut TableTarget::FastAndFull(&mut tables.fast_table, &mut tables.identifier_char_map),
        );
    }

    // Populate the surrogate tables for characters that require two
    // character codes.
    if !fast_table_only {
        for surrogate_table in _IDENTIFIER_CHAR_SURROGATE_RANGES {
            _build_identifier_lookup_table_from_surrogate_range_table(
                &mut tables,
                surrogate_table,
                CharCategory::IdentifierChar,
            );
        }

        for surrogate_table in _START_CHAR_SURROGATE_RANGES {
            _build_identifier_lookup_table_from_surrogate_range_table(
                &mut tables,
                surrogate_table,
                CharCategory::StartIdentifierChar,
            );
        }
    }

    tables
}
