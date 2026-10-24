# Design notes

Written before the build. These are the rules I will implement and why. The build works from this file alone.

A command line tool that streams a CSV of transactions into per client accounts and prints the balances. Three choices will surprise a reader: only deposits can be disputed, a dispute holds the full amount even when that pushes available negative, and a locked account still takes disputes, resolves and chargebacks on its old deposits. Money is an i64 in units of 1/10,000 with checked arithmetic. Tests live inline, in `tests/` and in `tests/fixtures`. The benchmark is `examples/gen_large.rs` plus `/usr/bin/time`, see section 4.

## 1. What the program does

One argument, the input path. The tool reads the CSV, applies each row in order to its client account, and prints the final balances as CSV.

```
cargo run -- transactions.csv > accounts.csv
```

Balances go to stdout, everything else to stderr. Exit 0 means the file was processed, even if rows were skipped and even if stdout closed early on a broken pipe. Exit 1 is fatal: the file cannot be opened, a required header name is missing, or a read or write error happened. Exit 2 is a wrong argument count, with a usage line on stderr. On exit 1 or 2 nothing goes to stdout, because partial balances that look final are worse than no answer. The exception is a write error part way through the output, which exits 1 with whatever was already written. An empty file is not an error. It prints the header and exits 0.

Input is a header row, then one transaction per row. Columns: `type` (one of five words, matched ignoring ASCII case), `client` (u16), `tx` (u32), `amount` (decimal text, up to four places after the dot, may be empty or missing). Whitespace is trimmed, header names are lowercased, column order is free. Rows are chronological and tx ids are unique across the file.

An account has `available`, `held` and `locked`. `total` is always available plus held. The five kinds:

- deposit: available and total go up by the amount. Creates the account if new.
- withdrawal: available and total go down. Fails when available is below the amount and then changes nothing. Creates the account even when it fails.
- dispute: names an earlier deposit by tx id. Available goes down by that amount, held goes up by the same, total unchanged.
- resolve: names a disputed deposit. Held goes down, available goes up, total unchanged. The deposit is no longer disputed.
- chargeback: names a disputed deposit. Held and total go down and the account is locked. Terminal for that deposit.

The amount column is read only on deposits and withdrawals.

Output is the header `client,available,held,total,locked`, then one row per account sorted by client id. Amounts have exactly four decimals with a minus sign in front when negative. `locked` is `true` or `false`.

## 2. Assumptions where the spec is silent

1. Only deposits can be disputed. The hold arithmetic only works for credited funds.
2. A dispute, resolve or chargeback must name the client that owns the deposit. Otherwise any client could freeze another client's money.
3. A dispute holds the full amount even when the client already withdrew it, so available can go negative. That number is the bank's exposure and I will not hide it.
4. After a resolve the deposit can be disputed again. Card networks allow a second cycle.
5. Chargeback is terminal for the deposit.
6. A locked account rejects deposits and withdrawals but still processes disputes, resolves and chargebacks on its existing deposits. Those are network events, and refusing them leaves disputes in limbo. Nothing unlocks an account. A resolve on a locked account releases the hold into available and the lock stays.
7. Withdrawals need available funds, not total. Held money cannot be withdrawn.
8. A deposit or withdrawal whose tx id matches a stored deposit is rejected, first one wins. I do not store withdrawal ids. The input guarantees uniqueness, and storing them would double the index for nothing.
9. Amounts are plain decimals: digits, optionally one dot and one to four digits, no sign, no exponent, no separators. `.5` and `5.` pass. Zero is accepted and applied as a no-op that still creates the account, with no stderr line, because a zero event from the partner is noise rather than an error. A fifth decimal is rejected even when it is a zero. A bank does not guess.
10. Amounts and balances are capped at i64 in units of 1/10,000, about 922 trillion. Over the cap on parse is TooLarge, on a sum it is Overflow. Never rounded, never a panic.
11. An amount on a dispute, resolve or chargeback row is ignored. The amount comes from the referenced deposit.
12. Type names are matched ignoring ASCII case, so `Deposit` and `WITHDRAWAL` pass. The five names are fixed. `depositt` is unknown and the row is skipped.
13. A row with a non-empty field in a position that has no header name is skipped. Surplus empty fields, as a trailing comma produces, are ignored. `deposit,1,1,1.0,` passes. `deposit,1,13,1,5` is probably a comma decimal, and guessing there is dangerous.
14. A row that fails to parse or is refused by the engine is reported on stderr with its data row number, and the run continues. One bad row should not stop the file. A bare empty line is not a row and does not take a number, so the count is the count of records, not of lines.
15. A withdrawal that fails for insufficient funds still creates the zero account, because the attempt is an account event. A duplicate tx id creates nothing, since that check runs before the account is touched. A row that fails to parse never reaches the engine and creates nothing either. Disputes, resolves and chargebacks never create an account.
16. Blank records are not rows. An empty line, a whitespace only line and a line of bare commas (`,,,`) are skipped silently, with no stderr line.
17. Output is sorted by client id so runs are diffable and golden tests are byte exact.
18. Header names are trimmed and ASCII lowercased, and the header record is kept whole, positions included, so a `type,,client,tx,amount` header cannot shift a column. An empty name is a position with no column. Extra named columns are ignored. Only a missing `type`, `client`, `tx` or `amount` is fatal. A required name that appears twice is also fatal. A header I cannot read unambiguously is not a header.
19. An empty file (zero bytes or only line endings) has no rows. The output header is printed and the exit code is 0.

