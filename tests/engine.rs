use std::collections::HashMap;

use payments_engine::TxId;
use payments_engine::csv_io::read_transactions;
use payments_engine::engine::{Engine, RejectReason};
use payments_engine::money::{Amount, Money};
use payments_engine::transaction::{Kind, Transaction};

const HEADER: &str = "type,client,tx,amount\n";

type Outcome = (Transaction, Result<(), RejectReason>);

// Feeds the rows through the csv reader and the engine, exactly as main does.
// Every scenario row is well formed, so a parse failure is a test bug.
fn outcomes(rows: &str) -> (Engine, Vec<Outcome>) {
    let input = format!("{HEADER}{rows}");
    let mut engine = Engine::new();
    let mut outcomes = Vec::new();
    for item in read_transactions(input.as_bytes()).unwrap() {
        let (_, transaction) = item.unwrap();
        let result = engine.apply(transaction);
        outcomes.push((transaction, result));
    }
    (engine, outcomes)
}

fn run(rows: &str) -> (Engine, Vec<RejectReason>) {
    let (engine, outcomes) = outcomes(rows);
    let rejections = outcomes
        .into_iter()
        .filter_map(|(_, result)| result.err())
        .collect();
    (engine, rejections)
}

// The sum of total over all accounts equals accepted deposits minus accepted
// withdrawals minus charged back amounts. The rows are replayed to learn
// which ones were accepted, and the replay must produce the same rejections
// the scenario saw.
fn assert_conserved(rows: &str, engine: &Engine, rejections: &[RejectReason]) {
    let (_, outcomes) = outcomes(rows);
    let replayed: Vec<RejectReason> = outcomes
        .iter()
        .filter_map(|(_, result)| result.err())
        .collect();
    assert_eq!(replayed, rejections, "replay disagrees with the scenario");

    let mut deposits: HashMap<TxId, Amount> = HashMap::new();
    let mut expected = Money::ZERO;
    for (transaction, result) in &outcomes {
        if result.is_err() {
            continue;
        }
        match transaction.kind {
            Kind::Deposit(amount) => {
                deposits.insert(transaction.tx, amount);
                expected = expected.checked_add(amount.money()).unwrap();
            }
            Kind::Withdrawal(amount) => {
                expected = expected.checked_sub(amount.money()).unwrap();
            }
            Kind::Chargeback => {
                let amount = deposits[&transaction.tx];
                expected = expected.checked_sub(amount.money()).unwrap();
            }
            Kind::Dispute | Kind::Resolve => {}
        }
    }

    let mut actual = Money::ZERO;
    for (_, account) in engine.accounts() {
        actual = actual.checked_add(account.total()).unwrap();
    }
    assert_eq!(actual, expected, "money was created or destroyed");
}

fn balances(engine: &Engine, client: u16) -> (String, String, String, bool) {
    let account = engine.account(client).unwrap();
    (
        account.available().to_string(),
        account.held().to_string(),
        account.total().to_string(),
        account.is_locked(),
    )
}

fn strings(
    available: &str,
    held: &str,
    total: &str,
    locked: bool,
) -> (String, String, String, bool) {
    (available.into(), held.into(), total.into(), locked)
}

