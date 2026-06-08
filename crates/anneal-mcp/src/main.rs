//! MCP server entry point for anneal tools.

use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--tools") => {
            let mut stdout = io::stdout().lock();
            for tool in anneal_mcp::stable_tools() {
                writeln!(stdout, "{tool}")?;
            }
        }
        Some("-h" | "--help") | None => {
            let mut stdout = io::stdout().lock();
            stdout.write_all(
                b"Usage: cargo run -p anneal-mcp -- --tools\n\n\
anneal-mcp is a crate-level developer surface.\n\
The installed anneal binary does not expose anneal --mcp or anneal mcp yet.\n",
            )?;
        }
        Some(other) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown anneal-mcp option {other:?}; try --help"),
            )
            .into());
        }
    }
    Ok(())
}
