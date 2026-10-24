use thiserror::Error;

use crate::{ClientId, TxId};

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
