use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use payments_engine::csv_io::{self, FatalError, RowError};
use payments_engine::engine::Engine;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let [path] = args.as_slice() else {
        report(format_args!("usage: payments_engine <transactions.csv>"));
        return ExitCode::from(2);
    };
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            report(format_args!("cannot open {path}: {error}"));
            return ExitCode::from(1);
        }
    };
    match run(file) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader closed early. Whatever it did read was correct, so the
        // run is not a failure. Checked on the csv error itself: converting
        // to io::Error first would turn the kind into Other.
        Err(FatalError::Write(error)) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            report(format_args!("{error}"));
            ExitCode::from(1)
        }
    }
}

fn run(file: File) -> Result<(), FatalError> {
    let mut engine = Engine::new();
    for item in csv_io::read_transactions(file)? {
        match item {
            Ok((row, transaction)) => {
                if let Err(reason) = engine.apply(transaction) {
                    report(format_args!("rejected row {row}: {reason}"));
                }
            }
            Err((_, RowError::Csv(error))) if error.is_io_error() => {
                return Err(FatalError::Read(error));
            }
            Err((row, error)) => report(format_args!("skipped row {row}: {error}")),
        }
    }
    csv_io::write_accounts(io::stdout().lock(), engine.accounts())
}

// Both the per row serialize error and the final flush error arrive here as
// FatalError::Write, so one check covers both.
fn is_broken_pipe(error: &csv::Error) -> bool {
    matches!(error.kind(), csv::ErrorKind::Io(io_error) if io_error.kind() == io::ErrorKind::BrokenPipe)
}

// The result is ignored on purpose: a closed stderr must not panic the way
// eprintln! would.
fn report(line: fmt::Arguments<'_>) {
    let _ = writeln!(io::stderr().lock(), "{line}");
}
