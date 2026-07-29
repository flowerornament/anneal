//! The atlas's Rust item-scanner authority.
//!
//! One line-based grammar answers "what items does this file declare" for every
//! atlas view and for `cargo xtask nontest-loc`. It is deliberately
//! approximate: no `syn`, no second Rust parser. Declaration lines are the
//! intentional interface, bodies are excluded, and text inside string literals
//! or block comments can misclassify. See `xtask/README.md` for the contract.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;

/// The item kinds the grammar recognizes. Census policy belongs to consumers;
/// the scanner reports everything it can name, including `const` and `static`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Fn,
    Mod,
    MacroRules,
    /// A `pub use` (or `pub(crate)`/`pub(super)` use) — a re-export, never a
    /// declaration; private `use` imports are not items at all.
    PubUse,
    Const,
    Static,
    /// A top-level `impl` block, named by its target type (`impl Tile`,
    /// `impl Trait for Tile` → `Tile`). Not a census kind — it exists because
    /// the density grade's denominator is "top-level items" and the worst
    /// god-objects are one giant impl with zero public types.
    Impl,
}

/// Declaration visibility. Restricted scopes beyond the census vocabulary map
/// to their nearest rung: `pub(self)` is private, `pub(in path)` counts as
/// crate-internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Visibility {
    Pub,
    PubCrate,
    PubSuper,
    Private,
}

/// One recognized declaration line, borrowing from the scanned line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineItem<'a> {
    pub(crate) kind: ItemKind,
    pub(crate) visibility: Visibility,
    pub(crate) name: &'a str,
}

/// One declared item with its declaration block — the carrier the atlas views
/// fold into census rows and co-mention edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) kind: ItemKind,
    pub(crate) visibility: Visibility,
    pub(crate) name: String,
    pub(crate) file: PathBuf,
    /// 1-indexed declaration line.
    pub(crate) line: usize,
    /// The declaration text used for co-mention edges: the signature for fns
    /// (through the body-opening brace on multi-line signatures), the body to
    /// the matching closing brace for structs/enums (fields and variants are
    /// the composition edges), a `pub use` through its semicolon, and the
    /// single declaration line otherwise.
    pub(crate) declaration_block: String,
    /// First line of the item's `///` doc comment — the atlas's comment
    /// surface. `None` is a visible documentation gap, not an omission of the
    /// scanner.
    pub(crate) doc: Option<String>,
    /// Whether the declaration starts at column 0. Under rustfmt this is the
    /// "top-level item" boundary the concentration view counts:
    /// methods inside an impl and items inside inline modules are indented
    /// and therefore not top-level.
    pub(crate) top_level: bool,
}

/// `cargo xtask nontest-loc <FILE>` — print the non-test LOC of one file.
pub(crate) fn nontest_loc(mut args: impl Iterator<Item = String>) -> Result<()> {
    let (Some(path), None) = (args.next(), args.next()) else {
        return Err("usage: cargo xtask nontest-loc <FILE>".to_owned());
    };
    let text = fs::read_to_string(&path).map_err(|err| format!("read {path}: {err}"))?;
    println!("{}", non_test_loc(&text));
    Ok(())
}

/// Every test-only region of one file: each top-level inline module whose
/// `cfg` gate implies `test`, whatever the module is named.
///
/// This is the one test boundary authority shared by the atlas census,
/// relationship heuristics, and concentration report. It is a set of regions
/// rather than a single terminal line because test code is not always last: a
/// file may open a test module, return to production code, and close with
/// another test module. A single terminal boundary previously hid production
/// code placed between two test modules.
///
/// Test-only *helpers* outside a module (`#[cfg(test)] fn helper()`) stay
/// counted as production, deliberately: gating one item is a property of that
/// item, and treating it as a boundary hides the production code after it
/// (the replicate.rs regression class).
pub(crate) struct TestRegions {
    /// Inclusive 1-based line ranges, in source order and non-overlapping.
    spans: Vec<(usize, usize)>,
    total: usize,
}

