#![deny(clippy::arithmetic_side_effects)]

use crate::engine::RejectReason;
use crate::money::{Amount, Money};

// Invariant: total is always available plus held, so it is derived, never stored.
// Every method computes every new value first and assigns only after every
// check passed. Checks run locked, then funds, then overflow.
#[derive(Debug, Default)]
pub struct Account {
    available: Money,
    held: Money,
    locked: bool,
}

impl Account {
    pub fn available(&self) -> Money {
        self.available
    }

    pub fn held(&self) -> Money {
        self.held
    }

    // deposit refuses any amount whose new total would not fit, so the
    // saturating path never runs. The assert makes that executable in tests.
    pub fn total(&self) -> Money {
        debug_assert!(
            self.available.checked_add(self.held).is_some(),
            "available plus held overflowed"
        );
        self.available.saturating_add(self.held)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn deposit(&mut self, amount: Amount) -> Result<(), RejectReason> {
        if self.locked {
            return Err(RejectReason::AccountLocked);
        }
        let Some(available) = self.available.checked_add(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        if available.checked_add(self.held).is_none() {
            return Err(RejectReason::Overflow);
        }
        self.available = available;
        Ok(())
    }

    pub fn withdraw(&mut self, amount: Amount) -> Result<(), RejectReason> {
        if self.locked {
            return Err(RejectReason::AccountLocked);
        }
        if self.available < amount.money() {
            return Err(RejectReason::InsufficientFunds);
        }
        let Some(available) = self.available.checked_sub(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        self.available = available;
        Ok(())
    }

    // No locked check and no funds check: a dispute holds the full deposit even
    // when it was already withdrawn, so available may go negative.
    pub fn hold(&mut self, amount: Amount) -> Result<(), RejectReason> {
        let Some(available) = self.available.checked_sub(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        let Some(held) = self.held.checked_add(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        self.available = available;
        self.held = held;
        Ok(())
    }

    pub fn release(&mut self, amount: Amount) -> Result<(), RejectReason> {
        let Some(available) = self.available.checked_add(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        let Some(held) = self.held.checked_sub(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        self.available = available;
        self.held = held;
        Ok(())
    }

    pub fn charge_back(&mut self, amount: Amount) -> Result<(), RejectReason> {
        let Some(held) = self.held.checked_sub(amount.money()) else {
            return Err(RejectReason::Overflow);
        };
        self.held = held;
        self.locked = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(text: &str) -> Amount {
        Amount::parse(text).unwrap()
    }

    fn balances(account: &Account) -> (String, String, String) {
        (
            account.available().to_string(),
            account.held().to_string(),
            account.total().to_string(),
        )
    }

    fn locked_account() -> Account {
        let mut account = Account::default();
        account.deposit(amount("10")).unwrap();
        account.hold(amount("10")).unwrap();
        account.charge_back(amount("10")).unwrap();
        account
    }

    #[test]
    fn a_chargeback_after_a_withdrawal_leaves_the_bank_exposed_and_locks_the_account() {
        let mut account = Account::default();
        account.deposit(amount("100")).unwrap();
        account.withdraw(amount("80")).unwrap();
        account.hold(amount("100")).unwrap();
        account.charge_back(amount("100")).unwrap();

        assert_eq!(account.available().to_string(), "-80.0000");
        assert_eq!(account.held().to_string(), "0.0000");
        assert_eq!(account.total().to_string(), "-80.0000");
        assert!(account.is_locked());
    }

    #[test]
    fn a_deposit_then_a_withdrawal_leaves_the_difference_available() {
        let mut account = Account::default();
        account.deposit(amount("10")).unwrap();
        account.withdraw(amount("4")).unwrap();

        assert_eq!(
            balances(&account),
            ("6.0000".into(), "0.0000".into(), "6.0000".into())
        );
        assert!(!account.is_locked());
    }

    #[test]
    fn insufficient_funds_leaves_the_state_unchanged() {
        let mut account = Account::default();
        account.deposit(amount("10")).unwrap();

        let result = account.withdraw(amount("10.0001"));

        assert_eq!(result, Err(RejectReason::InsufficientFunds));
        assert_eq!(
            balances(&account),
            ("10.0000".into(), "0.0000".into(), "10.0000".into())
        );
    }

    #[test]
    fn a_locked_account_rejects_deposits_and_withdrawals() {
        let mut account = locked_account();

        assert_eq!(
            account.deposit(amount("1")),
            Err(RejectReason::AccountLocked)
        );
        assert_eq!(
            account.withdraw(amount("1")),
            Err(RejectReason::AccountLocked)
        );
        assert_eq!(
            balances(&account),
            ("0.0000".into(), "0.0000".into(), "0.0000".into())
        );
    }

    #[test]
    fn a_hold_drives_available_negative_and_leaves_total_unchanged() {
        let mut account = Account::default();
        account.deposit(amount("10")).unwrap();
        account.withdraw(amount("8")).unwrap();
        account.hold(amount("10")).unwrap();

        assert_eq!(
            balances(&account),
            ("-8.0000".into(), "10.0000".into(), "2.0000".into())
        );
    }

    #[test]
    fn a_release_restores_the_held_amount_to_available() {
        let mut account = Account::default();
        account.deposit(amount("10")).unwrap();
        account.hold(amount("10")).unwrap();
        account.release(amount("10")).unwrap();

        assert_eq!(
            balances(&account),
            ("10.0000".into(), "0.0000".into(), "10.0000".into())
        );
    }

    #[test]
    fn a_deposit_that_overflows_available_is_refused_and_changes_nothing() {
        let mut account = Account::default();
        account.deposit(amount("922337203685477.5807")).unwrap();

        let result = account.deposit(amount("0.0001"));

        assert_eq!(result, Err(RejectReason::Overflow));
        assert_eq!(account.available().to_string(), "922337203685477.5807");
    }

    #[test]
    fn a_deposit_that_overflows_total_is_refused_even_when_available_fits() {
        let mut account = Account::default();
        account.deposit(amount("500000000000000")).unwrap();
        account.hold(amount("500000000000000")).unwrap();

        let result = account.deposit(amount("500000000000000"));

        assert_eq!(result, Err(RejectReason::Overflow));
        assert_eq!(
            balances(&account),
            (
                "0.0000".into(),
                "500000000000000.0000".into(),
                "500000000000000.0000".into()
            )
        );
    }
}
