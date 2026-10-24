use std::collections::{BTreeMap, HashMap};

use thiserror::Error;

use crate::account::Account;
use crate::money::Amount;
use crate::transaction::{Kind, Transaction};
use crate::{ClientId, TxId};

#[derive(Debug, Default)]
pub struct Engine {
    // BTreeMap: output is in client order and there are at most 65,536 keys.
    accounts: BTreeMap<ClientId, Account>,
    // HashMap with the std hasher: up to four billion keys, order irrelevant,
    // and tx ids come from outside.
    deposits: HashMap<TxId, DepositRecord>,
}

#[derive(Debug)]
struct DepositRecord {
    client: ClientId,
    amount: Amount,
    state: DisputeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisputeState {
    Undisputed,
    Disputed,
    ChargedBack,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn apply(&mut self, tx: Transaction) -> Result<(), RejectReason> {
        match tx.kind {
            Kind::Deposit(amount) => {
                // The duplicate check runs before the accounts map is touched,
                // so a duplicate from a new client creates nothing.
                if self.deposits.contains_key(&tx.tx) {
                    return Err(RejectReason::DuplicateTx(tx.tx));
                }
                let account = self.accounts.entry(tx.client).or_default();
                account.deposit(amount)?;
                // Stored only after the account accepted, so a deposit refused
                // on a locked account is never disputable.
                self.deposits.insert(
                    tx.tx,
                    DepositRecord {
                        client: tx.client,
                        amount,
                        state: DisputeState::Undisputed,
                    },
                );
                Ok(())
            }
            Kind::Withdrawal(amount) => {
                if self.deposits.contains_key(&tx.tx) {
                    return Err(RejectReason::DuplicateTx(tx.tx));
                }
                // A withdrawal attempt is an account event, so the account
                // exists even when the withdrawal then fails. Withdrawal ids
                // are not stored, the input guarantees uniqueness.
                let account = self.accounts.entry(tx.client).or_default();
                account.withdraw(amount)
            }
            Kind::Dispute | Kind::Resolve | Kind::Chargeback => Err(RejectReason::UnknownTx(tx.tx)),
        }
    }

    pub fn accounts(&self) -> impl Iterator<Item = (ClientId, &Account)> {
        self.accounts
            .iter()
            .map(|(client, account)| (*client, account))
    }

    pub fn account(&self, client: ClientId) -> Option<&Account> {
        self.accounts.get(&client)
    }
}

// The row parsed fine and the bank says no.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RejectReason {
    #[error("account is locked")]
    AccountLocked,
    #[error("insufficient available funds")]
    InsufficientFunds,
    #[error("tx {0} already exists")]
    DuplicateTx(TxId),
    #[error("tx {0} is not a known deposit")]
    UnknownTx(TxId),
    #[error("tx {tx} belongs to client {owner}, not client {claimed}")]
    ClientMismatch {
        tx: TxId,
        owner: ClientId,
        claimed: ClientId,
    },
    #[error("tx {0} is already under dispute")]
    AlreadyDisputed(TxId),
    #[error("tx {0} is not under dispute")]
    NotDisputed(TxId),
    #[error("tx {0} was already charged back")]
    AlreadyChargedBack(TxId),
    #[error("balance would overflow")]
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(text: &str) -> Amount {
        Amount::parse(text).unwrap()
    }

    fn deposit(client: ClientId, tx: TxId, text: &str) -> Transaction {
        Transaction {
            client,
            tx,
            kind: Kind::Deposit(amount(text)),
        }
    }

    fn withdrawal(client: ClientId, tx: TxId, text: &str) -> Transaction {
        Transaction {
            client,
            tx,
            kind: Kind::Withdrawal(amount(text)),
        }
    }

    #[test]
    fn a_duplicate_tx_from_another_client_does_not_create_that_client() {
        let mut engine = Engine::new();
        engine.apply(deposit(1, 5, "10")).unwrap();

        let result = engine.apply(deposit(2, 5, "1"));

        assert_eq!(result, Err(RejectReason::DuplicateTx(5)));
        assert!(engine.account(2).is_none());
        assert_eq!(engine.accounts().count(), 1);
    }

    #[test]
    fn a_failed_withdrawal_for_a_new_client_leaves_a_zero_row() {
        let mut engine = Engine::new();

        let result = engine.apply(withdrawal(3, 1, "5"));

        assert_eq!(result, Err(RejectReason::InsufficientFunds));
        let account = engine.account(3).unwrap();
        assert_eq!(account.available().to_string(), "0.0000");
        assert_eq!(account.total().to_string(), "0.0000");
        assert!(!account.is_locked());
    }

    #[test]
    fn a_deposit_rejected_on_a_locked_account_is_not_stored() {
        let mut engine = Engine::new();
        engine.apply(deposit(1, 1, "10")).unwrap();
        let account = engine.accounts.get_mut(&1).unwrap();
        account.hold(amount("10")).unwrap();
        account.charge_back(amount("10")).unwrap();

        let result = engine.apply(deposit(1, 2, "5"));

        assert_eq!(result, Err(RejectReason::AccountLocked));
        assert!(engine.deposits.contains_key(&1));
        assert!(!engine.deposits.contains_key(&2));
    }
}
