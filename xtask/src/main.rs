//! Read-only workspace development instruments.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "xtask is a CLI that reports derived workspace evidence"
)]

mod atlas;

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
        Some("nontest-loc") => {
            let (Some(path), None) = (args.next(), args.next()) else {
                return Err("usage: cargo xtask nontest-loc <FILE>".to_owned());
            };
            let text =
                std::fs::read_to_string(&path).map_err(|err| format!("read {path}: {err}"))?;
            println!("{}", atlas_core::source::non_test_loc(&text));
            Ok(())
        }
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
