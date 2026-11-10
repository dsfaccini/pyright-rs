//! JS `bigint`: exact integers with V8's size limit.
//!
//! V8 throws a RangeError when a result would need more than 2^30 bits. The
//! `checked_*` methods return `None` exactly where V8 throws, so a ported
//! `try`/`catch` around bigint math becomes a match on `None`.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{BitAnd, BitOr, BitXor, Neg, Not};

use num_bigint::BigInt;
use num_traits::{FromPrimitive, Num, One, Signed, ToPrimitive, Zero};

use crate::string::JsStr;

// bigint.h in V8 14.6, for 64-bit digits.
const MAX_LENGTH_BITS: u64 = 1 << 30;
const DIGIT_BITS: u64 = 64;
const MAX_LENGTH: u64 = MAX_LENGTH_BITS / DIGIT_BITS;

/// Number of 64-bit digits V8 stores for `x`.
fn length(x: &BigInt) -> u64 {
    x.bits().div_ceil(DIGIT_BITS)
}

/// A JS `bigint`. `Ord` is JS `<` on bigints; `PartialOrd<f64>` and
/// `PartialEq<f64>` are JS `<` and `==` between a bigint and a number.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JsBigInt(BigInt);

impl JsBigInt {
    pub fn as_bigint(&self) -> &BigInt {
        &self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.0.is_negative()
    }

    /// JS `BigInt(x)` for a number. `None` where JS throws: NaN, ±Infinity
    /// and fractions.
    pub fn from_f64(x: f64) -> Option<JsBigInt> {
        if !x.is_finite() || x.fract() != 0.0 {
            return None;
        }
        BigInt::from_f64(x).map(JsBigInt)
    }

    /// JS `Number(b)`: the nearest double, ties to even; Infinity past the
    /// largest double.
    pub fn to_f64(&self) -> f64 {
        self.0
            .to_f64()
            .expect("BigInt::to_f64 saturates to Infinity")
    }

    /// JS `b.toString()`: decimal, no `n` suffix.
    pub fn to_js_string(&self) -> String {
        self.0.to_string()
    }

    /// JS `b.toString(radix)`, lowercase. Panics unless `radix` is in
    /// `2..=36`, where JS throws.
    pub fn to_string_radix(&self, radix: u32) -> String {
        assert!(
            (2..=36).contains(&radix),
            "toString radix must be in 2..=36, got {radix}"
        );
        self.0.to_str_radix(radix)
    }

    /// JS `a + b`. `None` where V8 throws: the result could exceed 2^30 bits.
    pub fn checked_add(&self, other: &JsBigInt) -> Option<JsBigInt> {
        // bigint.cc BigInt::Add in V8 14.6.
        if self.is_zero() {
            return Some(other.clone());
        }
        if other.is_zero() {
            return Some(self.clone());
        }
        let same_sign = self.is_negative() == other.is_negative();
        let result_length = length(&self.0).max(length(&other.0)) + u64::from(same_sign);
        (result_length <= MAX_LENGTH).then(|| JsBigInt(&self.0 + &other.0))
    }

    /// JS `a - b`. `None` where V8 throws.
    pub fn checked_sub(&self, other: &JsBigInt) -> Option<JsBigInt> {
        // bigint.cc BigInt::Subtract in V8 14.6.
        if other.is_zero() {
            return Some(self.clone());
        }
        if self.is_zero() {
            return Some(-other);
        }
        let same_sign = self.is_negative() == other.is_negative();
        let result_length = length(&self.0).max(length(&other.0)) + u64::from(!same_sign);
        (result_length <= MAX_LENGTH).then(|| JsBigInt(&self.0 - &other.0))
    }

    /// JS `a * b`. `None` where V8 throws.
    pub fn checked_mul(&self, other: &JsBigInt) -> Option<JsBigInt> {
        // bigint.cc BigInt::Multiply in V8 14.6.
        if self.is_zero() || other.is_zero() {
            return Some(JsBigInt::default());
        }
        (length(&self.0) + length(&other.0) <= MAX_LENGTH).then(|| JsBigInt(&self.0 * &other.0))
    }

    /// JS `a / b`: truncates toward zero. `None` for division by zero.
    pub fn checked_div(&self, other: &JsBigInt) -> Option<JsBigInt> {
        (!other.is_zero()).then(|| JsBigInt(&self.0 / &other.0))
    }

