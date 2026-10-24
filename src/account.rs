#![deny(clippy::arithmetic_side_effects)]

use crate::engine::RejectReason;
use crate::money::{Amount, Money};

// Invariant: total is always available plus held, so it is derived, never stored.
#[derive(Debug, Default)]
pub struct Account {
    available: Money,
    held: Money,
    locked: bool,
}

impl Account {
    pub fn available(&self) -> Money {
        todo!()
    }

    pub fn held(&self) -> Money {
        todo!()
    }

    pub fn total(&self) -> Money {
        todo!()
    }

    pub fn is_locked(&self) -> bool {
        todo!()
    }

    pub fn deposit(&mut self, amount: Amount) -> Result<(), RejectReason> {
        todo!()
    }

    pub fn withdraw(&mut self, amount: Amount) -> Result<(), RejectReason> {
        todo!()
    }

    pub fn hold(&mut self, amount: Amount) -> Result<(), RejectReason> {
        todo!()
    }

    pub fn release(&mut self, amount: Amount) -> Result<(), RejectReason> {
        todo!()
    }

    pub fn charge_back(&mut self, amount: Amount) -> Result<(), RejectReason> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(text: &str) -> Amount {
        Amount::parse(text).unwrap()
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
}
