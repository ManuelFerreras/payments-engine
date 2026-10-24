use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const BINARY: &str = env!("CARGO_BIN_EXE_payments_engine");

// Fixture name and the number of stderr lines its input produces.
const EXPECTED_STDERR_LINES: [(&str, usize); 19] = [
    ("basic_spec_example", 1),
    ("dispute_after_withdrawal_negative", 1),
    ("dispute_chargeback_locks", 2),
    ("dispute_on_withdrawal_ignored", 2),
    ("dispute_resolve", 1),
    ("duplicate_tx_ignored", 3),
    ("header_only", 0),
    ("insufficient_funds", 3),
    ("large_values_no_overflow", 0),
    ("locked_account_behaviour", 3),
    ("malformed_rows_skipped", 14),
    ("messy_input_tolerated", 4),
    ("redispute_after_resolve", 4),
    ("reference_rows_amount_and_field_count", 0),
    ("unknown_client_rows", 3),
    ("unknown_tx_references", 5),
    ("unordered_ids_many_clients", 0),
    ("whitespace_and_precision", 1),
    ("wrong_client_dispute", 3),
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn run(args: &[&str]) -> Output {
    Command::new(BINARY).args(args).output().unwrap()
}

fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter_map(|file| file.strip_suffix(".in.csv").map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn expected_stderr_lines(name: &str) -> usize {
    EXPECTED_STDERR_LINES
        .iter()
        .find(|(fixture, _)| *fixture == name)
        .map(|(_, lines)| *lines)
        .unwrap_or_else(|| panic!("fixture {name} has no entry in the stderr table"))
}

fn is_row_report(line: &str) -> bool {
    let rest = line
        .strip_prefix("skipped row ")
        .or_else(|| line.strip_prefix("rejected row "));
    match rest {
        Some(rest) => rest.starts_with(|c: char| c.is_ascii_digit()),
        None => false,
    }
}

fn temp_file(contents: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "payments_engine_cli_{}_{nanos}.csv",
        std::process::id()
    ));
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn every_fixture_prints_its_expected_output_and_reports_each_bad_row_once() {
    let names = fixture_names();
    assert_eq!(
        names.len(),
        EXPECTED_STDERR_LINES.len(),
        "fixture count differs from the table"
    );

    for name in &names {
        let input = fixtures_dir().join(format!("{name}.in.csv"));
        let expected_stdout = fs::read(fixtures_dir().join(format!("{name}.out.csv"))).unwrap();

        let output = run(&[input.to_str().unwrap()]);

        assert_eq!(output.status.code(), Some(0), "{name}: exit code");
        assert_eq!(output.stdout, expected_stdout, "{name}: stdout");

        let stderr = String::from_utf8(output.stderr).unwrap();
        let lines: Vec<&str> = stderr.lines().collect();
        assert_eq!(
            lines.len(),
            expected_stderr_lines(name),
            "{name}: stderr line count\n{stderr}"
        );
        for line in lines {
            assert!(
                is_row_report(line),
                "{name}: unexpected stderr line {line:?}"
            );
        }
    }
}

#[test]
fn a_missing_file_exits_1_with_nothing_on_stdout() {
    let output = run(&["/definitely/not/here/transactions.csv"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn no_argument_exits_2_with_a_usage_line() {
    let output = run(&[]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn a_header_missing_a_required_column_exits_1_with_nothing_on_stdout() {
    let path = temp_file("type,client,amount\ndeposit,1,1.0\n");

    let output = run(&[path.to_str().unwrap()]);
    fs::remove_file(&path).unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
