#![deny(clippy::arithmetic_side_effects)]

use std::fmt;

use thiserror::Error;

const DECIMALS: usize = 4;
const UNITS_PER_WHOLE: i64 = 10_000;

// A signed count of 1/10,000 units. Balances are allowed to go negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Money(i64);

// Built only by `Amount::parse`, which rejects any sign, so it is never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(Money);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AmountError {
    #[error("amount is empty")]
    Empty,
    #[error("amount is not a plain decimal")]
    NotPlainDecimal,
    #[error("amount has more than four decimal places")]
    TooManyDecimals,
    #[error("amount is too large")]
    TooLarge,
}

impl Money {
    pub const ZERO: Money = Money(0);

    // Grammar: digits, optionally one dot and one to four digits. No sign, no
    // exponent, no separators. `.5` and `5.` are accepted, `.` alone is not.
    pub fn parse(text: &str) -> Result<Money, AmountError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AmountError::Empty);
        }
        let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
        if whole.is_empty() && fraction.is_empty() {
            return Err(AmountError::NotPlainDecimal);
        }
        if !is_all_digits(whole) || !is_all_digits(fraction) {
            return Err(AmountError::NotPlainDecimal);
        }
        if fraction.len() > DECIMALS {
            return Err(AmountError::TooManyDecimals);
        }
        let whole_units = digits_to_i64(whole)
            .and_then(|units| units.checked_mul(UNITS_PER_WHOLE))
            .ok_or(AmountError::TooLarge)?;
        let fraction_units = digits_to_i64(fraction)
            .and_then(|units| units.checked_mul(fraction_multiplier(fraction.len())))
            .ok_or(AmountError::TooLarge)?;
        whole_units
            .checked_add(fraction_units)
            .map(Money)
            .ok_or(AmountError::TooLarge)
    }

    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }

    // Only for the derived total, whose checked sum the account already guards.
    pub fn saturating_add(self, other: Money) -> Money {
        Money(self.0.saturating_add(other.0))
    }
}

impl Amount {
    pub fn parse(text: &str) -> Result<Amount, AmountError> {
        Money::parse(text).map(Amount)
    }

    pub fn money(self) -> Money {
        self.0
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // unsigned_abs so that i64::MIN prints instead of overflowing.
        let magnitude = self.0.unsigned_abs();
        let sign = if self.0 < 0 { "-" } else { "" };
        write!(f, "{sign}{}.{:04}", magnitude / 10_000, magnitude % 10_000)
    }
}

fn is_all_digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

// Only called on digit strings, so the digit lookup never fails and the only
// None is an i64 overflow.
fn digits_to_i64(digits: &str) -> Option<i64> {
    digits.chars().try_fold(0_i64, |total, digit| {
        let digit = i64::from(digit.to_digit(10)?);
        total.checked_mul(10)?.checked_add(digit)
    })
}

// Right pads the fraction to four places. The caller has already rejected
// fractions longer than four digits.
fn fraction_multiplier(len: usize) -> i64 {
    match len {
        0 => 10_000,
        1 => 1_000,
        2 => 100,
        3 => 10,
        _ => 1,
    }
}
