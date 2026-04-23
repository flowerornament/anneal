use std::env;
use std::io::IsTerminal;

use console::Style;

/// Output rendering mode, selected by CLI flags and environment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Mode {
    /// Full Unicode glyphs + color (subject to terminal detection).
    #[default]
    Rich,
    /// ASCII glyphs only. Color still allowed unless disabled.
    Minimal,
    /// ASCII only, no color. Used for piping, logging, accessibility.
    Plain,
}

/// Semantic text roles. Color is mapped to each role centrally so a
/// palette change is one-edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    /// Plain body text.
    Default,
    /// Bold, no color. Reserved for headings and KV keys.
    Heading,
    /// Dim. For secondary info and separators.
    Dim,
    /// File paths (cyan).
    Path,
    /// Numeric counts / IDs (cyan).
    Number,
    /// Error severity.
    Error,
    /// Warning severity.
    Warning,
    /// Informational severity.
    Info,
    /// Success severity.
    Success,
    /// Call-to-action / hints.
    Callout,
}

/// Glyph categories. Each maps to a tiered character based on `Mode`.
///
/// Inline separators and list-item markers are not glyphs — commas and
/// indentation carry those roles. Glyphs are reserved for semantic
/// pointers (arrows, severity marks).
#[allow(dead_code)] // full glyph palette; not every variant is used yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Glyph {
    /// Success check mark.
    Success,
    /// Failure cross.
    Error,
    /// Warning bang.
    Warning,
    /// Directional arrow (`→` / `->`).
    Arrow,
    /// Reverse arrow for "incoming" edges (`←` / `<-`).
    ArrowIn,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OutputStyle {
    pub(crate) mode: Mode,
    pub(crate) color: bool,
}

impl OutputStyle {
    /// Construct explicitly (used by tests and JSON-mode callers).
    pub(crate) const fn new(mode: Mode, color: bool) -> Self {
        Self { mode, color }
    }

    /// Plain-mode style with color off. Shared test constructor.
    #[cfg(test)]
    pub(crate) const fn plain() -> Self {
        Self::new(Mode::Plain, false)
    }

    /// Auto-detect color from `NO_COLOR`, `--plain`, and TTY state.
    /// `force_color = Some(true)` forces on; `Some(false)` forces off.
    pub(crate) fn detect(mode: Mode, force_color: Option<bool>) -> Self {
        let color = force_color.unwrap_or_else(|| {
            env::var_os("NO_COLOR").is_none()
                && mode != Mode::Plain
                && std::io::stdout().is_terminal()
        });
        Self { mode, color }
    }

    /// Resolve a tone to a `console::Style`. Returns `Style::new()` when
    /// color is disabled, which renders as plain text.
    pub(crate) fn tone(self, tone: Tone) -> Style {
        if !self.color {
            return match tone {
                // Even without color, Heading keeps its bold so scanability
                // survives in monochrome terminals and log files.
                Tone::Heading => Style::new().bold(),
                _ => Style::new(),
            };
        }
        match tone {
            Tone::Default => Style::new(),
            Tone::Heading => Style::new().bold(),
            Tone::Dim => Style::new().dim(),
            Tone::Path | Tone::Number => Style::new().cyan(),
            Tone::Error => Style::new().red().bold(),
            Tone::Warning => Style::new().yellow().bold(),
            Tone::Info => Style::new().cyan().bold(),
            Tone::Success => Style::new().green().bold(),
            Tone::Callout => Style::new().blue().bold(),
        }
    }

    /// Resolve a glyph to its rendered form for the current mode.
    pub(crate) fn glyph(self, g: Glyph) -> &'static str {
        match self.mode {
            Mode::Rich => match g {
                Glyph::Success => "✔",
                Glyph::Error => "✘",
                Glyph::Warning => "!",
                Glyph::Arrow => "→",
                Glyph::ArrowIn => "←",
            },
            Mode::Minimal | Mode::Plain => match g {
                Glyph::Success => "+",
                Glyph::Error => "x",
                Glyph::Warning => "!",
                Glyph::Arrow => "->",
                Glyph::ArrowIn => "<-",
            },
        }
    }
}

impl Default for OutputStyle {
    fn default() -> Self {
        Self::new(Mode::Plain, false)
    }
}
