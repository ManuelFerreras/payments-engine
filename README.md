# payments-engine

Reads a CSV of transactions, applies deposits, withdrawals, disputes, resolves and chargebacks to client accounts, and prints the final balances as CSV.

```
cargo run -- transactions.csv > accounts.csv
```

Needs Rust 1.85 or newer, the crate is on edition 2024. Balances go to stdout. Skipped and rejected rows go to stderr, one line each as `skipped row N` or `rejected row N`. N counts data rows from 1, header excluded, so it is the same number on LF and CRLF files. A bare empty line takes no number, a whitespace only line does. Exit 0 when the file was processed, bad rows included. Exit 1 on a fatal error: missing file, a header without the four required columns or with one of them twice, a read or write failure. Exit 2 on wrong usage. Nothing is written to stdout on 1 or 2, except a write failure part way through the output, which leaves what was already written.

## How I read the problem

I read this as a small bank ledger fed by a partner file. A deposit is money a partner credited to a client. A withdrawal is the client taking money out, and it only works against available funds. A dispute is the partner pulling a deposit back while the matter is investigated, so the money moves from available to held and the total does not change. A resolve returns it. A chargeback means the partner won, the money is gone, and the account is frozen because the client is now a risk. This framing settles most open questions. Only a credit can be pulled back, so only deposits are disputable. A dispute proceeds even when the client already spent the money, because that is the fraud the file is meant to catch, and the negative balance is the exposure the institution carries. A frozen account still takes network events on its old deposits, because those events happened whether we like it or not.

## Decisions

| Situation | Decision | Why |
|---|---|---|
| Dispute, resolve or chargeback names a withdrawal | Rejected as an unknown transaction | The hold arithmetic only works for credited funds |
| Dispute on a deposit the client already withdrew | Dispute applies in full, available goes negative | The negative number is the bank's exposure, and refusing the dispute would let the fraudster keep the money |
| Dispute, resolve or chargeback from a client who does not own the deposit | Rejected | Otherwise any client could freeze another client's funds |
| Resolve on a disputed deposit | Deposit returns to undisputed and can be disputed again | Undisputed is the state before the dispute, and card networks allow a second cycle |
| Chargeback | Terminal for the deposit, account locked | The spec requires freezing the account on chargeback |
| Deposit or withdrawal on a locked account | Rejected | Stopping client initiated movement is what freezing is for |
| Dispute, resolve or chargeback on a locked account | Processed if the deposit exists | Refusing network events on old deposits leaves open disputes in limbo |
| Withdrawal larger than available | Rejected, nothing changes | Held money cannot be spent |
| Transaction id already used by a stored deposit | Second row rejected, first one wins | Duplicate ids are a partner error, and withdrawal ids are not stored because the spec guarantees uniqueness |
| Amount present on a dispute, resolve or chargeback row | Ignored | The amount comes from the referenced deposit |
| Amount with more than four decimals, a sign, an exponent, a separator, or missing on a deposit or withdrawal | Row skipped | A bank does not guess at a fifth decimal or at `1e3` |
| Zero amount on a deposit or withdrawal | Applied as a no-op, the account is created. An overdrawn account still refuses every withdrawal, zero included | A zero event is noise from the partner, not an error, and a reference that adds the amount records it the same way |
| Amount or balance above the i64 range of about 922 trillion | Rejected, never rounded | Overflow is a typed rejection, never a panic or a silent wrap |
| First row for a client is a withdrawal that fails | Account is created with zero balance | A withdrawal attempt is an account event, while reference rows address a transaction and never create accounts |
| Row with content beyond the header's columns, a bad number, or an unknown type | Row skipped with one stderr line, run continues. A trailing comma is not content, surplus empty fields are ignored | One bad row should not stop the file, and `deposit,1,13,1,5` is probably a comma decimal, so guessing which field is the amount is dangerous |
| Header with a trailing comma, extra named columns, capitals, or a different column order | Accepted. Names are trimmed and lowercased, empty and extra names are ignored. Only a missing `type`, `client`, `tx` or `amount` is fatal, or one of them named twice | A header that still names the four columns describes the file. One that does not makes every row meaningless, so the whole file is wrong |
| Empty file | Output header only, exit 0 | No rows means no accounts, which is not an error |

The decision I am least sure of is the locked account rule. The other reading, refuse every row on a locked account, is also defensible. The fixture `locked_account_behaviour` pins mine, so changing it is one lock check in three account methods, that fixture and two tests.

Whitespace around fields and header names is trimmed. Type names are matched ignoring case, so `Deposit` is a deposit and `depositt` is an unknown type. Blank lines, whitespace only lines and lines of bare commas are not rows. Client and transaction ids go through the integer parser, so `+1` is accepted as 1. No money moves on an id, so I left that alone. Files with CRLF endings or a UTF-8 BOM are accepted. Output is sorted by client id so runs are easy to diff.