    /// JS `a % b`: takes the sign of `a`. `None` for division by zero.
    pub fn checked_rem(&self, other: &JsBigInt) -> Option<JsBigInt> {
        (!other.is_zero()).then(|| JsBigInt(&self.0 % &other.0))
    }

    /// JS `a ** b`. `None` where V8 throws: a negative exponent, or a result
    /// past V8's size limit.
    pub fn checked_pow(&self, exponent: &JsBigInt) -> Option<JsBigInt> {
        // bigint.cc BigInt::Exponentiate in V8 14.6.
        if exponent.is_negative() {
            return None;
        }
        if exponent.is_zero() {
            return Some(JsBigInt::from(1));
        }
        if self.is_zero() {
            return Some(self.clone());
        }
        if self.0.magnitude().is_one() {
            let even = !exponent.0.bit(0);
            return Some(if self.is_negative() && even {
                -self
            } else {
                self.clone()
            });
        }
        if length(&exponent.0) > 1 {
            return None;
        }
        let n = exponent.0.magnitude().to_u64().expect("one digit");
        if n == 1 {
            return Some(self.clone());
        }
        if n >= MAX_LENGTH_BITS {
            return None;
        }
        if *self.0.magnitude() == 2u32.into() {
            if 1 + n / DIGIT_BITS > MAX_LENGTH {
                return None;
            }
            let power = BigInt::one() << n;
            return Some(JsBigInt(if self.is_negative() && n & 1 == 1 {
                -power
            } else {
                power
            }));
        }
        let mut result: Option<JsBigInt> = (n & 1 == 1).then(|| self.clone());
        let mut running_square = self.clone();
        let mut n = n >> 1;
        while n != 0 {
            running_square = running_square.checked_mul(&running_square)?;
            if n & 1 == 1 {
                result = Some(match result {
                    None => running_square.clone(),
                    Some(r) => r.checked_mul(&running_square)?,
                });
            }
            n >>= 1;
        }
        result
    }

    /// JS `a << b`. `None` where V8 throws. A negative `b` shifts right.
    pub fn checked_shl(&self, shift: &JsBigInt) -> Option<JsBigInt> {
        // builtins-bigint.tq BigIntShiftLeftImpl in V8 14.6.
        if shift.is_negative() {
            Some(self.shift_right_by_abs(shift))
        } else {
            self.shift_left_by_abs(shift)
        }
    }

    /// JS `a >> b`: rounds toward -Infinity. `None` where V8 throws. A
    /// negative `b` shifts left.
    pub fn checked_shr(&self, shift: &JsBigInt) -> Option<JsBigInt> {
        // builtins-bigint.tq BigIntShiftRightImpl in V8 14.6.
        if shift.is_negative() {
            self.shift_left_by_abs(shift)
        } else {
            Some(self.shift_right_by_abs(shift))
        }
    }

    fn shift_left_by_abs(&self, shift: &JsBigInt) -> Option<JsBigInt> {
        if self.is_zero() || shift.is_zero() {
            return Some(self.clone());
        }
        if length(&shift.0) > 1 {
            return None;
        }
        let s = shift.0.magnitude().to_u64().expect("one digit");
        if s > MAX_LENGTH_BITS {
            return None;
        }
        let result_length = (self.0.bits() + s).div_ceil(DIGIT_BITS);
        (result_length <= MAX_LENGTH).then(|| JsBigInt(&self.0 << s))
    }

    fn shift_right_by_abs(&self, shift: &JsBigInt) -> JsBigInt {
        if self.is_zero() || shift.is_zero() {
            return self.clone();
        }
        let s = shift
            .0
            .magnitude()
            .to_u64()
            .filter(|&s| s <= MAX_LENGTH_BITS);
        match s {
            Some(s) => JsBigInt(&self.0 >> s),
            None if self.is_negative() => JsBigInt::from(-1),
            None => JsBigInt::default(),
        }
    }

    fn cmp_f64(&self, x: f64) -> Option<Ordering> {
        if x.is_nan() {
            return None;
        }
        if x.is_infinite() {
            return Some(if x > 0.0 {
                Ordering::Less
            } else {
                Ordering::Greater
            });
        }
        let floor = x.floor();
        let floor_big = BigInt::from_f64(floor).expect("finite");
        Some(match self.0.cmp(&floor_big) {
            // Between floor and floor + 1 when x has a fraction.
            Ordering::Equal if floor != x => Ordering::Less,
            ord => ord,
        })
    }
}

