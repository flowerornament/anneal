use std::io::{self, Write};

use super::style::{OutputStyle, Tone};

/// Indentation constants. Column 0-1 is reserved for glyphs (gutter);
/// content starts at column 2. Nested detail (e.g., diagnostic detail
/// under a heading) starts at column 4.
pub(crate) const CONTENT_COL: usize = 2;
pub(crate) const SUB_COL: usize = 4;

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Severity {
    Error,
    Warning,
    Info,
    Suggestion,
}

impl Severity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Suggestion => "suggestion",
        }
    }

    pub(crate) fn tone(self) -> Tone {
        match self {
            Severity::Error => Tone::Error,
            Severity::Warning => Tone::Warning,
            Severity::Info => Tone::Info,
            Severity::Suggestion => Tone::Callout,
        }
    }
}

/// A source location paired with a diagnostic. `line` is the 1-indexed
/// line number when available; otherwise the diagnostic renders as
/// `at <path>` without a line suffix.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Location<'a> {
    pub(crate) path: &'a str,
    pub(crate) line: Option<u32>,
}

impl<'a> Location<'a> {
    pub(crate) const fn new(path: &'a str, line: Option<u32>) -> Self {
        Self { path, line }
    }
}

/// Associates a domain value with an output tone so commands don't
/// need per-type mapping helpers scattered across the CLI layer.
pub(crate) trait Toned {
    fn tone(&self) -> Tone;
}

/// A value that can render itself to a `Printer`. Every command output
/// type implements this so `main.rs::emit_rendered` can construct the
/// Printer once and hand it to the value — commands never see a bare
/// writer.
pub(crate) trait Render {
    fn render<W: Write>(&self, p: &mut Printer<W>) -> io::Result<()>;
}

/// A styled segment. Either a text run (with a tone) or a fixed-width
/// pad. Pads defer expansion until write time, avoiding `" ".repeat(n)`
/// allocations on hot paths.
#[derive(Clone, Debug)]
enum Seg {
    Text { text: String, tone: Tone },
    Pad(usize),
}

/// A line composed of typed, styled segments. Use the builder-style
/// methods to compose: `Line::new().heading("Health").text(" ").num(2).text(" errors")`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Line {
    segs: Vec<Seg>,
}

#[allow(dead_code)] // API surface; not every command uses every variant yet.
impl Line {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn text(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Default);
        self
    }

    #[must_use]
    pub(crate) fn heading(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Heading);
        self
    }

    #[must_use]
    pub(crate) fn dim(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Dim);
        self
    }

    #[must_use]
    pub(crate) fn path(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Path);
        self
    }

    /// Signed integer count (with thousands separator).
    #[must_use]
    pub(crate) fn num(mut self, n: i64) -> Self {
        self.push(format_number(n), Tone::Number);
        self
    }

    /// Unsigned count (with thousands separator). Prefer this for
    /// `.len()` / `usize` fields; keeps call sites free of casts.
    #[must_use]
    pub(crate) fn count(mut self, n: usize) -> Self {
        self.push(
            format_number(i64::try_from(n).unwrap_or(i64::MAX)),
            Tone::Number,
        );
        self
    }

    /// Fixed-precision float, numeric tone.
    #[must_use]
    pub(crate) fn float(mut self, n: f64, precision: usize) -> Self {
        self.push(format!("{n:.precision$}"), Tone::Number);
        self
    }

    /// Reserve `n` spaces of horizontal padding without allocating a
    /// string of spaces. Expanded at write time.
    #[must_use]
    pub(crate) fn pad(mut self, n: usize) -> Self {
        if n > 0 {
            self.segs.push(Seg::Pad(n));
        }
        self
    }

    #[must_use]
    pub(crate) fn callout(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Callout);
        self
    }

    #[must_use]
    pub(crate) fn success(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Success);
        self
    }

    #[must_use]
    pub(crate) fn warning(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Warning);
        self
    }

    #[must_use]
    pub(crate) fn error(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Error);
        self
    }

    #[must_use]
    pub(crate) fn info(mut self, s: impl Into<String>) -> Self {
        self.push(s.into(), Tone::Info);
        self
    }

    #[must_use]
    pub(crate) fn toned(mut self, tone: Tone, s: impl Into<String>) -> Self {
        self.push(s.into(), tone);
        self
    }

    fn push(&mut self, text: String, tone: Tone) {
        if text.is_empty() {
            return;
        }
        self.segs.push(Seg::Text { text, tone });
    }

    /// Total visible character width (for alignment calculations).
    pub(crate) fn display_width(&self) -> usize {
        self.segs
            .iter()
            .map(|s| match s {
                Seg::Text { text, .. } => console::measure_text_width(text),
                Seg::Pad(n) => *n,
            })
            .sum()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }
}

