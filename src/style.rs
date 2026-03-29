use console::Style;
use std::sync::LazyLock;

/// Cached styles — computed once, reused everywhere.
/// `console` automatically disables color when stdout is not a terminal.
pub(crate) struct Styles {
    pub(crate) error: Style,
    pub(crate) warning: Style,
    pub(crate) info: Style,
    pub(crate) suggestion: Style,
    pub(crate) label: Style,
    pub(crate) dim: Style,
    pub(crate) bold: Style,
    pub(crate) green: Style,
}

pub(crate) static S: LazyLock<Styles> = LazyLock::new(|| Styles {
    error: Style::new().red().bold(),
    warning: Style::new().yellow().bold(),
    info: Style::new().cyan().bold(),
    suggestion: Style::new().blue().bold(),
    label: Style::new().bold(),
    dim: Style::new().dim(),
    bold: Style::new().bold(),
    green: Style::new().green(),
});
