//! npm `jsonc-parser` 3.3.1 `parse(text, errors, { allowTrailingComma: true })`
//! as pyright calls it: comments allowed, trailing commas allowed, any error
//! fails the parse. The value is a `serde_json::Value` with insertion-ordered
//! objects; a repeated key keeps its first position and its last value, as
//! assigning to a JS object does.
//!
//! Known gaps: a `\ud800` escape (lone surrogate) fails here but parses in
//! jsonc-parser.

use serde_json::Value;

/// Returns the parsed value, or `Err` when jsonc-parser would report errors.
pub fn parse(text: &str) -> Result<Value, String> {
    let stripped = strip_comments_and_trailing_commas(text)?;
    serde_json::from_str(&stripped).map_err(|e| e.to_string())
}

/// Replaces comments with spaces and drops commas that only precede `]` or `}`.
fn strip_comments_and_trailing_commas(text: &str) -> Result<String, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    // Index in `out` of the last comma not yet followed by a value.
    let mut pending_comma: Option<usize> = None;
    // The last character outside comments and whitespace.
    let mut last_significant: Option<char> = None;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                pending_comma = None;
                last_significant = Some('"');
                out.push(c);
                i += 1;
                while i < chars.len() {
                    let d = chars[i];
                    out.push(d);
                    i += 1;
                    if d == '\\' {
                        if i < chars.len() {
                            out.push(chars[i]);
                            i += 1;
                        }
                    } else if d == '"' {
                        break;
                    }
                }
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                    out.push(' ');
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                out.push(' ');
                out.push(' ');
                i += 2;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                        out.push(' ');
                        out.push(' ');
                        i += 2;
                        closed = true;
                        break;
                    }
                    out.push(if chars[i] == '\n' { '\n' } else { ' ' });
                    i += 1;
                }
                if !closed {
                    return Err("UnexpectedEndOfComment".to_string());
                }
            }
            ',' => {
                // A comma right after `[`, `{` or `,` stays, so the parse fails.
                pending_comma = match last_significant {
                    Some('[' | '{' | ',') | None => None,
                    _ => Some(out.len()),
                };
                last_significant = Some(',');
                out.push(c);
                i += 1;
            }
            ']' | '}' => {
                if let Some(index) = pending_comma.take() {
                    out[index] = ' ';
                }
                last_significant = Some(c);
                out.push(c);
                i += 1;
            }
            c if c.is_whitespace() => {
                out.push(c);
                i += 1;
            }
            _ => {
                pending_comma = None;
                last_significant = Some(c);
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_trailing_commas() {
        let v = parse("{\n // c\n \"a\": [1, 2,], /* x */ \"b\": \"//no\",\n}").unwrap();
        assert_eq!(v["a"], serde_json::json!([1, 2]));
        assert_eq!(v["b"], "//no");
    }

    #[test]
    fn duplicate_key_keeps_first_position_last_value() {
        let v = parse("{\"a\": 1, \"b\": 2, \"a\": 3}").unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(v["a"], 3);
    }

    #[test]
    fn errors() {
        assert!(parse("").is_err());
        assert!(parse("{\"a\": }").is_err());
        assert!(parse("{ /* open").is_err());
        assert!(parse("[,]").is_err());
    }
}

#[cfg(test)]
mod node_tests {
    /// `tests/data/jsonc.jsonl`: random JSONC texts with what jsonc-parser
    /// 3.3.1 returns for them (`null` when it reports errors).
    #[test]
    fn matches_jsonc_parser() {
        for line in include_str!("../tests/data/jsonc.jsonl").lines() {
            let case: serde_json::Value = serde_json::from_str(line).unwrap();
            let text = case[0].as_str().unwrap();
            match super::parse(text) {
                Ok(value) => assert!(js_equal(&value, &case[1]), "{text}: {value} vs {}", case[1]),
                Err(_) => assert!(case[1].is_null(), "{text}"),
            }
        }
    }

    /// Structural equality with numbers compared as JS numbers.
    fn js_equal(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        use serde_json::Value;
        match (a, b) {
            (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
            (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| js_equal(p, q)),
            (Value::Object(x), Value::Object(y)) => {
                x.len() == y.len() && x.iter().zip(y).all(|((k1, v1), (k2, v2))| k1 == k2 && js_equal(v1, v2))
            }
            _ => a == b,
        }
    }
}