/// Unified terminal printer. Owns the writer and the output style.
///
/// All command output should flow through printer methods rather than
/// raw `writeln!` / inline `Style::apply_to`. This keeps the design
/// system enforceable and lets future refactors (theming, `--minimal`
/// mode, snapshot testing) land in one place.
pub(crate) struct Printer<W: Write> {
    writer: W,
    style: OutputStyle,
}

impl<W: Write> Printer<W> {
    pub(crate) fn new(writer: W, style: OutputStyle) -> Self {
        Self { writer, style }
    }

    pub(crate) fn style(&self) -> OutputStyle {
        self.style
    }

    // --- Primitives ---------------------------------------------------

    /// Emit a single blank line.
    pub(crate) fn blank(&mut self) -> io::Result<()> {
        writeln!(self.writer)
    }

    /// Emit a line of segments at the default content column (col 2).
    pub(crate) fn line(&mut self, line: &Line) -> io::Result<()> {
        self.line_at(CONTENT_COL, line)
    }

    /// Emit a line of segments at an explicit indent column.
    pub(crate) fn line_at(&mut self, col: usize, line: &Line) -> io::Result<()> {
        self.write_indent(col)?;
        self.write_segments(&line.segs)?;
        writeln!(self.writer)
    }

    /// Emit one raw line without styling. Reserved for passthrough shapes
    /// like DOT graph rendering and JSON.
    pub(crate) fn raw_line(&mut self, s: &str) -> io::Result<()> {
        writeln!(self.writer, "{s}")
    }

    fn write_indent(&mut self, col: usize) -> io::Result<()> {
        if col > 0 {
            write!(self.writer, "{:col$}", "", col = col)?;
        }
        Ok(())
    }

    fn write_segments(&mut self, segs: &[Seg]) -> io::Result<()> {
        for seg in segs {
            match seg {
                Seg::Text { text, tone } => {
                    write!(self.writer, "{}", self.style.tone(*tone).apply_to(text))?;
                }
                Seg::Pad(n) => {
                    write!(self.writer, "{:width$}", "", width = *n)?;
                }
            }
        }
        Ok(())
    }

    // --- Headings -----------------------------------------------------

    /// `**Title**` (bold) at column 2. Optional parenthetical count in
    /// dim. Empty counts are omitted so the pattern stays scannable.
    pub(crate) fn heading(&mut self, title: &str, count: Option<usize>) -> io::Result<()> {
        let mut line = Line::new().heading(title);
        if let Some(n) = count {
            line = line.text(" ").dim(format!(
                "({})",
                format_number(i64::try_from(n).unwrap_or(i64::MAX))
            ));
        }
        self.line(&line)
    }

    /// Dim single-line caption underneath a heading (ranking rationale,
    /// filter summary, etc.). Rendered at column 2.
    pub(crate) fn caption(&mut self, text: &str) -> io::Result<()> {
        self.line(&Line::new().dim(text))
    }

    // --- Key/value rows ----------------------------------------------

    /// One KV row with an explicit label column width. Use `kv_block`
    /// when emitting multiple aligned rows.
    pub(crate) fn kv(&mut self, key: &str, value: &Line, label_width: usize) -> io::Result<()> {
        self.write_indent(CONTENT_COL)?;
        let key_style = self.style.tone(Tone::Heading);
        write!(self.writer, "{}", key_style.apply_to(key))?;
        let pad = label_width.saturating_sub(key.len()) + 2;
        write!(self.writer, "{:pad$}", "", pad = pad)?;
        self.write_segments(&value.segs)?;
        writeln!(self.writer)
    }