#[test]
fn the_sample_from_the_description_produces_the_documented_balances() {
    let rows = "deposit,1,1,1.0\n\
                deposit,2,2,2.0\n\
                deposit,1,3,2.0\n\
                withdrawal,1,4,1.5\n\
                withdrawal,2,5,3.0\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("1.5000", "0.0000", "1.5000", false)
    );
    assert_eq!(
        balances(&engine, 2),
        strings("2.0000", "0.0000", "2.0000", false)
    );
    assert_eq!(rejections.len(), 1);
    assert!(matches!(rejections[0], RejectReason::InsufficientFunds));
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_dispute_then_a_resolve_restores_the_balance_and_keeps_the_account_open() {
    let rows = "deposit,1,1,100.0\n\
                dispute,1,1,\n\
                resolve,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("100.0000", "0.0000", "100.0000", false)
    );
    assert!(rejections.is_empty());
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_dispute_then_a_chargeback_removes_the_funds_and_locks_the_account() {
    let rows = "deposit,1,1,100.0\n\
                deposit,1,2,25.0\n\
                dispute,1,1,\n\
                chargeback,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("25.0000", "0.0000", "25.0000", true)
    );
    assert!(rejections.is_empty());
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn the_fraud_case_leaves_the_bank_exposed_by_the_withdrawn_amount() {
    let rows = "deposit,1,1,100.0\n\
                withdrawal,1,2,80.0\n\
                dispute,1,1,\n\
                chargeback,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("-80.0000", "0.0000", "-80.0000", true)
    );
    assert!(rejections.is_empty());
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn an_unknown_tx_and_a_withdrawal_id_are_both_unknown_to_disputes() {
    let rows = "deposit,1,1,10.0\n\
                withdrawal,1,2,4.0\n\
                dispute,1,99,\n\
                dispute,1,2,\n\
                chargeback,1,2,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("6.0000", "0.0000", "6.0000", false)
    );
    assert_eq!(rejections.len(), 3);
    assert!(matches!(rejections[0], RejectReason::UnknownTx(99)));
    assert!(matches!(rejections[1], RejectReason::UnknownTx(2)));
    assert!(matches!(rejections[2], RejectReason::UnknownTx(2)));
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_reference_from_the_wrong_client_is_a_mismatch_and_changes_nothing() {
    let rows = "deposit,1,1,10.0\n\
                deposit,2,2,20.0\n\
                dispute,2,1,\n\
                chargeback,2,1,\n\
                resolve,2,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("10.0000", "0.0000", "10.0000", false)
    );
    assert_eq!(
        balances(&engine, 2),
        strings("20.0000", "0.0000", "20.0000", false)
    );
    assert_eq!(rejections.len(), 3);
    for rejection in &rejections {
        assert!(matches!(
            rejection,
            RejectReason::ClientMismatch {
                tx: 1,
                owner: 1,
                claimed: 2
            }
        ));
    }
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_resolve_or_chargeback_on_an_undisputed_deposit_is_not_disputed() {
    let rows = "deposit,1,1,10.0\n\
                resolve,1,1,\n\
                chargeback,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("10.0000", "0.0000", "10.0000", false)
    );
    assert_eq!(rejections.len(), 2);
    assert!(matches!(rejections[0], RejectReason::NotDisputed(1)));
    assert!(matches!(rejections[1], RejectReason::NotDisputed(1)));
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_duplicate_tx_is_rejected_in_all_three_shapes_and_the_first_deposit_stays_disputable() {
    let rows = "deposit,1,1,10.0\n\
                deposit,1,1,10.0\n\
                deposit,2,1,5.0\n\
                withdrawal,1,1,3.0\n\
                deposit,2,3,5.0\n\
                dispute,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("0.0000", "10.0000", "10.0000", false)
    );
    assert_eq!(
        balances(&engine, 2),
        strings("5.0000", "0.0000", "5.0000", false)
    );
    assert_eq!(rejections.len(), 3);
    for rejection in &rejections {
        assert!(matches!(rejection, RejectReason::DuplicateTx(1)));
    }
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_cross_client_duplicate_is_rejected_before_the_second_client_is_created() {
    let rows = "deposit,1,5,10.0\n\
                deposit,2,5,1.0\n";
    let (engine, rejections) = run(rows);

    assert_eq!(rejections.len(), 1);
    assert!(matches!(rejections[0], RejectReason::DuplicateTx(5)));
    assert_eq!(engine.accounts().count(), 1);
    assert!(engine.account(2).is_none());
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_second_dispute_is_already_disputed_and_anything_after_a_chargeback_is_already_charged_back() {
    let rows = "deposit,1,1,10.0\n\
                dispute,1,1,\n\
                dispute,1,1,\n\
                chargeback,1,1,\n\
                dispute,1,1,\n\
                resolve,1,1,\n\
                chargeback,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("0.0000", "0.0000", "0.0000", true)
    );
    assert_eq!(rejections.len(), 4);
    assert!(matches!(rejections[0], RejectReason::AlreadyDisputed(1)));
    assert!(matches!(rejections[1], RejectReason::AlreadyChargedBack(1)));
    assert!(matches!(rejections[2], RejectReason::AlreadyChargedBack(1)));
    assert!(matches!(rejections[3], RejectReason::AlreadyChargedBack(1)));
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn a_locked_account_refuses_money_movement_but_finishes_its_disputes() {
    let rows = "deposit,1,1,100.0\n\
                deposit,1,2,50.0\n\
                deposit,1,3,30.0\n\
                dispute,1,1,\n\
                chargeback,1,1,\n\
                deposit,1,4,10.0\n\
                withdrawal,1,5,10.0\n\
                dispute,1,2,\n\
                resolve,1,2,\n\
                dispute,1,3,\n\
                chargeback,1,3,\n\
                dispute,1,4,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("50.0000", "0.0000", "50.0000", true)
    );
    assert_eq!(rejections.len(), 3);
    assert!(matches!(rejections[0], RejectReason::AccountLocked));
    assert!(matches!(rejections[1], RejectReason::AccountLocked));
    assert!(matches!(rejections[2], RejectReason::UnknownTx(4)));
    assert_conserved(rows, &engine, &rejections);
}

#[test]
fn unordered_client_and_tx_ids_are_applied_in_file_order_and_printed_in_client_order() {
    let rows = "deposit,2,10,5.0\n\
                deposit,1,3,7.0\n\
                withdrawal,2,1,5.0\n\
                deposit,1,2,1.0\n\
                deposit,300,4294967000,0.1\n\
                deposit,30,8,0.2\n\
                deposit,3,7,0.3\n\
                withdrawal,300,9,0.1\n";
    let (engine, rejections) = run(rows);

    let clients: Vec<u16> = engine.accounts().map(|(client, _)| client).collect();
    assert_eq!(clients, vec![1, 2, 3, 30, 300]);
    assert_eq!(
        balances(&engine, 1),
        strings("8.0000", "0.0000", "8.0000", false)
    );
    assert_eq!(
        balances(&engine, 2),
        strings("0.0000", "0.0000", "0.0000", false)
    );
    assert_eq!(
        balances(&engine, 300),
        strings("0.0000", "0.0000", "0.0000", false)
    );
    assert!(rejections.is_empty());
    assert_conserved(rows, &engine, &rejections);
}

// A dispute on a disputed deposit must report AlreadyDisputed, never
// AlreadyChargedBack. With nine explicit arms the order cannot change that,
// this pins the rule against a future catch all arm on ChargedBack.
#[test]
fn a_redispute_after_a_resolve_is_allowed_and_a_third_dispute_is_already_disputed() {
    let rows = "deposit,1,1,10.0\n\
                dispute,1,1,\n\
                resolve,1,1,\n\
                dispute,1,1,\n\
                dispute,1,1,\n";
    let (engine, rejections) = run(rows);

    assert_eq!(
        balances(&engine, 1),
        strings("0.0000", "10.0000", "10.0000", false)
    );
    assert_eq!(rejections.len(), 1);
    assert!(matches!(rejections[0], RejectReason::AlreadyDisputed(1)));
    assert!(!matches!(
        rejections[0],
        RejectReason::AlreadyChargedBack(_)
    ));
    assert_conserved(rows, &engine, &rejections);
}