## Design

```
src/
  main.rs         CLI: argument, open file, loop, report, write, exit code
  lib.rs          module list and the two id aliases
  money.rs        Money and Amount, the fixed point types
  transaction.rs  raw CSV row and the validated Transaction it becomes
  account.rs      one account, balance rules, no ids
  engine.rs       accounts map, deposit index, dispute state machine, apply
  csv_io.rs       reading transactions from any Read, writing balances to any Write
```

Money is an `i64` count of 1/10,000 units, the usual way a ledger stores amounts. `Money` is signed because balances can go negative by design. `Amount` wraps a `Money` that cannot be negative and can only be built by parsing, so once a deposit or withdrawal exists the engine cannot receive a negative, malformed or over-precise number. Both types use checked arithmetic, with one saturating add reserved for the derived total and guarded by a debug assertion. rust_decimal was the obvious alternative. I probed it for this use at the start of the build and found four behaviours that each needed a guard: csv infers the field as a float under default features, the parser accepts exponents, the padded formatter panics on wide values, and `checked_add` near the top of the range returns a result with a dropped decimal. Four guards around a crate was more code than the integer type, which is under a hundred lines.

Total is never stored. It is computed as available plus held, so the identity cannot drift. Every mutation computes all new values first and assigns only after every check passed, so a rejected row leaves the account untouched.

The engine is a pure state machine. `Engine::apply` takes a `Transaction` and returns `Ok` or a typed `RejectReason`. It does no IO and knows nothing about files or CSV. Accounts live in a `BTreeMap` keyed by client, which has at most 65,536 entries and iterates sorted for free. Deposits live in a `HashMap` keyed by tx id with the owner, the amount and the dispute state, because that map can grow to billions of keys and order does not matter there. Withdrawals are not stored.

## Correctness

Four layers of tests, all run by `cargo test`. The money type has a table of assertions in `tests/money.rs`, written from the case list in the design notes and committed before the type existed, covering every accepted and rejected input and every formatting edge, including `i64::MIN`. The account type has unit tests for each rule and both reachable overflow paths. `tests/engine.rs` has scenario tests through the public API, named after the decisions above, including the fraud pattern: deposit, withdraw most of it, dispute, chargeback, negative total, locked account. Every scenario also checks conservation: all totals summed equal accepted deposits minus accepted withdrawals minus charged back amounts. `tests/cli.rs` runs the real binary against 19 fixture pairs in `tests/fixtures`, worked out during planning and checked against a small reference model outside the repo, including the sample from the brief, compares stdout byte for byte, and adds three negative tests for the exit codes. Rejections are typed enums and tests match on variants, so error wording can change freely.

## Safety and robustness

No `unsafe`. No `unwrap`, `expect` or `panic!` outside tests. One `debug_assert!` guards the total identity in debug builds and compiles out of release. All money movement is checked, and `clippy::arithmetic_side_effects` is denied in the two modules that touch balances so clippy proves it on every gate run. A malformed row, or one the bank refuses, costs one stderr line and processing continues. A missing file, a header without the four required columns or with one of them twice, or a read error exits 1 with nothing on stdout, because partial balances presented as final are worse than no answer. A write failure exits 1 too, except a closed pipe, which exits 0 so `| head` works. A read error part way through the file and a write error that is not a closed pipe take that same fatal path. Neither is exercised by a test, both were checked by reading the code, and the review noted them as unverified. Amounts go through a short grammar with an exhaustive test table, so `+3`, `-0`, `1e3`, `1,5` and a fifth decimal never become money.

## Efficiency

The reader yields one row at a time and nothing accumulates except the accounts map and the deposit index. Peak memory is proportional to the number of accounts plus the number of deposits, not to the number of rows.

I generated 10,000,000 rows (302 MB) with `examples/gen_large.rs` (about 70 percent deposits) and ran the release build on a MacBook Pro with an Apple M3 Pro and 18 GB:

- 10,000,000 rows in 5.13 seconds, about 1.9 million rows per second (`/usr/bin/time -l`, the macOS flag, `-v` on Linux)
- peak resident memory 425 MB (424,591,360 bytes) with 7,001,404 deposits in the index and 65,536 accounts, about 61 bytes per stored deposit

The design notes predicted two to three million rows per second and the build does about 1.9. I did not profile the gap. The likely cost is the per row UTF-8 validation and the serde path, and the number is enough for the file sizes this tool is for.