/// JS `BigInt(s)` for a string. `None` where JS throws a SyntaxError.
///
/// Surrounding whitespace is ignored and the empty string is `0n`. A decimal
/// may carry a sign; `0x`, `0o` and `0b` prefixes may not.
pub fn parse_bigint(s: &str) -> Option<JsBigInt> {
    let t = s.js_trim();
    if t.is_empty() {
        return Some(JsBigInt::default());
    }
    let b = t.as_bytes();
    let radix = if b.len() >= 2 && b[0] == b'0' {
        match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 10,
        }
    } else {
        10
    };
    let (negative, digits) = if radix == 10 {
        match b[0] {
            b'-' => (true, &t[1..]),
            b'+' => (false, &t[1..]),
            _ => (false, t),
        }
    } else {
        (false, &t[2..])
    };
    if digits.is_empty() || !digits.bytes().all(|c| char::from(c).is_digit(radix)) {
        return None;
    }
    let value = BigInt::from_str_radix(digits, radix).ok()?;
    Some(JsBigInt(if negative { -value } else { value }))
}

impl From<i64> for JsBigInt {
    fn from(n: i64) -> Self {
        JsBigInt(BigInt::from(n))
    }
}

impl From<i32> for JsBigInt {
    fn from(n: i32) -> Self {
        JsBigInt(BigInt::from(n))
    }
}

impl From<u64> for JsBigInt {
    fn from(n: u64) -> Self {
        JsBigInt(BigInt::from(n))
    }
}

impl From<BigInt> for JsBigInt {
    fn from(n: BigInt) -> Self {
        JsBigInt(n)
    }
}

impl PartialEq<f64> for JsBigInt {
    fn eq(&self, other: &f64) -> bool {
        self.cmp_f64(*other) == Some(Ordering::Equal)
    }
}

impl PartialOrd<f64> for JsBigInt {
    fn partial_cmp(&self, other: &f64) -> Option<Ordering> {
        self.cmp_f64(*other)
    }
}

impl PartialEq<JsBigInt> for f64 {
    fn eq(&self, other: &JsBigInt) -> bool {
        other == self
    }
}

impl PartialOrd<JsBigInt> for f64 {
    fn partial_cmp(&self, other: &JsBigInt) -> Option<Ordering> {
        other.cmp_f64(*self).map(Ordering::reverse)
    }
}

/// JS `-b`.
impl Neg for &JsBigInt {
    type Output = JsBigInt;

    fn neg(self) -> JsBigInt {
        JsBigInt(-&self.0)
    }
}

impl Neg for JsBigInt {
    type Output = JsBigInt;

    fn neg(self) -> JsBigInt {
        JsBigInt(-self.0)
    }
}

/// JS `~b`: `-b - 1`.
impl Not for &JsBigInt {
    type Output = JsBigInt;

    fn not(self) -> JsBigInt {
        JsBigInt(!&self.0)
    }
}

/// JS `a & b`, two's complement.
impl BitAnd for &JsBigInt {
    type Output = JsBigInt;

    fn bitand(self, other: &JsBigInt) -> JsBigInt {
        JsBigInt(&self.0 & &other.0)
    }
}

/// JS `a | b`, two's complement.
impl BitOr for &JsBigInt {
    type Output = JsBigInt;

    fn bitor(self, other: &JsBigInt) -> JsBigInt {
        JsBigInt(&self.0 | &other.0)
    }
}

/// JS `a ^ b`, two's complement.
impl BitXor for &JsBigInt {
    type Output = JsBigInt;

    fn bitxor(self, other: &JsBigInt) -> JsBigInt {
        JsBigInt(&self.0 ^ &other.0)
    }
}

/// Prints like JS `String(b)`.
impl fmt::Display for JsBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// Prints like a JS bigint literal: `12n`.
impl fmt::Debug for JsBigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}n", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_limit() {
        let two = JsBigInt::from(2);
        let three = JsBigInt::from(3);
        assert!(two.checked_pow(&JsBigInt::from(1i64 << 30)).is_none());
        assert!(three.checked_pow(&JsBigInt::from(1i64 << 30)).is_none());
        assert!(
            JsBigInt::from(1)
                .checked_shl(&JsBigInt::from((1i64 << 30) + 1))
                .is_none()
        );
        assert_eq!(
            JsBigInt::from(-1).checked_pow(&JsBigInt::from(1i64 << 40)),
            Some(JsBigInt::from(1))
        );
        assert_eq!(
            JsBigInt::from(-5).checked_shr(&JsBigInt::from(1i64 << 40)),
            Some(JsBigInt::from(-1))
        );
    }
}