impl TestRegions {
    pub(crate) fn of(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let mut spans = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let opened = gates_test(lines[index])
                .then(|| declaration_after_attributes(&lines, index))
                .flatten()
                .filter(|&start| opens_inline_module(lines[start]));
            match opened {
                Some(start) => {
                    let end = closing_line(&lines, start);
                    spans.push((index + 1, end + 1));
                    index = end + 1;
                }
                None => index += 1,
            }
        }
        Self {
            spans,
            total: lines.len(),
        }
    }

    /// Whether a 1-based line lies in a test region.
    pub(crate) fn covers(&self, line: usize) -> bool {
        self.spans
            .iter()
            .any(|&(start, end)| (start..=end).contains(&line))
    }

    /// The first test region beginning after `line`. A production block ends
    /// where the next test region starts, whether or not the scanner recorded
    /// that region's declaration as an item.
    pub(crate) fn next_start_after(&self, line: usize) -> Option<usize> {
        self.spans
            .iter()
            .map(|&(start, _)| start)
            .find(|&start| start > line)
    }

    /// Lines outside every test region.
    pub(crate) fn non_test_loc(&self) -> usize {
        let covered: usize = self
            .spans
            .iter()
            .map(|&(start, end)| end.saturating_sub(start) + 1)
            .sum();
        self.total.saturating_sub(covered)
    }
}

/// Non-test LOC of one file — the `TestRegions` count exposed by the
/// `nontest-loc` verb and reused by atlas views.
pub(crate) fn non_test_loc(text: &str) -> usize {
    TestRegions::of(text).non_test_loc()
}

/// The file modules (`mod name;`) a file declares under a test-implying gate.
/// Their whole file is test code, which no single-file scan can know — the
/// atlas resolves these against the crate's file list.
pub(crate) fn test_module_declarations(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    (0..lines.len())
        .filter(|&index| gates_test(lines[index]))
        .filter_map(|index| declaration_after_attributes(&lines, index))
        .filter_map(|start| declares_file_module(lines[start]))
        .map(str::to_owned)
        .collect()
}

/// Whether a line is a top-level attribute whose `cfg` predicate implies
/// `test`. Column 0 keeps this to the file's own items, matching
/// [`Item::top_level`].
fn gates_test(line: &str) -> bool {
    line.strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
        .is_some_and(implies_test)
}

/// Whether a `cfg` predicate implies `test` — the condition under which the
/// gated item exists *only* in test builds. This reads the predicate's own
/// boolean semantics rather than its spelling, which is what distinguishes
/// `all(test, feature = "loom-tests")` (test-only) from `any(test, feature =
/// "test-strategies")` (also compiled for that feature, so production) and
/// from `not(all(test, ..))` (the negation, production). Both shapes are live
/// in the workspace.
fn implies_test(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if let Some(arguments) = strip_call(predicate, "all") {
        return split_top_level(arguments).into_iter().any(implies_test);
    }
    if let Some(arguments) = strip_call(predicate, "any") {
        let arguments = split_top_level(arguments);
        return arguments.iter().all(|argument| implies_test(argument));
    }
    predicate == "test"
}

/// The arguments of `name(..)`, or `None` when the predicate is not that call.
fn strip_call<'a>(predicate: &'a str, name: &str) -> Option<&'a str> {
    predicate
        .strip_prefix(name)?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

/// The first non-attribute line at or after `index`, skipping the attribute
/// stack (`#[cfg(test)] #[allow(..)] mod tests {`).
fn declaration_after_attributes(lines: &[&str], index: usize) -> Option<usize> {
    (index..lines.len()).find(|&next| !lines[next].starts_with("#["))
}

fn opens_inline_module(line: &str) -> bool {
    module_name(line).is_some_and(|rest| rest.trim_start().starts_with('{'))
}

fn declares_file_module(line: &str) -> Option<&str> {
    let rest = module_name(line)?;
    rest.trim_start().starts_with(';').then(|| {
        line[..line.len() - rest.len()]
            .trim_end()
            .rsplit(' ')
            .next()
            .unwrap_or_default()
    })
}

/// The text following `mod <name>` on a top-level module line, or `None` when
/// the line declares no module.
fn module_name(line: &str) -> Option<&str> {
    let (_, rest) = split_visibility(line);
    let after = keyword(rest, "mod")?;
    let end = after
        .find(|character: char| !character.is_alphanumeric() && character != '_')
        .unwrap_or(after.len());
    (end > 0).then(|| &after[end..])
}

/// The line closing a top-level block opened at `start` — under rustfmt's
/// column-0 discipline, the next bare `}`.
fn closing_line(lines: &[&str], start: usize) -> usize {
    (start + 1..lines.len())
        .find(|&index| lines[index] == "}")
        .unwrap_or(lines.len().saturating_sub(1))
}

