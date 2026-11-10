//! JS number semantics: `String(n)`, `toString(radix)`, `parseInt`,
//! `parseFloat`, `Number(string)`, `Math.round`, and `bigint` with V8's
//! limits.

mod bigint;

use std::hash::{Hash, Hasher};
use std::iter::repeat_n;

use num_bigint::BigUint;
use num_traits::{Num, ToPrimitive};

use crate::string::JsStr;

pub use bigint::{JsBigInt, parse_bigint};

/// JS `number | bigint`, the value of a Python number literal.
///
/// `==` and `Hash` follow JS `SameValueZero`, the key equality of `Map` and
/// `Set`: a number never equals a bigint, `0 == -0`, and `NaN == NaN`.
#[derive(Clone, Debug)]
pub enum NumberValue {
    Number(f64),
    BigInt(JsBigInt),
}

impl NumberValue {
    /// JS `String(value)`. A bigint prints without the `n` suffix.
    pub fn to_js_string(&self) -> String {
        match self {
            NumberValue::Number(x) => number_to_string(*x),
            NumberValue::BigInt(b) => b.to_js_string(),
        }
    }
}

impl PartialEq for NumberValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NumberValue::Number(a), NumberValue::Number(b)) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            (NumberValue::BigInt(a), NumberValue::BigInt(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for NumberValue {}

impl Hash for NumberValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            NumberValue::Number(x) => {
                state.write_u8(0);
                // One hash for 0 and -0, and one for every NaN.
                let canonical = if *x == 0.0 {
                    0.0
                } else if x.is_nan() {
                    f64::NAN
                } else {
                    *x
                };
                state.write_u64(canonical.to_bits());
            }
            NumberValue::BigInt(b) => {
                state.write_u8(1);
                b.hash(state);
            }
        }
    }
}

