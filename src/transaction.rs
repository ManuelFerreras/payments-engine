use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

use crate::money::{Amount, AmountError};
use crate::{ClientId, TxId};

// Mirrors one data row of the file. The two text fields borrow from the csv
// record, so a row costs no allocation beyond the record itself.
#[derive(Debug, Deserialize)]
pub struct RawRecord<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub client: ClientId,
    pub tx: TxId,
    #[serde(borrow)]
    pub amount: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawKind {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

const RAW_KINDS: [RawKind; 5] = [
    RawKind::Deposit,
    RawKind::Withdrawal,
    RawKind::Dispute,
    RawKind::Resolve,
    RawKind::Chargeback,
];

impl RawKind {
    fn name(self) -> &'static str {
        match self {
            RawKind::Deposit => "deposit",
            RawKind::Withdrawal => "withdrawal",
            RawKind::Dispute => "dispute",
            RawKind::Resolve => "resolve",
            RawKind::Chargeback => "chargeback",
        }
    }
}

impl FromStr for RawKind {
    type Err = ParseError;

    // Matched ignoring ASCII case without lowercasing into a new String.
    fn from_str(text: &str) -> Result<RawKind, ParseError> {
        let trimmed = text.trim();
        for kind in RAW_KINDS {
            if trimmed.eq_ignore_ascii_case(kind.name()) {
                return Ok(kind);
            }
        }
        Err(ParseError::UnknownType(text.to_owned()))
    }
}

impl fmt::Display for RawKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("unknown transaction type {0:?}")]
    UnknownType(String),
    #[error("{0} row has no amount")]
    MissingAmount(RawKind),
    #[error("bad amount: {0}")]
    Amount(#[from] AmountError),
}

// Owned, so nothing borrowed from the record leaves the read loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transaction {
    pub client: ClientId,
    pub tx: TxId,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Deposit(Amount),
    Withdrawal(Amount),
    Dispute,
    Resolve,
    Chargeback,
}

impl TryFrom<RawRecord<'_>> for Transaction {
    type Error = ParseError;

    fn try_from(raw: RawRecord<'_>) -> Result<Transaction, ParseError> {
        let raw_kind: RawKind = raw.kind.parse()?;
        let kind = match raw_kind {
            RawKind::Deposit => Kind::Deposit(required_amount(raw_kind, raw.amount)?),
            RawKind::Withdrawal => Kind::Withdrawal(required_amount(raw_kind, raw.amount)?),
            // Reference rows ignore the amount column. The amount comes from
            // the referenced deposit.
            RawKind::Dispute => Kind::Dispute,
            RawKind::Resolve => Kind::Resolve,
            RawKind::Chargeback => Kind::Chargeback,
        };
        Ok(Transaction {
            client: raw.client,
            tx: raw.tx,
            kind,
        })
    }
}

fn required_amount(kind: RawKind, amount: Option<&str>) -> Result<Amount, ParseError> {
    match amount {
        Some(text) => Ok(Amount::parse(text)?),
        None => Err(ParseError::MissingAmount(kind)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw<'a>(kind: &'a str, amount: Option<&'a str>) -> RawRecord<'a> {
        RawRecord {
            kind,
            client: 7,
            tx: 42,
            amount,
        }
    }

    #[test]
    fn type_names_match_ignoring_ascii_case_and_surrounding_whitespace() {
        assert_eq!("Deposit".parse::<RawKind>(), Ok(RawKind::Deposit));
        assert_eq!("WITHDRAWAL".parse::<RawKind>(), Ok(RawKind::Withdrawal));
        assert_eq!("  dispute ".parse::<RawKind>(), Ok(RawKind::Dispute));
        assert_eq!("ReSoLvE".parse::<RawKind>(), Ok(RawKind::Resolve));
        assert_eq!("ChargeBack".parse::<RawKind>(), Ok(RawKind::Chargeback));
    }

    #[test]
    fn an_unknown_type_keeps_the_original_text() {
        assert_eq!(
            "depositt".parse::<RawKind>(),
            Err(ParseError::UnknownType("depositt".to_owned()))
        );
    }

    #[test]
    fn a_deposit_row_carries_its_parsed_amount() {
        let transaction = Transaction::try_from(raw("deposit", Some("1.5"))).unwrap();
        assert_eq!(transaction.client, 7);
        assert_eq!(transaction.tx, 42);
        assert_eq!(
            transaction.kind,
            Kind::Deposit(Amount::parse("1.5").unwrap())
        );
    }

    #[test]
    fn a_withdrawal_row_without_an_amount_is_missing_amount() {
        assert_eq!(
            Transaction::try_from(raw("withdrawal", None)),
            Err(ParseError::MissingAmount(RawKind::Withdrawal))
        );
    }

    #[test]
    fn a_bad_amount_surfaces_the_amount_error() {
        assert_eq!(
            Transaction::try_from(raw("deposit", Some("1,5"))),
            Err(ParseError::Amount(AmountError::NotPlainDecimal))
        );
    }

    #[test]
    fn a_reference_row_ignores_any_amount_it_carries() {
        let transaction = Transaction::try_from(raw("dispute", Some("99.0"))).unwrap();
        assert_eq!(transaction.kind, Kind::Dispute);
    }

    #[test]
    fn a_reference_row_needs_no_amount() {
        assert_eq!(
            Transaction::try_from(raw("chargeback", None)).unwrap().kind,
            Kind::Chargeback
        );
    }

    #[test]
    fn an_unknown_type_is_rejected_before_the_amount_is_read() {
        assert_eq!(
            Transaction::try_from(raw("foo", Some("abc"))),
            Err(ParseError::UnknownType("foo".to_owned()))
        );
    }

    #[test]
    fn a_raw_record_borrows_both_text_fields_from_a_csv_record() {
        let data = "type,client,tx,amount\nDeposit,1,2,3.0\n";
        let mut reader = csv::Reader::from_reader(data.as_bytes());
        let headers = reader.headers().unwrap().clone();
        let record = reader.records().next().unwrap().unwrap();
        let raw: RawRecord<'_> = record.deserialize(Some(&headers)).unwrap();
        assert_eq!(raw.kind, "Deposit");
        assert_eq!(raw.client, 1);
        assert_eq!(raw.tx, 2);
        assert_eq!(raw.amount, Some("3.0"));
    }
}
