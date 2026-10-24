// Benchmark input generator. One argument, the row count, csv on stdout.
// About 70 percent deposits, 20 withdrawals, 10 disputes and resolves on
// earlier deposits. Deterministic, so two runs produce the same file.
use std::io::{self, BufWriter, Write};

struct Lcg(u64);

impl Lcg {
    fn draw(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn main() -> io::Result<()> {
    let rows: u32 = match std::env::args().nth(1).and_then(|arg| arg.parse().ok()) {
        Some(rows) => rows,
        None => {
            eprintln!("usage: gen_large <rows>");
            std::process::exit(2);
        }
    };
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut out = BufWriter::new(io::stdout().lock());
    let mut deposits: Vec<(u32, u16)> = Vec::new();
    let mut disputed: Vec<(u32, u16)> = Vec::new();
    writeln!(out, "type,client,tx,amount")?;
    for tx in 1..=rows {
        let roll = rng.draw() % 100;
        let units = rng.draw() % 10_000_000;
        let amount = format!("{}.{:04}", units / 10_000, units % 10_000);
        if roll < 70 || deposits.is_empty() {
            let client = rng.draw() as u16;
            writeln!(out, "deposit,{client},{tx},{amount}")?;
            deposits.push((tx, client));
        } else if roll < 90 {
            // Smaller than deposits so most withdrawals are accepted.
            let (_, client) = deposits[(rng.draw() % deposits.len() as u64) as usize];
            writeln!(
                out,
                "withdrawal,{client},{tx},{}.{:04}",
                units / 100_000,
                units % 10_000
            )?;
        } else {
            match if roll % 2 == 1 { disputed.pop() } else { None } {
                Some((deposit_tx, client)) => writeln!(out, "resolve,{client},{deposit_tx},")?,
                None => {
                    let (deposit_tx, client) =
                        deposits[(rng.draw() % deposits.len() as u64) as usize];
                    writeln!(out, "dispute,{client},{deposit_tx},")?;
                    disputed.push((deposit_tx, client));
                }
            }
        }
    }
    out.flush()
}
