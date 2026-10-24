use payments_engine::money::{Amount, AmountError, Money};

fn money(text: &str) -> Money {
    Money::parse(text).unwrap()
}

fn tick() -> Money {
    money("0.0001")
}

fn max() -> Money {
    money("922337203685477.5807")
}

// i64::MIN is one tick below the negation of i64::MAX.
fn min() -> Money {
    Money::ZERO
        .checked_sub(max())
        .unwrap()
        .checked_sub(tick())
        .unwrap()
}

// Accepted inputs, printed back in canonical form.

#[test]
fn one_decimal_place_is_padded_to_four() {
    assert_eq!(money("1.0").to_string(), "1.0000");
}

#[test]
fn surrounding_whitespace_is_trimmed() {
    assert_eq!(money(" 2.50 ").to_string(), "2.5000");
}

#[test]
fn four_decimal_places_pass_unchanged() {
    assert_eq!(money("1.5000").to_string(), "1.5000");
}

#[test]
fn one_tick_is_the_smallest_amount() {
    assert_eq!(money("0.0001").to_string(), "0.0001");
}

#[test]
fn a_whole_number_needs_no_dot() {
    assert_eq!(money("2").to_string(), "2.0000");
}

#[test]
fn a_leading_dot_means_zero_units() {
    assert_eq!(money(".5").to_string(), "0.5000");
}

#[test]
fn a_trailing_dot_means_no_fraction() {
    assert_eq!(money("5.").to_string(), "5.0000");
}

#[test]
fn leading_zeros_are_dropped_on_display() {
    assert_eq!(money("00012.3400").to_string(), "12.3400");
}

#[test]
fn fourteen_whole_digits_fit() {
    assert_eq!(
        money("99999999999999.9999").to_string(),
        "99999999999999.9999"
    );
}

#[test]
fn the_largest_representable_value_parses() {
    assert_eq!(
        money("922337203685477.5807").to_string(),
        "922337203685477.5807"
    );
}

#[test]
fn bare_zero_parses_as_money() {
    assert_eq!(money("0"), Money::ZERO);
}

#[test]
fn zero_with_four_places_parses_as_money() {
    assert_eq!(money("0.0000"), Money::ZERO);
}

#[test]
fn bare_zero_parses_as_amount() {
    assert_eq!(Amount::parse("0").unwrap().money(), Money::ZERO);
}

#[test]
fn zero_with_four_places_parses_as_amount() {
    assert_eq!(Amount::parse("0.0000").unwrap().money(), Money::ZERO);
}

// Rejected inputs, by reason.

#[test]
fn a_fifth_decimal_is_too_many_even_when_zero() {
    assert!(matches!(
        Money::parse("1.50000"),
        Err(AmountError::TooManyDecimals)
    ));
}

#[test]
fn a_fifth_significant_decimal_is_too_many() {
    assert!(matches!(
        Money::parse("0.00001"),
        Err(AmountError::TooManyDecimals)
    ));
}

#[test]
fn a_negative_number_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("-5"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn negative_zero_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("-0"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn a_plus_sign_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("+3"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn an_exponent_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("1e3"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn a_comma_is_not_a_decimal_point() {
    assert!(matches!(
        Money::parse("1,5"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn an_underscore_separator_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("1_000"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn letters_are_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("abc"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn two_dots_are_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("1.2.3"),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn a_lone_dot_is_not_a_plain_decimal() {
    assert!(matches!(
        Money::parse("."),
        Err(AmountError::NotPlainDecimal)
    ));
}

#[test]
fn an_empty_string_is_empty() {
    assert!(matches!(Money::parse(""), Err(AmountError::Empty)));
}

#[test]
fn whitespace_only_is_empty() {
    assert!(matches!(Money::parse("   "), Err(AmountError::Empty)));
}

#[test]
fn one_tick_over_the_cap_is_too_large() {
    assert!(matches!(
        Money::parse("922337203685477.5808"),
        Err(AmountError::TooLarge)
    ));
}

#[test]
fn a_twenty_digit_number_is_too_large() {
    assert!(matches!(
        Money::parse("10000000000000000000"),
        Err(AmountError::TooLarge)
    ));
}

// Display of values that cannot come from the parser.

#[test]
fn a_negative_balance_prints_with_a_leading_minus() {
    let balance = Money::ZERO.checked_sub(money("20")).unwrap();
    assert_eq!(balance.to_string(), "-20.0000");
}

#[test]
fn a_negative_tick_prints_with_a_leading_minus() {
    let balance = Money::ZERO.checked_sub(tick()).unwrap();
    assert_eq!(balance.to_string(), "-0.0001");
}

#[test]
fn zero_prints_with_four_decimals() {
    assert_eq!(Money::ZERO.to_string(), "0.0000");
}

#[test]
fn the_maximum_prints_in_full() {
    assert_eq!(max().to_string(), "922337203685477.5807");
}

#[test]
fn the_minimum_prints_in_full() {
    assert_eq!(min().to_string(), "-922337203685477.5808");
}

// Checked arithmetic.

#[test]
fn adding_a_tick_to_the_maximum_is_none() {
    assert_eq!(max().checked_add(tick()), None);
}
