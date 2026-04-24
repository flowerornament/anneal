//! Unified terminal output for anneal.
//!
//! All human-facing text goes through `Printer`. Commands express *what*
//! to render (heading, kv, diagnostic, hint) and the printer handles
//! layout, color, glyph tier, and indentation.
//!
//! Design principles (Tufte-inspired):
//! - Data-ink ratio: every colored or bold character must encode meaning.
//! - Strategic emphasis: bold only for headings and keys; color only for
//!   semantic roles (error, warning, success, path, number, callout).
//! - Small multiples: the same shape appears everywhere a concept repeats
//!   (counts always thousands-separated, paths always `Tone::Path`, hints
//!   always via `hints()`).
//! - No chartjunk: no borders, box-drawing, or separators that don't
//!   encode structure. Whitespace is the layout medium.

mod printer;
mod style;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use printer::{
    Line, Location, Printer, Render, Severity, TableHeader, Toned, format_number,
};
pub(crate) use style::{Glyph, Mode, OutputStyle, Tone};
