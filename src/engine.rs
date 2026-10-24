use thiserror::Error;

// The row parsed fine and the bank says no.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RejectReason {
    #[error("account is locked")]
    AccountLocked,
    #[error("insufficient available funds")]
    InsufficientFunds,
    #[error("tx {0} already exists")]
    DuplicateTx(u32),
    #[error("tx {0} is not a known deposit")]
    UnknownTx(u32),
    #[error("tx {tx} belongs to client {owner}, not client {claimed}")]
    ClientMismatch { tx: u32, owner: u16, claimed: u16 },
    #[error("tx {0} is already under dispute")]
    AlreadyDisputed(u32),
    #[error("tx {0} is not under dispute")]
    NotDisputed(u32),
    #[error("tx {0} was already charged back")]
    AlreadyChargedBack(u32),
    #[error("balance would overflow")]
    Overflow,
}
