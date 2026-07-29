//! Read-only workspace development instruments.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a CLI that reports derived workspace evidence"
)]

mod atlas;
mod scan;

use std::{env, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("atlas") => atlas::run(args),
        Some("nontest-loc") => scan::nontest_loc(args),
        Some("-h" | "--help") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown xtask command `{command}`")),
    }
}

fn print_help() {
    println!(
        "\
anneal workspace instruments

Usage:
  cargo xtask atlas <verb> [--json] [--pub-only]
  cargo xtask nontest-loc <FILE>

Commands:
  atlas        Derived whole-system name and module map
  nontest-loc  Lines outside cfg-test-gated modules
"
    );
}