## 3. Design

Lib plus thin bin, flat `src/`: `main.rs` (args, open, loop, report, exit code), `lib.rs`, `money.rs`, `transaction.rs`, `account.rs`, `engine.rs` and `csv_io.rs`. Reading takes any `io::Read` and writing any `io::Write`, so tests never touch the filesystem.

Money. `Money(i64)` is a signed count of 1/10,000 units. It has its own parser for the grammar in assumption 9 (trim, whitelist digits and dot, `split_once('.')`, right pad the fraction to four digits, `checked_mul`, `checked_add`) and its own `Display`: four decimals, minus in front, `unsigned_abs` so `i64::MIN` prints. The only operations are `checked_add`, `checked_sub`, `saturating_add` (for `total` alone) and `is_negative`. No `Deref`, no `From<i64>`, no public field, no operator overloads. `Amount(Money)` is never negative and is built only by `Amount::parse`. Two types because a balance can be negative by design and an input amount cannot. rust_decimal is the obvious alternative and I will probe it with a scratch program before committing to the integer type: how csv deserialises a `Decimal` under default features, whether `from_str` accepts exponents or signs, what `{:.4}` does with many whole digits, and whether `checked_add` near the top of the range keeps the fourth decimal. If each of those needs a guard, the integer type is less code. Integer minor units is also what exchange ledgers usually do. i128 would double the deposit record from 16 to 32 bytes.

Two stage parse. `csv::ReaderBuilder` with `Trim::All` and `flexible(true)`. Header names are trimmed and lowercased, and the header record is kept whole so positions never shift. Empty and extra names are ignored. Only a missing required name is fatal. An empty header record means an empty file: print the output header, exit 0. Then `read_record` loops into one reused `StringRecord` behind a hand written `Iterator`, with the row number from `position().record()`, which is already the 1-based data row because the header took index 0. Per record, in this order. If every field is empty the record is a blank line and is skipped silently. If a non-empty field sits in a position with no header name, the row is skipped with one stderr line. Then the record is padded to the header length, because a short row under a trailing empty name otherwise fails with end of row, which is exactly what the messy fixture has. Then it is deserialised into a borrowed `RawRecord` with `kind: &str`, `client: u16`, `tx: u32` and `amount: Option<&str>`, so a row allocates nothing beyond the record itself. Then `TryFrom<RawRecord> for Transaction`: type names are matched ignoring ASCII case, so `Deposit` and `WITHDRAWAL` pass and `depositt` is unknown, and the result is `Kind::{Deposit(Amount), Withdrawal(Amount), Dispute, Resolve, Chargeback}`. That `TryFrom` is the only source of `ParseError`: `UnknownType`, `MissingAmount` or `Amount(AmountError)`. A tagged serde enum does not work here, csv supports unit variants only. The `deserialize` iterator does not work either, it drops surplus fields silently and hides the row number. A UTF-8 BOM and CRLF endings are handled by the csv crate.

