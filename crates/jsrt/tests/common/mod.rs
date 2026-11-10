//! Loads the node-derived fixtures written by `fixtures/gen.mjs`.

#![allow(dead_code)]

use serde_json::Value;

pub fn fixture(name: &str) -> Value {
    let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn units(v: &Value) -> Vec<u16> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected code units, got {v}"))
        .iter()
        .map(|x| u16::try_from(x.as_u64().expect("code unit")).expect("code unit"))
        .collect()
}

pub fn int(v: &Value) -> i64 {
    v.as_i64()
        .unwrap_or_else(|| panic!("expected an integer, got {v}"))
}

pub fn i32_of(v: &Value) -> i32 {
    i32::try_from(int(v)).expect("i32")
}

pub fn opt_i32(v: &Value) -> Option<i32> {
    if v.is_null() { None } else { Some(i32_of(v)) }
}

pub fn arr(v: &Value) -> &Vec<Value> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected an array, got {v}"))
}

/// The string, when it holds no lone surrogate.
pub fn well_formed(units: &[u16]) -> Option<String> {
    String::from_utf16(units).ok()
}

pub fn lossy(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// Shows code units in hex for assertion messages.
pub fn hex(units: &[u16]) -> String {
    units
        .iter()
        .map(|u| format!("{u:04x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
