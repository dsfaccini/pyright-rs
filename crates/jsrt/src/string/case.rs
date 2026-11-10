//! `toLowerCase` and `toUpperCase` as V8 does them: ICU's root-locale full
//! case mapping. Rust's `char` mappings use Unicode 17, the version of Node
//! 26's ICU. Final sigma uses ICU's context rule with Cased and Case_Ignorable
//! tables generated from Node.

use super::case_tables::{CASE_IGNORABLE, CASED};

fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < cp {
                std::cmp::Ordering::Less
            } else if lo > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Code points of UTF-16, with each lone surrogate as its own code point.
fn code_points(units: &[u16]) -> Vec<u32> {
    char::decode_utf16(units.iter().copied())
        .map(|r| r.map_or_else(|e| u32::from(e.unpaired_surrogate()), u32::from))
        .collect()
}

fn push_code_point(out: &mut Vec<u16>, cp: u32) {
    match char::from_u32(cp) {
        Some(c) => {
            let mut buf = [0u16; 2];
            out.extend_from_slice(c.encode_utf16(&mut buf));
        }
        None => out.push(u16::try_from(cp).expect("a non-char code point is a surrogate")),
    }
}

/// ICU `isFollowedByCasedLetter`: skips case-ignorable code points, then
/// reports whether the next one is cased.
fn next_is_cased(mut cps: impl Iterator<Item = u32>) -> bool {
    cps.find(|&cp| !in_ranges(CASE_IGNORABLE, cp))
        .is_some_and(|cp| in_ranges(CASED, cp))
}

pub(super) fn to_lower_units(units: &[u16]) -> Vec<u16> {
    let cps = code_points(units);
    let mut out = Vec::with_capacity(units.len());
    for (i, &cp) in cps.iter().enumerate() {
        if cp == 0x3A3 {
            let is_final = next_is_cased(cps[..i].iter().rev().copied())
                && !next_is_cased(cps[i + 1..].iter().copied());
            out.push(if is_final { 0x3C2 } else { 0x3C3 });
            continue;
        }
        match char::from_u32(cp) {
            Some(c) => c
                .to_lowercase()
                .for_each(|l| push_code_point(&mut out, u32::from(l))),
            None => push_code_point(&mut out, cp),
        }
    }
    out
}

pub(super) fn to_upper_units(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    for cp in code_points(units) {
        match char::from_u32(cp) {
            Some(c) => c
                .to_uppercase()
                .for_each(|u| push_code_point(&mut out, u32::from(u))),
            None => push_code_point(&mut out, cp),
        }
    }
    out
}