/// The `//!` module-header lines (content after `//!`, trimmed). The first
/// entry is the module's orientation line; the joined block is the comment
/// audit's citation surface. Collected file-wide — inner docs of
/// nested inline modules are rare enough to leave to the heuristic.
pub(crate) fn module_header_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim_start().strip_prefix("//!"))
        .map(|rest| rest.trim().to_owned())
        .collect()
}

/// Scan Rust source text into its declared items, attributing them to `file`.
///
/// Doc attribution follows rustdoc's shape line-wise: a `///` run attaches to
/// the next declaration through any attributes, comments, and blank lines; any
/// other intervening line (a statement, an inner attribute) detaches it.
pub(crate) fn scan_source(text: &str, file: &Path) -> Vec<Item> {
    let lines: Vec<&str> = text.lines().collect();
    let mut items = Vec::new();
    let mut pending_doc: Option<String> = None;
    let mut attr_depth = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if attr_depth > 0 {
            attr_depth = next_attr_depth(attr_depth, trimmed);
            continue;
        }
        if let Some(doc) = trimmed.strip_prefix("///") {
            // First line of the run is the doc summary the atlas displays.
            pending_doc.get_or_insert_with(|| doc.trim().to_owned());
            continue;
        }
        if trimmed.starts_with("#[") {
            attr_depth = next_attr_depth(0, trimmed);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let Some(found) = classify_line(line) else {
            pending_doc = None;
            continue;
        };
        let declaration_block = match found.kind {
            ItemKind::Struct | ItemKind::Enum => braced_block(&lines, index),
            ItemKind::PubUse => semicolon_block(&lines, index),
            // Impl blocks capture the header only (through the opening
            // brace): bodies are excluded by design — signatures are the
            // intentional interface — and the methods inside are
            // scanned as their own (non-top-level) items.
            ItemKind::Fn | ItemKind::Impl => signature_block(&lines, index),
            _ => (*line).to_owned(),
        };
        items.push(Item {
            kind: found.kind,
            visibility: found.visibility,
            name: found.name.to_owned(),
            file: file.to_path_buf(),
            line: index + 1,
            declaration_block,
            doc: pending_doc.take(),
            top_level: !line.starts_with([' ', '\t']),
        });
    }
    items
}

/// Track how deep inside a (possibly multi-line) `#[...]` attribute the scan
/// is, by square-bracket counting. Textual by design — brackets inside string
/// literals can miscount, which only ever mis-attaches a doc summary.
fn next_attr_depth(depth: usize, line: &str) -> usize {
    line.chars().fold(depth, |depth, ch| match ch {
        '[' => depth + 1,
        ']' => depth.saturating_sub(1),
        _ => depth,
    })
}

/// Lines from the declaration to its matching closing brace (struct/enum
/// bodies), or to the terminating `;` for unit/tuple forms. Brace counting is
/// textual; an unterminated block degrades to the rest of the file.
fn braced_block(lines: &[&str], start: usize) -> String {
    let mut depth = 0usize;
    let mut opened = false;
    let mut end = start;
    'outer: for (offset, line) in lines[start..].iter().enumerate() {
        end = start + offset;
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => {
                    depth = depth.saturating_sub(1);
                    if opened && depth == 0 {
                        break 'outer;
                    }
                }
                ';' if !opened => break 'outer,
                _ => {}
            }
        }
    }
    lines[start..=end].join("\n")
}

/// Lines from the fn declaration through the first line carrying the
/// body-opening `{` (or a terminating `;` for trait-method signatures) — the
/// whole signature even when rustfmt breaks it across lines.
fn signature_block(lines: &[&str], start: usize) -> String {
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        end = start + offset;
        if line.contains('{') || line.contains(';') {
            break;
        }
    }
    lines[start..=end].join("\n")
}

fn semicolon_block(lines: &[&str], start: usize) -> String {
    let mut end = start;
    for (offset, line) in lines[start..].iter().enumerate() {
        end = start + offset;
        if line.contains(';') {
            break;
        }
    }
    lines[start..=end].join("\n")
}

/// One finite public name exposed by a captured `pub use` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReexportedName {
    pub(crate) source: String,
    pub(crate) source_path: String,
    pub(crate) exposed: String,
}