    /// Aligned KV block. Label column width is computed from the widest
    /// key in `rows`, so all values line up.
    pub(crate) fn kv_block(&mut self, rows: &[(&str, Line)]) -> io::Result<()> {
        let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in rows {
            self.kv(k, v, width)?;
        }
        Ok(())
    }

    // --- Tally / summary row -----------------------------------------

    /// Emit a single row like `0 errors, 0 warnings, 1 info`. Each
    /// part pairs a count with a label. Zero-valued parts render dim so
    /// non-zero values pop. Comma separator — glyph separators are
    /// chartjunk; punctuation is prose.
    pub(crate) fn tally(&mut self, parts: &[(usize, &str)]) -> io::Result<()> {
        self.write_indent(CONTENT_COL)?;
        let sep = self.style.tone(Tone::Dim);
        for (i, (count, label)) in parts.iter().enumerate() {
            if i > 0 {
                write!(self.writer, "{}", sep.apply_to(", "))?;
            }
            let (num_tone, lbl_tone) = if *count == 0 {
                (Tone::Dim, Tone::Dim)
            } else {
                (Tone::Number, Tone::Default)
            };
            write!(
                self.writer,
                "{} {}",
                self.style
                    .tone(num_tone)
                    .apply_to(format_number(i64::try_from(*count).unwrap_or(i64::MAX))),
                self.style.tone(lbl_tone).apply_to(label),
            )?;
        }
        writeln!(self.writer)
    }

    // --- Indexed lists -----------------------------------------------
    //
    // Plain list items are indentation-only: emit via `line_at(SUB_COL, …)`.
    // No decorative bullet glyphs — indent already communicates grouping.

    /// Indexed list row: `  N  content`. Index is right-aligned within
    /// `width` columns so single- and double-digit indices share a
    /// content start column.
    pub(crate) fn indexed(&mut self, index: usize, width: usize, line: &Line) -> io::Result<()> {
        self.write_indent(CONTENT_COL)?;
        let idx = self
            .style
            .tone(Tone::Dim)
            .apply_to(format!("{index:>width$}"));
        write!(self.writer, "{idx}  ")?;
        self.write_segments(&line.segs)?;
        writeln!(self.writer)
    }

    // --- Diagnostics --------------------------------------------------

    /// Compiler-style diagnostic: `severity[CODE]  message` with an
    /// optional `at <path>[:<line>]` continuation on the next line.
    ///
    /// Severity and code are colored by `Severity::tone()`. The code is
    /// padded so 4-char codes (E001, W001, I001) align across rows.
    pub(crate) fn diagnostic(
        &mut self,
        severity: Severity,
        code: &str,
        message: &str,
        at: Option<Location<'_>>,
    ) -> io::Result<()> {
        let tone = self.style.tone(severity.tone());
        let label = format!("{}[{}]", severity.label(), code);
        self.write_indent(CONTENT_COL)?;
        // Width: "suggestion[S001]" is the widest label at 16 chars.
        let width = 18usize;
        write!(self.writer, "{:<width$}", tone.apply_to(&label))?;
        writeln!(self.writer, "{message}")?;
        if let Some(loc) = at {
            self.write_indent(SUB_COL)?;
            let at_word = self.style.tone(Tone::Dim).apply_to("at ");
            let path_text = match loc.line {
                Some(ln) => format!("{}:{ln}", loc.path),
                None => loc.path.to_string(),
            };
            let path_styled = self.style.tone(Tone::Path).apply_to(path_text);
            writeln!(self.writer, "{at_word}{path_styled}")?;
        }
        Ok(())
    }

    // --- Hints --------------------------------------------------------

