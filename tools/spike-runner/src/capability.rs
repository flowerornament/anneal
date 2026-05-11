//! Reporting harness for MVS capability tests.
//!
//! Each capability is a function that produces (rows, verdict). The runner
//! emits NDJSON: one record per row, then one `CapabilityReport` summary
//! record per capability. This shape is what the parity harness diffs.
//!
//! Engine-specific code lives in the binaries (`ascent_spike`, future
//! `crepe_spike`); this module is engine-agnostic.

use serde::Serialize;
use std::io::{self, Write};

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// Outcome of a capability's expectation check.
#[derive(Copy, Clone, Debug)]
pub enum Verdict {
    Pass,
    Fail(&'static str),
}

impl Verdict {
    pub const fn is_pass(&self) -> bool { matches!(self, Self::Pass) }

    pub const fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Pass => None,
            Self::Fail(msg) => Some(*msg),
        }
    }
}

impl From<Result<(), &'static str>> for Verdict {
    fn from(r: Result<(), &'static str>) -> Self {
        match r {
            Ok(()) => Self::Pass,
            Err(msg) => Self::Fail(msg),
        }
    }
}

// ---------------------------------------------------------------------------
// CapabilityReport — one summary record per capability
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CapabilityReport {
    pub capability: &'static str,
    pub query: &'static str,
    pub row_count: usize,
    pub pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct CapabilityRow<'a, R: Serialize> {
    capability: &'static str,
    row: &'a R,
}

// ---------------------------------------------------------------------------
// Emit
// ---------------------------------------------------------------------------

/// Streams a capability's rows + summary as NDJSON to `out`.
///
/// Rows are emitted first (in caller-provided order — sort beforehand if
/// determinism matters), then a summary record. No buffering: one
/// `writeln!` per row.
pub fn emit<W, R>(
    out: &mut W,
    capability: &'static str,
    query: &'static str,
    rows: &[R],
    verdict: Verdict,
    detail: Option<&'static str>,
) -> io::Result<()>
where
    W: Write,
    R: Serialize,
{
    for row in rows {
        let wrapped = CapabilityRow { capability, row };
        serde_json::to_writer(&mut *out, &wrapped)?;
        out.write_all(b"\n")?;
    }
    let report = CapabilityReport {
        capability,
        query,
        row_count: rows.len(),
        pass: verdict.is_pass(),
        reason: verdict.reason(),
        detail,
    };
    serde_json::to_writer(&mut *out, &report)?;
    out.write_all(b"\n")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct DemoRow { id: &'static str, n: u32 }

    #[test]
    fn emit_writes_rows_then_summary_as_ndjson() {
        let rows = [DemoRow { id: "a", n: 1 }, DemoRow { id: "b", n: 2 }];
        let mut buf = Vec::new();
        emit(&mut buf, "DEMO", "? demo(x).", &rows, Verdict::Pass, None).unwrap();
        let out = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "expected 2 rows + 1 summary");
        for line in &lines[..2] { assert!(line.contains("\"capability\":\"DEMO\"")); }
        assert!(lines[2].contains("\"row_count\":2"));
        assert!(lines[2].contains("\"pass\":true"));
    }

    #[test]
    fn fail_verdict_serializes_reason() {
        let rows: [DemoRow; 0] = [];
        let mut buf = Vec::new();
        emit(&mut buf, "DEMO", "?.", &rows,
             Verdict::from(Err::<(), _>("expected at least one row")),
             Some("synthetic")).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains("\"pass\":false"));
        assert!(out.contains("\"reason\":\"expected at least one row\""));
        assert!(out.contains("\"detail\":\"synthetic\""));
    }

    #[test]
    fn from_result_converts_correctly() {
        assert!(matches!(Verdict::from(Ok::<(), &'static str>(())), Verdict::Pass));
        assert!(matches!(Verdict::from(Err::<(), &'static str>("nope")), Verdict::Fail("nope")));
    }
}