/// Extract exposed names from the scanner-owned `pub use` block. Nested groups
/// are flattened; globs remain explicit unresolved entries instead of
/// disappearing from downstream inventories.
pub(crate) fn reexported_names(declaration: &str) -> Vec<ReexportedName> {
    let Some((_, after_use)) = declaration.split_once("use ") else {
        return Vec::new();
    };
    let target = after_use.split(';').next().unwrap_or(after_use).trim();
    let mut names = BTreeMap::new();
    collect_reexported_names(target, "", &mut names);
    names
        .into_iter()
        .map(|(exposed, source_path)| ReexportedName {
            source: source_path
                .rsplit("::")
                .next()
                .unwrap_or(&source_path)
                .to_owned(),
            source_path,
            exposed,
        })
        .collect()
}

fn collect_reexported_names(tree: &str, parent: &str, names: &mut BTreeMap<String, String>) {
    let tree = tree.trim();
    let Some(open) = tree.find('{') else {
        collect_reexported_leaf(tree, parent, names);
        return;
    };
    let Some(close) = tree.rfind('}') else {
        return;
    };
    let prefix = qualified_path(parent, tree[..open].trim_end_matches("::").trim());
    for member in split_top_level(&tree[open + 1..close]) {
        let member = member.trim();
        if member == "self" {
            collect_reexported_leaf("self", &prefix, names);
        } else if !member.is_empty() {
            collect_reexported_names(member, &prefix, names);
        }
    }
}

fn collect_reexported_leaf(tree: &str, parent: &str, names: &mut BTreeMap<String, String>) {
    let tree = tree.trim();
    if tree.is_empty() {
        return;
    }
    if tree.ends_with("::*") || tree == "*" {
        let source_path = if tree == "*" {
            qualified_path(parent, "*")
        } else {
            qualified_path(parent, tree)
        };
        names.insert("*".to_owned(), source_path);
        return;
    }
    let (source, exposed) = tree
        .rsplit_once(" as ")
        .map_or((tree, tree), |(source, alias)| (source, alias));
    let source_path = if source == "self" {
        parent.to_owned()
    } else {
        qualified_path(parent, source)
    };
    let exposed = exposed.rsplit("::").next().unwrap_or(exposed).trim();
    if !source_path.is_empty() && !exposed.is_empty() {
        names.insert(exposed.to_owned(), source_path);
    }
}

fn qualified_path(parent: &str, child: &str) -> String {
    if parent.is_empty()
        || ["crate::", "self::", "super::", "::"]
            .iter()
            .any(|prefix| child.starts_with(prefix))
    {
        child.to_owned()
    } else if child.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}::{child}")
    }
}

/// Split on commas outside every bracket family and string literal — the one
/// argument splitter, shared by the struct/enum member scan and the `cfg`
/// predicate reader. Tuple variants (`Stop(u32, u32)`) and string arguments
/// (`feature = "a,b"`) therefore stay whole.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut start = 0usize;
    let mut parts = Vec::new();
    for (index, byte) in text.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b'{' | b'(' | b'[' if !quoted => depth += 1,
            b'}' | b')' | b']' if !quoted => depth = depth.saturating_sub(1),
            b',' if depth == 0 && !quoted => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Classify one source line as an item declaration — the grammar authority.
///
/// Handles fn qualifiers in any grammatical position (`pub const async unsafe
/// extern "C" fn`), distinguishing `const fn` (a fn) from `const NAME:` (a
/// const item) by lookahead — the keyword-order bug this extraction retires.
pub(crate) fn classify_line(line: &str) -> Option<LineItem<'_>> {
    let rest = line.trim_start();
    if rest.starts_with("//") || rest.starts_with("#[") {
        return None;
    }
    let (visibility, rest) = split_visibility(rest);

    if let Some(after) = rest.strip_prefix("macro_rules!") {
        return named(ItemKind::MacroRules, visibility, after.trim_start());
    }
    if let Some(after) = keyword(rest, "use") {
        if visibility == Visibility::Private {
            return None;
        }
        return Some(LineItem {
            kind: ItemKind::PubUse,
            visibility,
            name: use_target(after),
        });
    }

    // Fn qualifiers: `const? async? unsafe? extern "abi"?` before `fn`. A lone
    // `const` not followed by another qualifier or `fn` opens a const item.
    // `impl` is checked inside the loop so `unsafe impl Send for T` resolves.
    let mut rest = rest;
    loop {
        if let Some(after) = keyword(rest, "fn") {
            return named(ItemKind::Fn, visibility, after);
        }
        if let Some(after) = rest
            .strip_prefix("impl")
            .filter(|after| after.starts_with(char::is_whitespace) || after.starts_with('<'))
        {
            return Some(LineItem {
                kind: ItemKind::Impl,
                visibility,
                name: impl_target(after.trim_start()),
            });
        }
        if let Some(after) = keyword(rest, "async").or_else(|| keyword(rest, "unsafe")) {
            rest = after;
            continue;
        }
        if let Some(after) = keyword(rest, "extern") {
            rest = after
                .strip_prefix('"')
                .and_then(|abi| abi.split_once('"'))
                .map_or(after, |(_, tail)| tail.trim_start());
            continue;
        }
        if let Some(after) = keyword(rest, "const") {
            if ["fn", "async", "unsafe", "extern"]
                .iter()
                .any(|qualifier| keyword(after, qualifier).is_some())
            {
                rest = after;
                continue;
            }
            return named(ItemKind::Const, visibility, after);
        }
        break;
    }

    for (word, kind) in [
        ("struct", ItemKind::Struct),
        ("enum", ItemKind::Enum),
        ("trait", ItemKind::Trait),
        ("type", ItemKind::TypeAlias),
        ("mod", ItemKind::Mod),
        ("static", ItemKind::Static),
    ] {
        if let Some(after) = keyword(rest, word) {
            let after = if kind == ItemKind::Static {
                keyword(after, "mut").unwrap_or(after)
            } else {
                after
            };
            return named(kind, visibility, after);
        }
    }
    None
}