    /// "Try" hint block: a single `Try` gutter followed by rows of
    /// `command  description`. Commands are aligned to the widest entry
    /// so the second column reads as a table.
    pub(crate) fn hints(&mut self, rows: &[(&str, &str)]) -> io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let cmd_width = rows.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
        let try_label = self.style.tone(Tone::Callout).apply_to("Try");
        for (i, (cmd, desc)) in rows.iter().enumerate() {
            self.write_indent(CONTENT_COL)?;
            if i == 0 {
                write!(self.writer, "{try_label}  ")?;
            } else {
                write!(self.writer, "     ")?;
            }
            let cmd_styled = self.style.tone(Tone::Default).apply_to(cmd);
            write!(self.writer, "{cmd_styled:<cmd_width$}  ")?;
            let desc_styled = self.style.tone(Tone::Dim).apply_to(desc);
            writeln!(self.writer, "{desc_styled}")?;
        }
        Ok(())
    }

    // --- Tables -------------------------------------------------------

    /// Lightweight borderless table. Caller passes header labels and
    /// rows of pre-styled `Line`s. Column widths are computed from the
    /// max display width across header + all rows per column.
    ///
    /// Alignment is left-by-default; mark a column as `numeric: true`
    /// to right-align its cells.
    pub(crate) fn table(
        &mut self,
        headers: &[TableHeader<'_>],
        rows: &[Vec<Line>],
    ) -> io::Result<()> {
        if headers.is_empty() {
            return Ok(());
        }
        let col_count = headers.len();
        let mut widths = vec![0usize; col_count];
        for (i, h) in headers.iter().enumerate() {
            widths[i] = console::measure_text_width(h.label);
        }
        for row in rows {
            for (i, cell) in row.iter().take(col_count).enumerate() {
                widths[i] = widths[i].max(cell.display_width());
            }
        }
        // Header
        self.write_indent(CONTENT_COL)?;
        for (i, h) in headers.iter().enumerate() {
            let is_last = i + 1 == col_count;
            let hstyled = self.style.tone(Tone::Heading).apply_to(h.label);
            let hwidth = console::measure_text_width(h.label);
            let pad = widths[i].saturating_sub(hwidth);
            if h.numeric {
                write!(self.writer, "{:pad$}{hstyled}", "", pad = pad)?;
            } else if is_last {
                write!(self.writer, "{hstyled}")?; // skip trailing padding on last col
            } else {
                write!(self.writer, "{hstyled}{:pad$}", "", pad = pad)?;
            }
            if !is_last {
                write!(self.writer, "  ")?;
            }
        }
        writeln!(self.writer)?;
        // Rows
        for row in rows {
            self.write_indent(CONTENT_COL)?;
            for (i, h) in headers.iter().enumerate() {
                let is_last = i + 1 == col_count;
                let empty = Line::new();
                let cell = row.get(i).unwrap_or(&empty);
                let cwidth = cell.display_width();
                let pad = widths[i].saturating_sub(cwidth);
                if h.numeric {
                    write!(self.writer, "{:pad$}", "", pad = pad)?;
                    self.write_segments(&cell.segs)?;
                } else if is_last {
                    self.write_segments(&cell.segs)?;
                } else {
                    self.write_segments(&cell.segs)?;
                    write!(self.writer, "{:pad$}", "", pad = pad)?;
                }
                if !is_last {
                    write!(self.writer, "  ")?;
                }
            }
            writeln!(self.writer)?;
        }
        Ok(())
    }
}

/// Column descriptor for `Printer::table`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TableHeader<'a> {
    pub(crate) label: &'a str,
    pub(crate) numeric: bool,
}

impl<'a> TableHeader<'a> {
    pub(crate) const fn text(label: &'a str) -> Self {
        Self {
            label,
            numeric: false,
        }
    }

    pub(crate) const fn numeric(label: &'a str) -> Self {
        Self {
            label,
            numeric: true,
        }
    }
}

/// Format an integer with thousands separators: 12557 → "12,557".
pub(crate) fn format_number(n: i64) -> String {
    let sign = if n < 0 { "-" } else { "" };
    let abs = n.unsigned_abs().to_string();
    let grouped: Vec<&str> = abs
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).expect("ascii digits"))
        .collect();
    format!("{sign}{}", grouped.join(","))
}
