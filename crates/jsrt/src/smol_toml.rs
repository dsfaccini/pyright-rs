//! npm `smol-toml` 1.7.0 `parse(text)` (TOML 1.1.0), returning the JS value
//! as a `serde_json::Value` with tables in insertion order. The `toml` crate
//! implements the same spec version.
//!
//! Value mapping: integers and floats become JS numbers (`nan` and `inf`,
//! which JSON cannot hold, become `null`); dates and times become an empty
//! object, since pyright only checks that such a value is not a string.

use serde_json::{Map, Number, Value};

pub fn parse(text: &str) -> Result<Value, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
    Ok(convert_table(table))
}

fn convert_table(table: toml::Table) -> Value {
    let mut map = Map::new();
    for (key, value) in table {
        map.insert(key, convert(value));
    }
    Value::Object(map)
}

fn convert(value: toml::Value) -> Value {
    match value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(Number::from(i)),
        toml::Value::Float(f) => Number::from_f64(f).map_or(Value::Null, Value::Number),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(_) => Value::Object(Map::new()),
        toml::Value::Array(items) => Value::Array(items.into_iter().map(convert).collect()),
        toml::Value::Table(table) => convert_table(table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_pyright_table() {
        let v = parse("[project]\nname = 'x'\n[tool.pyright]\nvenv = \".venv\"\nexclude = [\"a\", \"b\"]\n").unwrap();
        assert_eq!(v["tool"]["pyright"]["venv"], ".venv");
        assert_eq!(v["tool"]["pyright"]["exclude"][1], "b");
        assert!(parse("[tool\n").is_err());
    }
}
