use std::io;

use csv::{ReaderBuilder, StringRecord, Trim, WriterBuilder};
use serde::Serialize;
use thiserror::Error;

use crate::ClientId;
use crate::account::Account;
use crate::transaction::{ParseError, RawRecord, Transaction};

const REQUIRED_NAMES: [&str; 4] = ["type", "client", "tx", "amount"];
const OUTPUT_HEADER: [&str; 5] = ["client", "available", "held", "total", "locked"];

// What stops the run.
#[derive(Debug, Error)]
pub enum FatalError {
    #[error("bad header {found:?}: need type, client, tx and amount, each exactly once")]
    Header { found: Vec<String> },
    #[error("read error: {0}")]
    Read(csv::Error),
    #[error("write error: {0}")]
    Write(csv::Error),
}

// What skips one row.
#[derive(Debug, Error)]
pub enum RowError {
    #[error("{0}")]
    Csv(csv::Error),
    #[error("a value sits in a position with no column name ({found} fields, {expected} named)")]
    ExtraFields { found: usize, expected: usize },
    #[error("{0}")]
    Parse(#[from] ParseError),
}

// The 1-based data row is the record index, because the header took index 0.
pub type Row = u64;

pub type RowResult = Result<(Row, Transaction), (Row, RowError)>;

pub struct Transactions<R: io::Read> {
    reader: csv::Reader<R>,
    // Trimmed and ASCII lowercased, kept whole with its empty names so that a
    // header such as `type,,client,tx,amount` cannot shift a column.
    headers: StringRecord,
    named: usize,
    record: StringRecord,
}

pub fn read_transactions<R: io::Read>(input: R) -> Result<Transactions<R>, FatalError> {
    let mut reader = ReaderBuilder::new()
        .trim(Trim::All)
        .flexible(true)
        .from_reader(input);
    let found = reader.headers().map_err(FatalError::Read)?;
    let headers: StringRecord = found
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    // An empty header record is an empty file: no rows, not an error.
    if !headers.is_empty() {
        for required in REQUIRED_NAMES {
            if headers.iter().filter(|name| *name == required).count() != 1 {
                return Err(FatalError::Header {
                    found: found.iter().map(str::to_owned).collect(),
                });
            }
        }
    }
    let named = headers.iter().filter(|name| !name.is_empty()).count();
    Ok(Transactions {
        reader,
        headers,
        named,
        record: StringRecord::new(),
    })
}

impl<R: io::Read> Transactions<R> {
    fn row(&self) -> Row {
        self.record
            .position()
            .map_or(0, |position| position.record())
    }

    fn has_value_without_column(&self) -> bool {
        self.record.iter().enumerate().any(|(index, field)| {
            !field.is_empty() && self.headers.get(index).is_none_or(str::is_empty)
        })
    }
}

impl<R: io::Read> Iterator for Transactions<R> {
    type Item = RowResult;

