//! Temporary stand-ins for `jsrt` number items that have not landed on `main`
//! yet, with the names and shapes the jsrt agent announced. Delete this file
//! and import from `jsrt` once `jsrt::NumberValue`, `jsrt::JsBigInt` and
//! `jsrt::number` exist there.

/// Placeholder for `jsrt::JsBigInt`: decimal digits with an optional sign.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JsBigInt(pub String);

/// JS `number | bigint`.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberValue {
    Number(f64),
    BigInt(JsBigInt),
}

/// Placeholders for `jsrt::number`.
pub mod number {
    use super::JsBigInt;

    /// `parseInt(s, radix)` for a string of digits in `radix`; NaN when empty.
    pub fn parse_int(s: &str, radix: i32) -> f64 {
        if s.is_empty() {
            return f64::NAN;
        }
        let mut value = 0f64;
        for c in s.chars() {
            match c.to_digit(radix as u32) {
                Some(d) => value = value * radix as f64 + d as f64,
                None => break,
            }
        }
        value
    }

    /// `parseFloat(s)`: the longest decimal-literal prefix, NaN when none.
    pub fn parse_float(s: &str) -> f64 {
        let b = s.as_bytes();
        let mut i = 0;
        let digits = |i: &mut usize| {
            let start = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            *i - start
        };
        let mut n = digits(&mut i);
        if i < b.len() && b[i] == b'.' {
            i += 1;
            n += digits(&mut i);
        }
        if n == 0 {
            return f64::NAN;
        }
        let mantissa_end = i;
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                j += 1;
            }
            if digits(&mut j) > 0 {
                i = j;
            } else {
                i = mantissa_end;
            }
        }
        s[..i].parse::<f64>().unwrap_or(f64::NAN)
    }

    /// `BigInt(s)`: keeps the literal text until jsrt's big integers land.
    pub fn parse_bigint(s: &str) -> Option<JsBigInt> {
        Some(JsBigInt(s.to_string()))
    }
}

impl JsBigInt {
    /// Placeholder: the literal as `i128`, or the sign of an overflowing one.
    fn approx(&self) -> f64 {
        let s = self.0.as_str();
        let (radix, digits) = if let Some(d) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        {
            (16, d)
        } else if let Some(d) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
            (8, d)
        } else if let Some(d) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
            (2, d)
        } else {
            (10, s)
        };
        number::parse_int(digits, radix)
    }
}

impl PartialEq<f64> for JsBigInt {
    fn eq(&self, other: &f64) -> bool {
        self.approx() == *other
    }
}

impl PartialOrd<f64> for JsBigInt {
    fn partial_cmp(&self, other: &f64) -> Option<std::cmp::Ordering> {
        self.approx().partial_cmp(other)
    }
}
