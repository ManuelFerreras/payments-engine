# Working rules for this repo

Small payments engine in Rust. docs/design-notes.md is the spec.
If something is not decided there, ask me, do not guess.

## Dependencies

- Only csv, serde with derive, and thiserror. Ask before adding anything, dev dependencies included.
- Edition 2024 as in Cargo.toml. No lints table, no clippy.toml, no toolchain file.

## Code

- No unwrap, expect, panic or todo outside tests, except where I say so.
- Money math is always checked_add or checked_sub.
- Comments only for decisions and invariants. No doc comments that restate a name.
- Test names are sentences, no test_ prefix.

## Process

- Do not commit. I commit from another terminal after reading the diff.
- Do not read anything outside this repo. The only exceptions are crate sources under ~/.cargo/registry when I ask you to check a library fact, and one scratch folder I will name.
- Before telling me a step is done, run cargo fmt, cargo clippy --all-targets -- -D warnings and cargo test, and show me the result.
- Do not start subagents unless I ask for one.