State. `Account { available: Money, held: Money, locked: bool }` with private fields and total derived. The methods `deposit`, `withdraw`, `hold`, `release` and `charge_back` each return `Result<(), RejectReason>`, compute every new value first and assign only after every check passed. Checks run in a fixed order, locked first, then funds, then overflow, so a locked account always reports AccountLocked. `total()` uses `saturating_add` with a `debug_assert!` that the checked sum exists, which makes the invariant executable in tests instead of a comment to trust. `Engine { accounts: BTreeMap<u16, Account>, deposits: HashMap<u32, DepositRecord> }`, where the record holds the client, the amount and a `DisputeState { Undisputed, Disputed, ChargedBack }`. BTreeMap gives sorted output for free over at most 65,536 keys. HashMap for up to four billion deposits, order irrelevant, default hasher.

Dispute state machine. The three reference kinds map to a private `Reference { Dispute, Resolve, Chargeback }` first, so the match on `(Reference, DisputeState)` is exactly nine arms and exhaustive. Dispute on Undisputed holds and moves to Disputed, otherwise `AlreadyDisputed` or `AlreadyChargedBack`. Resolve on Disputed releases and returns to Undisputed, otherwise `NotDisputed` or `AlreadyChargedBack`. Chargeback on Disputed charges back, locks and moves to ChargedBack, otherwise the same two. An unknown id is `UnknownTx`.

Order of checks in `apply`. Deposit: duplicate check first, so `DuplicateTx` fires before the accounts map is touched, then `entry().or_default()`, then `account.deposit(amount)?`, then insert the record. The record goes in only after the account accepted, so a deposit refused on a locked account is never disputable later. Withdrawal: duplicate check, entry, withdraw. Reference rows: find the record (`UnknownTx`), check the owner (`ClientMismatch`), check the state, then fetch the account with `ok_or` and a one line comment that the fallback is unreachable because the deposit created the account. No unwrap.

Errors. Five thiserror enums, one per layer. `AmountError` and `ParseError` mean the row is malformed. `RejectReason` means the row is fine and the bank says no: `AccountLocked`, `InsufficientFunds`, `DuplicateTx`, `UnknownTx`, `ClientMismatch`, `AlreadyDisputed`, `NotDisputed`, `AlreadyChargedBack` and `Overflow`. `RowError` is one per iterator item: `Csv`, `ExtraFields` or `Parse`. `FatalError` is what stops the run: `Header`, `Read` or `Write`. A csv error whose `is_io_error()` is true is fatal, everything else is a skipped row. stderr says `skipped row N: ...` for parser refusals and `rejected row N: ...` for engine refusals, N being the 1-based data row. Not the file line, because csv 1.4's `position().line()` skips blank lines and is off by one on CRLF files, while the record count is never wrong. stderr goes through `writeln!` with the result ignored, so a closed stderr cannot panic the way `eprintln!` would.

Output. `WriterBuilder::new().has_headers(false)` over locked stdout, with the header written by hand first, because the writer only emits one on the first serialised row and an empty file must still print it. No BufWriter, csv buffers already. A private `OutputRow` with `Serialize`, amounts through `to_string()`, locked as `bool`. Iterate the BTreeMap, flush. Broken pipe: match `csv::ErrorKind::Io(e)` with `e.kind() == BrokenPipe` on the csv error itself, before any conversion to `io::Error` loses the kind, and check both the per row error and the flush error. Exit 0.

## 4. Testing plan

Four layers.