/// Strip a leading `word` when whitespace-delimited, returning the trimmed
/// remainder — the keyword boundary rule for the whole grammar.
fn keyword<'a>(rest: &'a str, word: &str) -> Option<&'a str> {
    let after = rest.strip_prefix(word)?;
    after
        .chars()
        .next()
        .is_some_and(char::is_whitespace)
        .then(|| after.trim_start())
}

fn named(kind: ItemKind, visibility: Visibility, rest: &str) -> Option<LineItem<'_>> {
    let raw = rest.strip_prefix("r#").unwrap_or(rest);
    let end = raw
        .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
        .unwrap_or(raw.len());
    let name = &raw[..end];
    (!name.is_empty()).then_some(LineItem {
        kind,
        visibility,
        name,
    })
}

/// The impl target's head type name: the type after `for` in a trait impl,
/// the subject type otherwise (`impl<T> Grade<T> for Tile<T>` → `Tile`).
/// Falls back to the trait when the target has no head identifier
/// (`impl Marker for (A, B)` → `Marker`), then to the literal `impl` — the
/// block still counts as a top-level item even when it cannot be named.
fn impl_target(after: &str) -> &str {
    let rest = skip_generics(after);
    let (trait_side, subject) = match split_top_level_for(rest) {
        Some((trait_side, subject)) => (Some(trait_side), subject),
        None => (None, rest),
    };
    type_head(subject)
        .or_else(|| trait_side.and_then(type_head))
        .unwrap_or("impl")
}

/// Skip a leading `<...>` generic-parameter list by angle-bracket counting.
/// `>` preceded by `-` is a return arrow inside a bound (`Fn() -> T`), not a
/// closer. Textual by design; an unterminated list degrades to empty.
fn skip_generics(rest: &str) -> &str {
    let Some(inner) = rest.strip_prefix('<') else {
        return rest;
    };
    let mut depth = 1usize;
    let mut previous = '<';
    for (index, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if previous != '-' => {
                depth -= 1;
                if depth == 0 {
                    return inner[index + 1..].trim_start();
                }
            }
            _ => {}
        }
        previous = ch;
    }
    ""
}