    fn next(&mut self) -> Option<RowResult> {
        loop {
            match self.reader.read_record(&mut self.record) {
                Ok(false) => return None,
                Err(error) => {
                    let row = error
                        .position()
                        .map_or_else(|| self.reader.position().record(), |p| p.record());
                    return Some(Err((row, RowError::Csv(error))));
                }
                Ok(true) => {}
            }
            let row = self.row();
            // A blank record is not a row: skipped silently, but it keeps its number.
            if self.record.iter().all(str::is_empty) {
                continue;
            }
            if self.has_value_without_column() {
                return Some(Err((
                    row,
                    RowError::ExtraFields {
                        found: self.record.len(),
                        expected: self.named,
                    },
                )));
            }
            // Pad to the header length: serde skips an ignored name by reading
            // its field, and a short row would otherwise fail with end of row.
            while self.record.len() < self.headers.len() {
                self.record.push_field("");
            }
            let parsed = self
                .record
                .deserialize::<RawRecord>(Some(&self.headers))
                .map_err(RowError::Csv)
                .and_then(|raw| Transaction::try_from(raw).map_err(RowError::Parse));
            return Some(match parsed {
                Ok(transaction) => Ok((row, transaction)),
                Err(error) => Err((row, error)),
            });
        }
    }
}

#[derive(Serialize)]
struct OutputRow {
    client: ClientId,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

pub fn write_accounts<'a, W: io::Write>(
    output: W,
    accounts: impl Iterator<Item = (ClientId, &'a Account)>,
) -> Result<(), FatalError> {
    // No BufWriter, csv buffers already. The header is written by hand because
    // the writer only emits one on the first serialised row and an empty
    // file must still print it.
    let mut writer = WriterBuilder::new().has_headers(false).from_writer(output);
    writer
        .write_record(OUTPUT_HEADER)
        .map_err(FatalError::Write)?;
    for (client, account) in accounts {
        let row = OutputRow {
            client,
            available: account.available().to_string(),
            held: account.held().to_string(),
            total: account.total().to_string(),
            locked: account.is_locked(),
        };
        writer.serialize(row).map_err(FatalError::Write)?;
    }
    writer
        .flush()
        .map_err(|error| FatalError::Write(csv::Error::from(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Amount;
    use crate::transaction::{Kind, RawKind};

    fn rows(input: &str) -> Vec<RowResult> {
        read_transactions(input.as_bytes()).unwrap().collect()
    }

    fn ok_rows(input: &str) -> Vec<(Row, Transaction)> {
        rows(input).into_iter().map(|item| item.unwrap()).collect()
    }

    fn deposit(client: ClientId, tx: u32, amount: &str) -> Transaction {
        Transaction {
            client,
            tx,
            kind: Kind::Deposit(Amount::parse(amount).unwrap()),
        }
    }

    #[test]
    fn header_names_are_trimmed_and_lowercased_and_columns_may_come_in_any_order() {
        let parsed = ok_rows(" Client , TX , Type , AMOUNT \n1,2,Deposit,3.0\n");
        assert_eq!(parsed, vec![(1, deposit(1, 2, "3.0"))]);
    }

    #[test]
    fn a_bom_and_crlf_endings_need_no_special_handling() {
        let parsed = ok_rows("\u{feff}type,client,tx,amount\r\ndeposit,1,2,3.0\r\n");
        assert_eq!(parsed, vec![(1, deposit(1, 2, "3.0"))]);
    }

    #[test]
    fn a_trailing_comma_on_the_header_and_on_rows_is_ignored() {
        let parsed = ok_rows("Type, Client, Tx, Amount,\nDispute, 1, 10,\ndeposit,1,1,1.0,\n");
        assert_eq!(parsed[0].1.kind, Kind::Dispute);
        assert_eq!(parsed[1], (2, deposit(1, 1, "1.0")));
    }

    #[test]
    fn an_empty_header_name_is_a_position_with_no_column() {
        let input = "type,,client,tx,amount\ndeposit,,1,1,1.0\ndeposit,x,1,2,1.0\n";
        let parsed = rows(input);
        assert_eq!(parsed[0].as_ref().unwrap(), &(1, deposit(1, 1, "1.0")));
        assert!(matches!(
            parsed[1],
            Err((
                2,
                RowError::ExtraFields {
                    found: 5,
                    expected: 4
                }
            ))
        ));
    }

    #[test]
    fn a_missing_required_name_is_a_bad_header() {
        let result = read_transactions("type,client,amount\n".as_bytes());
        assert!(matches!(result, Err(FatalError::Header { .. })));
    }

    #[test]
    fn a_required_name_appearing_twice_is_a_bad_header() {
        let result = read_transactions("type,client,tx,amount,amount\n".as_bytes());
        assert!(matches!(result, Err(FatalError::Header { .. })));
    }

    #[test]
    fn an_extra_named_column_is_ignored() {
        let parsed = ok_rows("type,client,tx,amount,note\ndeposit,1,2,3.0,hello\n");
        assert_eq!(parsed, vec![(1, deposit(1, 2, "3.0"))]);
    }

    #[test]
    fn an_empty_file_has_no_rows_and_is_not_an_error() {
        assert!(rows("").is_empty());
        assert!(rows("\n\r\n\n").is_empty());
    }

    #[test]
    fn a_header_only_file_has_no_rows() {
        assert!(rows("type,client,tx,amount\n").is_empty());
    }

    #[test]
    fn blank_records_are_skipped_silently_but_bare_empty_lines_take_no_number() {
        let input = "type,client,tx,amount\ndeposit,1,1,1.0\n\n   \n,,,\ndeposit,1,2,2.0\n";
        let parsed = ok_rows(input);
        assert_eq!(
            parsed,
            vec![(1, deposit(1, 1, "1.0")), (4, deposit(1, 2, "2.0"))]
        );
    }

    #[test]
    fn a_surplus_non_empty_field_skips_the_row_with_its_number() {
        let parsed = rows("type,client,tx,amount\ndeposit,1,1,1.0\ndeposit,1,13,1,5\n");
        assert!(parsed[0].is_ok());
        assert!(matches!(
            parsed[1],
            Err((
                2,
                RowError::ExtraFields {
                    found: 5,
                    expected: 4
                }
            ))
        ));
    }

    #[test]
    fn a_short_deposit_row_is_missing_its_amount() {
        let parsed = rows("type,client,tx,amount\ndeposit,1,6\n");
        assert!(matches!(
            parsed[0],
            Err((
                1,
                RowError::Parse(ParseError::MissingAmount(RawKind::Deposit))
            ))
        ));
    }

    #[test]
    fn an_empty_amount_field_is_missing_too() {
        let parsed = rows("type,client,tx,amount\nwithdrawal,1,5,\n");
        assert!(matches!(
            parsed[0],
            Err((
                1,
                RowError::Parse(ParseError::MissingAmount(RawKind::Withdrawal))
            ))
        ));
    }

    #[test]
    fn a_bad_client_or_tx_is_a_csv_error_on_that_row_and_the_run_continues() {
        let parsed = rows(
            "type,client,tx,amount\ndeposit,70000,1,1.0\ndeposit,1,4294967296,1.0\ndeposit,1,3,1.0\n",
        );
        assert!(matches!(parsed[0], Err((1, RowError::Csv(_)))));
        assert!(matches!(parsed[1], Err((2, RowError::Csv(_)))));
        assert_eq!(parsed[2].as_ref().unwrap(), &(3, deposit(1, 3, "1.0")));
    }

    #[test]
    fn write_accounts_prints_the_header_for_no_accounts() {
        let mut output = Vec::new();
        write_accounts(&mut output, std::iter::empty()).unwrap();
        assert_eq!(output, b"client,available,held,total,locked\n");
    }

    #[test]
    fn write_accounts_prints_one_row_per_account_with_four_decimals() {
        let mut account = Account::default();
        account.deposit(Amount::parse("1.5").unwrap()).unwrap();
        let accounts = [(7, &account)];
        let mut output = Vec::new();
        write_accounts(&mut output, accounts.into_iter()).unwrap();
        assert_eq!(
            output,
            b"client,available,held,total,locked\n7,1.5000,0.0000,1.5000,false\n"
        );
    }
}