1. `money.rs` unit table, about 35 asserts. Accepted: `1.0`, ` 2.50 `, `1.5000`, `0.0001`, `2`, `.5`, `5.`, `00012.3400`, `99999999999999.9999`, `922337203685477.5807`, and `0` and `0.0000` through both `Money::parse` and `Amount::parse`. Rejected: `1.50000`, `0.00001` (TooManyDecimals), `-5`, `-0`, `+3`, `1e3`, `1,5`, `1_000`, `abc`, `1.2.3`, `.` (NotPlainDecimal), empty and blanks (Empty), `922337203685477.5808` and a 20 digit number (TooLarge). Display: `-20.0000`, `-0.0001`, `0.0000`, `i64::MAX`, `i64::MIN`. `checked_add` at MAX is None.
2. `account.rs` unit tests: deposit then withdraw, insufficient funds leaves state untouched, locked rejects deposit and withdrawal, hold drives available negative with total unchanged, release restores, charge_back reduces held and total and locks, overflow on available (MAX then `0.0001`) and on total (`500000000000000`, dispute it, same deposit again), total equals available plus held after every operation.
3. `tests/engine.rs` scenarios through the public API with a helper `run(rows: &str) -> (Engine, Vec<RejectReason>)`. The sample from the description, dispute then resolve, dispute then chargeback, the fraud case (deposit 100, withdraw 80, dispute the 100, charge it back, expect available -80, held 0, total -80, locked), unknown tx and dispute on a withdrawal id, wrong client, resolve or chargeback on an undisputed deposit, duplicates in all three shapes, a cross client duplicate (client 1 deposits tx 5, client 2 reuses tx 5 and has no other row, assert `DuplicateTx` and exactly one account, which pins the duplicate check before account creation), already disputed and already charged back, lifecycle on a locked account, unordered ids. The redispute scenario (dispute, resolve, dispute, dispute) pins the match arm order: `(Dispute, Disputed)` must come before any `(_, ChargedBack)` arm, or the last dispute reports the wrong reason, and the compiler cannot catch that. Every scenario checks conservation: the sum of `total()` over all accounts equals accepted deposits minus accepted withdrawals minus charged back amounts, computed by the test from the rows it fed and the rejections it got back. Assert variants with `matches!`.
4. `tests/cli.rs` golden tests. Fixtures in `tests/fixtures/<name>.in.csv` and `<name>.out.csv`, 19 cases worked out during planning and checked against a small reference model outside the repo, named after the behaviour (`malformed_rows_skipped`, `wrong_client_dispute`, `large_values_no_overflow`). `messy_input_tolerated` covers the input rules in one file: a UTF-8 BOM, CRLF endings, a capitalised header with a trailing comma, trailing commas on data rows, mixed case type names, a blank line and a whitespace only line. The test file holds one table of fixture name and expected stderr line count. Run the binary with `env!("CARGO_BIN_EXE_payments_engine")`. stdout byte exact, exit 0, stderr line count equal to the table and each line starting with `skipped row ` or `rejected row ` then digits. Wording is not asserted. Three negative tests: missing file exits 1 with empty stdout, no argument exits 2, wrong header exits 1 with empty stdout.

Benchmark. `examples/gen_large.rs` with a tiny LCG and no dependencies, N rows, about 70 percent deposits, 20 withdrawals, 10 disputes and resolves. `cargo run --release --example gen_large -- 10000000 > big.csv`, then `/usr/bin/time -l cargo run --release -- big.csv > /dev/null`. Record rows, seconds, rows per second, peak RSS and deposits in the index. I expect two to three million rows per second and 50 to 70 bytes of peak RSS per stored deposit.

## 5. Left out on purpose

- async or threads: one file, one thread. The sharding story is a README paragraph.
- traits for storage: one storage, one engine. A trait with one impl is a lie about the future.
- clap: one positional argument, `std::env::args` is enough.
- rust_decimal: I probe it first, see section 3. If the probe surprises me, this file changes before the code does.
- proptest: conservation is checked by hand in every scenario. The strategy and the dependency do not fit the budget.
- log or tracing: one `writeln!` to stderr per bad row is all the observability a CLI needs.
- newtypes for client and tx ids: u16 and u32 cannot be confused.
- operator overloads on Money: they would hide a checked op or bring back a panicking plus.

## 6. Style rules

Edition 2024. Three crates: csv, serde with derive, thiserror. No dev-dependencies. Cargo.lock committed. No `unwrap`, `expect`, `panic!` or indexing on untrusted lengths outside tests. Checked arithmetic wherever money moves, with `#![deny(clippy::arithmetic_side_effects)]` in `money.rs` and `account.rs` only. No `[lints]` table, no clippy.toml. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean before every commit. Comments only for decisions and invariants. Test names are sentences without a `test_` prefix. Inline `#[cfg(test)] mod tests` next to the code, `tests/` for scenarios and the CLI. Imports in three groups: std, external, crate. No banners, no emoji, no TODOs.