/// JS `String(x)` for a number: the shortest digits that round-trip, plain
/// notation from 1e-6 up to 1e21, `0` for `-0`.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        return "0".to_owned();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let (digits, n) = shortest_digits(x.abs());
    let k = i32::try_from(digits.len()).expect("at most 17 digits");
    let mut out = String::with_capacity(32);
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(repeat_n('0', usize::try_from(n - k).expect("n >= k")));
    } else if 0 < n && n <= 21 {
        let (int_part, frac_part) = digits.split_at(usize::try_from(n).expect("n > 0"));
        out.push_str(int_part);
        out.push('.');
        out.push_str(frac_part);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(repeat_n('0', usize::try_from(-n).expect("n <= 0")));
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        out.push('e');
        out.push(if n >= 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// The shortest digits `d1..dk` that round-trip to `x > 0`, and `n` with
/// `x = 0.d1..dk * 10^n`.
///
/// Ryu, like V8's Dragonbox, breaks an exact tie between two shortest
/// candidates toward the even one, as the spec asks. Rust's `{:e}` rounds such
/// a tie up: 2^-25 prints as `...313e-8` where JS prints `...312e-8`.
fn shortest_digits(x: f64) -> (String, i32) {
    let mut buffer = ryu::Buffer::new();
    let formatted = buffer.format_finite(x);
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((formatted, "0"));
    let exponent = exponent
        .parse::<i32>()
        .expect("ryu prints an integer exponent");
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{int_part}{frac_part}");
    let leading_zeros = all.len() - all.trim_start_matches('0').len();
    let point = i32::try_from(int_part.len()).expect("short") + exponent
        - i32::try_from(leading_zeros).expect("short");
    (all.trim_matches('0').to_owned(), point)
}

const DIGIT_CHARS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// JS `x.toString(radix)`. Panics unless `radix` is in `2..=36`, where JS
/// throws a RangeError.
pub fn number_to_string_radix(x: f64, radix: u32) -> String {
    assert!(
        (2..=36).contains(&radix),
        "toString radix must be in 2..=36, got {radix}"
    );
    if radix == 10 || !x.is_finite() || x == 0.0 {
        return number_to_string(x);
    }
    double_to_radix_string(x, radix)
}

// conversions.cc DoubleToRadixStringView in V8 14.6.
fn double_to_radix_string(value: f64, radix: u32) -> String {
    let radix_f = f64::from(radix);
    let negative = value < 0.0;
    let value = value.abs();
    let mut integer = value.floor();
    let mut fraction = value - integer;
    // Fractional digits only go as far as the input's precision.
    let mut delta = (0.5 * (value.next_up() - value)).max(0.0f64.next_up());
    let mut fraction_digits: Vec<u32> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= radix_f;
            delta *= radix_f;
            let digit = fraction
                .to_u32()
                .expect("a fraction digit is below the radix");
            fraction_digits.push(digit);
            fraction -= f64::from(digit);
            if (fraction > 0.5 || (fraction == 0.5 && digit & 1 == 1)) && fraction + delta > 1.0 {
                // Round up, carrying through the digits already written.
                loop {
                    match fraction_digits.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(d) if d + 1 < radix => {
                            fraction_digits.push(d + 1);
                            break;
                        }
                        Some(_) => {}
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }
    // Integer digits past the input's precision print as zeros.
    const TWO_POW_53: f64 = 9_007_199_254_740_992.0;
    let mut integer_digits: Vec<u8> = Vec::new();
    while integer / radix_f >= TWO_POW_53 {
        integer /= radix_f;
        integer_digits.push(b'0');
    }
    loop {
        let remainder = integer % radix_f;
        let digit = remainder
            .to_usize()
            .expect("a remainder is below the radix");
        integer_digits.push(DIGIT_CHARS[digit]);
        integer = (integer - remainder) / radix_f;
        if integer <= 0.0 {
            break;
        }
    }
    let mut out = String::with_capacity(integer_digits.len() + fraction_digits.len() + 2);
    if negative {
        out.push('-');
    }
    out.extend(integer_digits.iter().rev().map(|&b| char::from(b)));
    if !fraction_digits.is_empty() {
        out.push('.');
        out.extend(
            fraction_digits
                .iter()
                .map(|&d| char::from(DIGIT_CHARS[usize::try_from(d).expect("digit")])),
        );
    }
    out
}

fn digit_value(c: u8) -> Option<u32> {
    char::from(c).to_digit(36)
}

fn is_radix_digit(c: u8, radix: u32) -> bool {
    digit_value(c).is_some_and(|d| d < radix)
}

/// The exact value of radix-`radix` digits, rounded once to the nearest
/// double, ties to even. V8 does this for radixes 2, 4, 8, 16 and 32.
fn exact_digits_to_f64(digits: &[u8], radix: u32) -> f64 {
    let text = std::str::from_utf8(digits).expect("ASCII digits");
    BigUint::from_str_radix(text, radix)
        .expect("valid digits")
        .to_f64()
        .expect("BigUint::to_f64 saturates to Infinity")
}

// conversions.cc NumberParseIntHelper::HandleGenericCase in V8 14.6: radixes
// other than powers of two and 10 accumulate rounding error past 2^53.
fn generic_radix_to_f64(digits: &[u8], radix: u32) -> f64 {
    const MAX_MULTIPLIER: u32 = u32::MAX / 36;
    let mut result = 0.0;
    let mut i = 0;
    while i < digits.len() {
        let mut part: u32 = 0;
        let mut multiplier: u32 = 1;
        while i < digits.len() {
            let m = multiplier * radix;
            if m > MAX_MULTIPLIER {
                break;
            }
            part = part * radix + digit_value(digits[i]).expect("radix digit");
            multiplier = m;
            i += 1;
        }
        result = result * f64::from(multiplier) + f64::from(part);
    }
    result
}

fn signed_zero(negative: bool) -> f64 {
    if negative { -0.0 } else { 0.0 }
}

/// JS `parseInt(s, radix)`. Pass 0 for an omitted radix; `radix` is the
/// result of JS `ToInt32`. Returns NaN where JS does.
pub fn parse_int(s: &str, radix: i32) -> f64 {
    // runtime-numbers.cc Runtime_StringParseInt, then conversions.cc
    // StringToIntHelper and NumberParseIntHelper, V8 14.6.
    if radix != 0 && !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let mut radix = u32::try_from(radix).expect("radix is in 0..=36");
    let b = s.js_trim_start().as_bytes();
    if b.is_empty() {
        return f64::NAN;
    }
    let mut i = 0;
    let negative = b[0] == b'-';
    if b[0] == b'+' || b[0] == b'-' {
        i += 1;
        if i == b.len() {
            return f64::NAN;
        }
    }
    let mut leading_zero = false;
    if radix == 0 || radix == 16 {
        let detect = radix == 0;
        if detect {
            radix = 10;
        }
        if b[i] == b'0' {
            i += 1;
            if i == b.len() {
                return signed_zero(negative);
            }
            if b[i] == b'x' || b[i] == b'X' {
                radix = 16;
                i += 1;
                if i == b.len() {
                    return f64::NAN;
                }
            } else {
                leading_zero = true;
            }
        }
    }
    while b[i] == b'0' {
        leading_zero = true;
        i += 1;
        if i == b.len() {
            return signed_zero(negative);
        }
    }
    if !is_radix_digit(b[i], radix) {
        return if leading_zero {
            signed_zero(negative)
        } else {
            f64::NAN
        };
    }
    let end = i + b[i..]
        .iter()
        .take_while(|&&c| is_radix_digit(c, radix))
        .count();
    let digits = &b[i..end];
    let value = match radix {
        10 => {
            // V8 keeps the first 310 digits. More digits are at least 1e309,
            // which is Infinity either way.
            let kept = &digits[..digits.len().min(310)];
            std::str::from_utf8(kept)
                .expect("ASCII digits")
                .parse::<f64>()
                .expect("decimal digits")
        }
        2 | 4 | 8 | 16 | 32 => exact_digits_to_f64(digits, radix),
        _ => generic_radix_to_f64(digits, radix),
    };
    if negative { -value } else { value }
}

/// Length of the longest prefix of `b` that is a JS `StrDecimalLiteral`.
fn decimal_literal_prefix(b: &[u8]) -> Option<usize> {
    let count_digits = |from: usize| {
        b.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|c| c.is_ascii_digit()).count()
        })
    };
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    if b[i..].starts_with(b"Infinity") {
        return Some(i + 8);
    }
    let int_digits = count_digits(i);
    i += int_digits;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        frac_digits = count_digits(i + 1);
        if int_digits + frac_digits > 0 {
            i += 1 + frac_digits;
        }
    }
    if int_digits + frac_digits == 0 {
        return None;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exp_digits = count_digits(j);
        if exp_digits > 0 {
            i = j + exp_digits;
        }
    }
    Some(i)
}