Each stored deposit is a 4 byte key and a 16 byte record, 24 bytes in the table slot, plus one control byte, the load factor and the doubling growth of the hash table, so between 30 and 65 bytes in practice. Measured above: 61. The honest ceiling is the deposit index: at that rate 100 million deposits need about 6 GB, and the full u32 space does not fit on one machine. Past that point there are two moves, one sentence each. Drop a record once it is charged back, which is terminal and never read again, that frees the fraud cases but not the honest ones. Or spill the index to disk, which keeps the engine the same and moves the cost to the lookup. Not a smaller struct.

If the rows came from thousands of concurrent TCP streams I would keep this engine and put it behind channels. It is `Send`, does no IO and takes plain `Transaction` values. I would run N shards, each owning one engine for the clients where `client % N == shard`, each behind a bounded mpsc channel. Connection tasks decode rows and send each one to its client's shard. All state for one client lives in one shard because every reference row must carry the owning client's id, which the engine already enforces, so no cross shard coordination is needed. Bounded channels give backpressure: a slow shard slows its producers instead of growing memory. Order within one stream is preserved by the channel. Order across streams for the same client is undefined by the spec and would need a sequencing rule a real system would have to specify. The honest limit is global tx uniqueness: a shard sees every deposit for its clients, so a duplicate inside a client is caught, but a cross client duplicate would need a shared set or a coordinator, and I would ship that as a documented gap before I would add a lock to the hot path. I did not build this because the spec asks for a CLI over one file, and an async runtime would add a second execution model to review on top of forty lines of domain logic.

## What I left out

- The sharded engine above. The API is shaped for it, the runtime is not here.
- A property test generating random transaction sequences and asserting conservation and no panics. The conservation check runs on every hand written scenario instead. This is the next test I would add.
- A set of withdrawal ids for full duplicate detection. The spec guarantees uniqueness, and in production the upstream ledger's idempotency key does this job.
- i128 money. A small change, the width sits in a handful of places plus the cap values in the money tests and one fixture, if the range ever matters more than memory.
- clap, logging crates, async. One argument, one `writeln!` to stderr per bad row, one thread.

## AI use

I used Claude Code (Anthropic) while building this, and this section says how.

Before writing any code I worked through the problem with Claude in a planning conversation, mostly on the cases the description leaves open. The decisions in `docs/design-notes.md` are mine and were taken there. The prose was drafted in that conversation and tidied by Claude Code inside this repo before the build, which is the first exchange in the transcript. The notes were committed before any code and are the spec the build worked from. The rules the session ran under are in CLAUDE.md, committed before the first prompt.

The money cases and the fraud scenario are in the notes, written before any code existed. In the build I had the model write `tests/money.rs` from that table and the account fraud test from that scenario before it wrote either implementation. I read both against the notes and committed them before the code they test, so the money grammar and the hold semantics were fixed before the model implemented them. All the code was typed by Claude Code in one session inside this repo, one module per step, from the design notes. After every step I ran fmt, clippy with warnings denied and the tests, read the diff, and made the commit myself. One commit, Engine Cleanup, is a change I made by hand without the model.

Near the end I had a clean context review the crate against the design notes for correctness gaps only. It ran the gate, the fixtures and its own boundary inputs, and came back with no correctness findings and two observations, which I ruled on in the transcript.

The full session transcript is in `docs/ai-transcript.md`, exported from the local session log with a small script. It contains every prompt I typed and every reply, in order, with tool calls summarised and long tool outputs truncated. Nothing was removed. The reviewer subagent's transcript is appended. The only commit after the export is the one that adds the transcript.

Things the model got wrong or that I sent back, from the transcript:

- While tidying the design notes it wrote that the 19 fixtures were written by hand. They were generated by a script and checked against a reference model, and I corrected the sentence.
- It changed the edition line in the notes to match Cargo.toml and told me afterwards instead of asking.
- At the money step it created four empty module files so the whole module list would compile. I had them removed, each module lands in the commit that creates it.
- Asked to confirm a std fact, it fetched the std source from the web because rust-src was not installed, which was outside the rules I had set. I told it to ask first next time.

This README was drafted in that same planning conversation from the decisions and the measured numbers, and I edited it. The planning conversation is not in the repo because it contains the text of the brief, which the rules say must not be committed. It exists and I can share a redacted export on request.

Every assumption and every decision here is mine. I can walk through any prompt or any line.

## Dependencies

- `csv`: streaming CSV reader and writer, named by the brief.
- `serde` with `derive`: deserialising the raw row and serialising the output row, named by the brief.
- `thiserror`: `Display` and `Error` impls for the typed error enums, over a billion downloads, one well known maintainer.

Nothing else. No dev-dependencies. Money is parsed, added and formatted by under a hundred lines in this crate, so no third party code touches an amount.
