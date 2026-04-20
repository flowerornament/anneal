use super::printer::{Location, TableHeader, format_number};
use super::*;

fn plain() -> OutputStyle {
    OutputStyle::plain()
}

fn render<F>(f: F) -> String
where
    F: FnOnce(&mut Printer<&mut Vec<u8>>) -> std::io::Result<()>,
{
    let mut buf = Vec::new();
    {
        let mut p = Printer::new(&mut buf, plain());
        f(&mut p).unwrap();
    }
    String::from_utf8(buf).unwrap()
}

#[test]
fn format_number_thousands() {
    assert_eq!(format_number(0), "0");
    assert_eq!(format_number(1), "1");
    assert_eq!(format_number(999), "999");
    assert_eq!(format_number(1_000), "1,000");
    assert_eq!(format_number(12_557), "12,557");
    assert_eq!(format_number(1_234_567), "1,234,567");
    assert_eq!(format_number(-1_000), "-1,000");
}

#[test]
fn heading_with_count() {
    let out = render(|p| p.heading("Suggestions", Some(55)));
    assert_eq!(out, "  Suggestions (55)\n");
}

#[test]
fn heading_without_count() {
    let out = render(|p| p.heading("Corpus status", None));
    assert_eq!(out, "  Corpus status\n");
}

#[test]
fn kv_block_alignment() {
    let out = render(|p| {
        p.kv_block(&[
            ("Corpus status", Line::new().text("401 files")),
            ("Health", Line::new().text("2 errors")),
            ("Convergence", Line::new().text("holding")),
        ])
    });
    assert_eq!(
        out,
        "  Corpus status  401 files\n  Health         2 errors\n  Convergence    holding\n"
    );
}

#[test]
fn tally_mixed_zero_nonzero() {
    let out = render(|p| p.tally(&[(2, "errors"), (0, "warnings"), (1, "info")]));
    assert_eq!(out, "  2 errors , 0 warnings , 1 info\n");
}

#[test]
fn diagnostic_with_location() {
    let out = render(|p| {
        p.diagnostic(
            Severity::Warning,
            "I001",
            "270 section references use section notation",
            Some(Location::new("docs/spec.md", Some(42))),
        )
    });
    assert_eq!(
        out,
        "  warning[I001]     270 section references use section notation\n    at docs/spec.md:42\n"
    );
}

#[test]
fn diagnostic_without_line() {
    let out = render(|p| {
        p.diagnostic(
            Severity::Suggestion,
            "S001",
            "orphaned handle: OQ-64",
            Some(Location::new("OQ-64.md", None)),
        )
    });
    assert_eq!(
        out,
        "  suggestion[S001]  orphaned handle: OQ-64\n    at OQ-64.md\n"
    );
}

#[test]
fn hints_aligned() {
    let out = render(|p| {
        p.hints(&[
            ("anneal check", "for detailed diagnostics"),
            ("anneal garden", "for ranked maintenance tasks"),
        ])
    });
    assert_eq!(
        out,
        "  Try  anneal check   for detailed diagnostics\n       anneal garden  for ranked maintenance tasks\n"
    );
}

#[test]
fn bullet_uses_ascii_in_plain() {
    let out = render(|p| p.bullet(&Line::new().text("hello")));
    assert_eq!(out, "  - hello\n");
}

#[test]
fn indexed_list_pads_index() {
    let out = render(|p| {
        p.indexed(1, 2, &Line::new().text("alpha"))?;
        p.indexed(12, 2, &Line::new().text("beta"))
    });
    assert_eq!(out, "   1  alpha\n  12  beta\n");
}

#[test]
fn table_left_and_right_alignment() {
    let out = render(|p| {
        p.table(
            &[
                TableHeader::text("Name"),
                TableHeader::numeric("Count"),
                TableHeader::text("Notes"),
            ],
            &[
                vec![
                    Line::new().text("alpha"),
                    Line::new().count(3),
                    Line::new().text("ok"),
                ],
                vec![
                    Line::new().text("bb"),
                    Line::new().count(12),
                    Line::new().text("needs review"),
                ],
            ],
        )
    });
    assert_eq!(
        out,
        "  Name   Count  Notes\n  alpha      3  ok\n  bb        12  needs review\n"
    );
}

#[test]
fn line_display_width_ignores_ansi() {
    let line = Line::new().text("abc").dim("def");
    assert_eq!(line.display_width(), 6);
}

#[test]
#[allow(clippy::redundant_closure_for_method_calls)]
fn blank_is_lf_only() {
    let out = render(|p| p.blank());
    assert_eq!(out, "\n");
}

#[test]
fn rich_mode_selects_unicode_glyph() {
    let style = OutputStyle::new(Mode::Rich, false);
    assert_eq!(style.glyph(Glyph::Bullet), "•");
    assert_eq!(style.glyph(Glyph::Arrow), "→");
    assert_eq!(style.glyph(Glyph::Separator), "·");
}

#[test]
fn plain_mode_selects_ascii_glyph() {
    let style = OutputStyle::new(Mode::Plain, false);
    assert_eq!(style.glyph(Glyph::Bullet), "-");
    assert_eq!(style.glyph(Glyph::Arrow), "->");
    assert_eq!(style.glyph(Glyph::Separator), ",");
}