/// The value of a whole `StrDecimalLiteral`, correctly rounded as V8's
/// `Strtod` does.
fn decimal_literal_value(literal: &str) -> f64 {
    let unsigned = literal.trim_start_matches(['+', '-']);
    if unsigned == "Infinity" {
        return if literal.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    literal
        .parse::<f64>()
        .expect("a StrDecimalLiteral parses as f64")
}

/// JS `parseFloat(s)`: the longest decimal prefix after leading whitespace.
pub fn parse_float(s: &str) -> f64 {
    let t = s.js_trim_start();
    match decimal_literal_prefix(t.as_bytes()) {
        Some(len) => decimal_literal_value(&t[..len]),
        None => f64::NAN,
    }
}

/// JS `Number(s)` for a string: the whole trimmed string must be a number;
/// the empty string is 0; `0x`, `0o` and `0b` prefixes are allowed.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.js_trim();
    let b = t.as_bytes();
    if b.is_empty() {
        return 0.0;
    }
    if b.len() >= 2 && b[0] == b'0' {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            let digits = &b[2..];
            if digits.is_empty() || !digits.iter().all(|&c| is_radix_digit(c, radix)) {
                return f64::NAN;
            }
            return exact_digits_to_f64(digits, radix);
        }
    }
    match decimal_literal_prefix(b) {
        Some(len) if len == b.len() => decimal_literal_value(t),
        _ => f64::NAN,
    }
}

/// JS `Math.round(x)`: halves round toward +Infinity; keeps `-0`.
pub fn math_round(x: f64) -> f64 {
    // code-stub-assembler.cc Float64Round in V8 14.6.
    let up = x.ceil();
    if up - 0.5 <= x { up } else { up - 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_value_zero() {
        assert_eq!(NumberValue::Number(0.0), NumberValue::Number(-0.0));
        assert_eq!(NumberValue::Number(f64::NAN), NumberValue::Number(f64::NAN));
        assert_ne!(
            NumberValue::Number(1.0),
            NumberValue::BigInt(JsBigInt::from(1))
        );
    }
}