/// Split `Trait for Target` at the ` for ` outside any bracket nesting.
fn split_top_level_for(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut previous = b' ';
    for (index, &byte) in bytes.iter().enumerate() {
        if depth == 0 && text[index..].starts_with(" for ") {
            return Some((text[..index].trim_end(), text[index + 5..].trim_start()));
        }
        match byte {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' if previous != b'-' => depth = depth.saturating_sub(1),
            b')' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        previous = byte;
    }
    None
}

/// The head identifier of a type expression: strips `&`, lifetimes, `mut`,
/// and `dyn`, then takes the last path segment before generics
/// (`&mut crate::plan::Tile<T>` → `Tile`). `None` when the type has no head
/// identifier (tuples, slices, primitive literals like `[T; 4]`).
fn type_head(text: &str) -> Option<&str> {
    let mut rest = text.trim_start();
    loop {
        rest = rest.trim_start_matches('&').trim_start();
        if let Some(after) = rest.strip_prefix('\'') {
            let end = after
                .find(|ch: char| !(ch == '_' || ch.is_ascii_alphanumeric()))
                .unwrap_or(after.len());
            rest = after[end..].trim_start();
            continue;
        }
        if let Some(after) = keyword(rest, "mut").or_else(|| keyword(rest, "dyn")) {
            rest = after;
            continue;
        }
        break;
    }
    let end = rest
        .find(|ch: char| !(ch == ':' || ch == '_' || ch.is_ascii_alphanumeric()))
        .unwrap_or(rest.len());
    let name = rest[..end].rsplit("::").next().unwrap_or("");
    name.chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        .then_some(name)
}

fn split_visibility(rest: &str) -> (Visibility, &str) {
    let Some(after) = rest.strip_prefix("pub") else {
        return (Visibility::Private, rest);
    };
    if !after
        .chars()
        .next()
        .is_some_and(|ch| ch.is_whitespace() || ch == '(')
    {
        return (Visibility::Private, rest);
    }
    let after = after.trim_start();
    let Some(restricted) = after.strip_prefix('(') else {
        return (Visibility::Pub, after);
    };
    let Some((scope, tail)) = restricted.split_once(')') else {
        return (Visibility::Private, rest);
    };
    let visibility = match scope.trim() {
        "super" => Visibility::PubSuper,
        "self" => Visibility::Private,
        // `crate`; `pub(in path)` and friends are also crate-internal for
        // census purposes.
        _ => Visibility::PubCrate,
    };
    (visibility, tail.trim_start())
}

/// The re-exported name: the last path segment (or `as` alias) for a simple
/// `pub use a::b::C;`; the raw target text for grouped or glob re-exports.
fn use_target(after: &str) -> &str {
    let target = after.split(';').next().unwrap_or(after).trim_end();
    if target.contains(['{', '*']) {
        return target;
    }
    if let Some((_, alias)) = target.rsplit_once(" as ") {
        return alias.trim();
    }
    target.rsplit("::").next().unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(line: &str) -> LineItem<'_> {
        classify_line(line).expect("line declares an item")
    }

    #[test]
    fn fn_qualifiers_yield_the_fn_name() {
        // The keyword-order bug this module retires: `pub const fn` used to
        // resolve to the name "fn".
        assert_eq!(
            item("pub const fn cost_of(x: u32) -> u32 {").name,
            "cost_of"
        );
        assert_eq!(
            item("pub async fn fetch_block(&self) -> Block {").name,
            "fetch_block"
        );
        assert_eq!(
            item("pub unsafe fn from_raw(ptr: *const u8) -> Self {").name,
            "from_raw"
        );
        assert_eq!(
            item("pub const unsafe extern \"C\" fn trampoline() {").name,
            "trampoline"
        );
        assert_eq!(item("async fn local_probe() {").kind, ItemKind::Fn);
        // A lone `const` still opens a const item, not a fn.
        let constant = item("pub const MAX_LANES: usize = 4;");
        assert_eq!(
            (constant.kind, constant.name),
            (ItemKind::Const, "MAX_LANES")
        );
    }

    #[test]
    fn impl_blocks_are_named_by_their_target() {
        // Inherent, generic, trait, path-qualified, and unsafe impls all
        // resolve to the target type's head identifier.
        let cases = [
            ("impl Tile {", "Tile"),
            ("impl Tile<T> {", "Tile"),
            ("impl<T: Clone> Grade<T> for Tile<T> {", "Tile"),
            ("impl fmt::Display for LaneTile {", "LaneTile"),
            ("impl<'a> Iterator for Walker<'a> {", "Walker"),
            ("unsafe impl Send for Tile {}", "Tile"),
            ("impl Drop for Guard {", "Guard"),
            ("impl<F: Fn() -> usize> Probe<F> {", "Probe"),
            ("impl From<&str> for Name {", "Name"),
            // No head identifier on the target: fall back to the trait.
            ("impl Marker for (A, B) {", "Marker"),
            ("impl Marker for [T; 4] {", "Marker"),
        ];
        for (line, expected) in cases {
            let found = item(line);
            assert_eq!(
                (found.kind, found.name),
                (ItemKind::Impl, expected),
                "line: {line}"
            );
        }
        // `implement` is an identifier, not the `impl` keyword.
        assert_eq!(classify_line("implement stuff"), None);
    }

    #[test]
    fn impl_capture_is_top_level_with_a_signature_block() {
        let source = "\
pub struct Tile;

impl Grade for Tile
where
    Tile: Clone,
{
    /// Method doc.
    pub fn cost(&self) -> Cost {
        Cost::ZERO
    }
}
";
        let items = scan_source(source, Path::new("fixture.rs"));
        let imp = items
            .iter()
            .find(|item| item.kind == ItemKind::Impl)
            .expect("impl captured");
        assert_eq!(
            (imp.name.as_str(), imp.line, imp.top_level),
            ("Tile", 3, true)
        );
        // Header only, through the body-opening brace: the where clause is in,
        // the method body is out.
        assert!(imp.declaration_block.contains("Tile: Clone"));
        assert!(!imp.declaration_block.contains("cost"));
        // The method inside is scanned as its own item, but not top-level.
        let method = items
            .iter()
            .find(|item| item.name == "cost")
            .expect("method scanned");
        assert_eq!((method.kind, method.top_level), (ItemKind::Fn, false));
        let tile = items
            .iter()
            .find(|item| item.kind == ItemKind::Struct)
            .expect("struct scanned");
        assert!(tile.top_level);
    }

    #[test]
    fn visibility_ladder_is_recognized() {
        assert_eq!(item("pub fn a() {}").visibility, Visibility::Pub);
        assert_eq!(
            item("pub(crate) fn b() {}").visibility,
            Visibility::PubCrate
        );
        assert_eq!(
            item("pub(super) fn c() {}").visibility,
            Visibility::PubSuper
        );
        assert_eq!(item("fn d() {}").visibility, Visibility::Private);
        assert_eq!(item("pub(self) fn e() {}").visibility, Visibility::Private);
    }

    #[test]
    fn reexports_and_macros_are_recognized() {
        let single = item("pub use crate::grade::Cost;");
        assert_eq!((single.kind, single.name), (ItemKind::PubUse, "Cost"));
        let grouped = item("pub use grade::{Cost, Grade};");
        assert_eq!(
            (grouped.kind, grouped.name),
            (ItemKind::PubUse, "grade::{Cost, Grade}")
        );
        // A private import is not an item.
        assert_eq!(classify_line("use std::fmt;"), None);
        let rules = item("macro_rules! grade_table {");
        assert_eq!(
            (rules.kind, rules.name),
            (ItemKind::MacroRules, "grade_table")
        );
    }

    #[test]
    fn multiline_nested_reexports_share_one_captured_grammar() {
        let text = "\
pub use crate::measurement::{
    block::{BlockFrame, BlockRefusal as Refusal},
    Floor,
    hidden::*,
};
";
        let items = scan_source(text, Path::new("src/lib.rs"));
        let export = items.first().expect("multiline re-export is captured");
        assert_eq!(export.kind, ItemKind::PubUse);
        assert_eq!(export.declaration_block, text.trim_end());
        assert_eq!(
            reexported_names(&export.declaration_block),
            vec![
                ReexportedName {
                    source: "*".to_owned(),
                    source_path: "crate::measurement::hidden::*".to_owned(),
                    exposed: "*".to_owned(),
                },
                ReexportedName {
                    source: "BlockFrame".to_owned(),
                    source_path: "crate::measurement::block::BlockFrame".to_owned(),
                    exposed: "BlockFrame".to_owned(),
                },
                ReexportedName {
                    source: "Floor".to_owned(),
                    source_path: "crate::measurement::Floor".to_owned(),
                    exposed: "Floor".to_owned(),
                },
                ReexportedName {
                    source: "BlockRefusal".to_owned(),
                    source_path: "crate::measurement::block::BlockRefusal".to_owned(),
                    exposed: "Refusal".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn struct_blocks_capture_to_the_closing_brace() {
        let source = "\
pub struct Tile {
    lanes: Vec<LaneId>,
    cost: Cost,
}

pub struct Marker;

pub enum Verdict {
    Admitted(Tile),
    Refused { reason: String },
}

pub fn admit(
    candidate: Candidate,
) -> Verdict {
    Verdict::Admitted(Tile::default())
}
";
        let items = scan_source(source, Path::new("realize/admit.rs"));
        let by_name = |name: &str| {
            items
                .iter()
                .find(|item| item.name == name)
                .expect("scanned item present")
        };

        let tile = by_name("Tile");
        assert_eq!((tile.kind, tile.line), (ItemKind::Struct, 1));
        assert!(tile.declaration_block.ends_with('}'));
        assert!(tile.declaration_block.contains("lanes: Vec<LaneId>"));

        let marker = by_name("Marker");
        assert_eq!(marker.declaration_block, "pub struct Marker;");

        let verdict = by_name("Verdict");
        assert_eq!(verdict.kind, ItemKind::Enum);
        assert!(
            verdict
                .declaration_block
                .contains("Refused { reason: String }")
        );

        // Multi-line fn signatures capture through the body-opening brace and
        // exclude the body.
        let admit = by_name("admit");
        assert!(admit.declaration_block.contains("-> Verdict {"));
        assert!(!admit.declaration_block.contains("Tile::default"));
    }

    #[test]
    fn doc_summaries_attach_through_attributes() {
        let source = "\
//! Module header line.
//! Second header line.

/// Summary line of Tile.
///
/// Detail the atlas never shows.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = \"multi-line attribute between doc and item\"
)]
pub struct Tile;

/// Detached by an intervening statement.
static_assertions_placeholder();

pub fn undocumented() {}

/// Doc on a re-export.
pub use crate::grade::Cost;
";
        let items = scan_source(source, Path::new("fixture.rs"));
        let doc_of = |name: &str| {
            items
                .iter()
                .find(|item| item.name == name)
                .expect("scanned item present")
                .doc
                .clone()
        };
        assert_eq!(doc_of("Tile").as_deref(), Some("Summary line of Tile."));
        assert_eq!(doc_of("undocumented"), None);
        assert_eq!(doc_of("Cost").as_deref(), Some("Doc on a re-export."));
        assert_eq!(
            module_header_lines(source),
            vec!["Module header line.", "Second header line."]
        );
        assert!(module_header_lines("pub fn a() {}\n").is_empty());
    }

    #[test]
    fn test_regions_cover_every_gated_module_not_only_the_last() {
        // Regression: production code between two test modules remains counted.
        // A single terminal boundary counted the interleaved module — and
        // everything after it — as production.
        let source = "\
pub fn production_a() {}

#[cfg(all(test, feature = \"loom-tests\"))]
mod loom_tests {
    fn inner() {}
}

pub fn production_b() {}

#[cfg(test)]
mod tests {
    use super::*;
}
";
        let regions = TestRegions::of(source);
        assert!(regions.covers(4), "the loom module is test code");
        assert!(!regions.covers(8), "production_b is not");
        assert!(regions.covers(11), "the terminal module is test code");
        // 13 lines, 4 in each region: the two declarations, the blank lines
        // around them, and the two production fns remain.
        assert_eq!(regions.non_test_loc(), 5);
    }

    #[test]
    fn cfg_gates_are_read_as_predicates_not_spellings() {
        // Every shape below is live in the workspace.
        assert!(implies_test("test"));
        assert!(implies_test("all(test, feature = \"loom-tests\")"));
        assert!(implies_test("all(feature = \"gpu\", test)"));
        // `any` also compiles without test, and `not` is the negation: both
        // are production code that merely mentions the word.
        assert!(!implies_test("any(test, feature = \"test-strategies\")"));
        assert!(!implies_test("not(all(test, feature = \"loom-tests\"))"));
        assert!(!implies_test("not(test)"));
        assert!(!implies_test("feature = \"testing\""));
    }

    #[test]
    fn test_module_declarations_name_whole_test_files() {
        let source = "\
#[cfg(test)]
mod finite_interior_tests;

pub(crate) mod production;

#[cfg(test)]
mod tests {
}
";
        assert_eq!(
            test_module_declarations(source),
            vec!["finite_interior_tests".to_owned()]
        );
    }

    #[test]
    fn split_top_level_keeps_bracketed_and_quoted_commas_whole() {
        assert_eq!(
            split_top_level("Stop(u32, u32), Go"),
            vec!["Stop(u32, u32)", " Go"]
        );
        assert_eq!(
            split_top_level("feature = \"a,b\", test"),
            vec!["feature = \"a,b\"", " test"]
        );
    }

    #[test]
    fn non_test_boundary_keeps_gated_helpers_as_production() {
        // The replicate.rs regression class: an early test-only helper must
        // not truncate the count — gating one item is a property of that
        // item, not a boundary.
        let source = "\
pub fn production_a() {}

#[cfg(test)]
fn test_only_helper() {}

pub fn production_b() {}

#[cfg(test)]
mod tests {
    use super::*;
}
";
        assert_eq!(non_test_loc(source), 7);
        // No boundary: the whole file counts.
        assert_eq!(non_test_loc("fn a() {}\nfn b() {}\n"), 2);
    }
}
