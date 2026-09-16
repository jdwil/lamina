//! The recursive, provenance-ready renderer.
//!
//! Rendering is reduced to its simplest form and applied recursively: a
//! [`Template`] is a sequence of literal text and named `{slot}` holes; each
//! slot is filled by recursively rendering something else, threading a
//! [`RenderContext`](crate::predicate::RenderContext) of inherited facts down so
//! that [`When`](crate::predicate) predicates on child nodes can query the
//! caller (e.g. `caller is async`).
//!
//! Every rendered fragment is a [`Rendered`], which carries its text plus
//! *optional* provenance (which source construct produced it). No provenance is
//! populated yet, but the type is provenance-ready so a source map — needed to
//! map raised prose back to source during review — can be layered on later
//! without reworking the renderer.

/// A rendered fragment: text plus optional provenance.
///
/// Provenance is deliberately open-ended for now (a label naming the producing
/// construct). When raising and source maps are built, this grows into a proper
/// span mapping; today it exists so the plumbing does not have to change then.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    /// The emitted text.
    pub text: String,
    /// Optional provenance segments, in output order. Each records a byte range
    /// within `text` and a label for the source construct that produced it.
    pub provenance: Vec<Provenance>,
}

/// A provenance segment: a byte range of rendered output and its source label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Start byte offset within [`Rendered::text`].
    pub start: usize,
    /// End byte offset within [`Rendered::text`].
    pub end: usize,
    /// A label naming the source construct (e.g. `"fn:answer"`). Free-form for
    /// now.
    pub label: String,
}

impl Rendered {
    /// An empty rendered fragment.
    pub fn empty() -> Self {
        Rendered::default()
    }

    /// A rendered fragment from plain text with no provenance.
    pub fn text(text: impl Into<String>) -> Self {
        Rendered {
            text: text.into(),
            provenance: Vec::new(),
        }
    }

    /// Appends another fragment, shifting its provenance ranges to account for
    /// the current length so offsets remain valid in the combined text.
    pub fn push(&mut self, other: Rendered) {
        let base = self.text.len();
        self.text.push_str(&other.text);
        for mut p in other.provenance {
            p.start += base;
            p.end += base;
            self.provenance.push(p);
        }
    }

    /// Appends literal text with no provenance.
    pub fn push_str(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Wraps the entire current fragment in a single provenance segment with
    /// the given label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.provenance.insert(
            0,
            Provenance {
                start: 0,
                end: self.text.len(),
                label: label.into(),
            },
        );
        self
    }
}

/// A parsed template: a sequence of literal chunks and named slot holes.
///
/// Written with `{slot}` holes and `{{` / `}}` for literal braces (mirroring
/// Rust's format-string convention), so target code containing braces (a Rust
/// function body `{ ... }`) is expressible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<TemplatePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplatePart {
    /// Literal text emitted verbatim.
    Literal(String),
    /// A named slot to be filled by a [`SlotResolver`].
    Slot(String),
}

/// An error while parsing a template string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// A `{` was not closed by a `}`.
    UnclosedSlot,
    /// A `}` appeared without a matching `{` (and was not doubled `}}`).
    UnmatchedBrace,
    /// A slot name was empty (`{}`).
    EmptySlot,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::UnclosedSlot => write!(f, "unclosed `{{` in template"),
            TemplateError::UnmatchedBrace => write!(f, "unmatched `}}` in template"),
            TemplateError::EmptySlot => write!(f, "empty slot `{{}}` in template"),
        }
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    /// Parses a template string into literal chunks and slot holes.
    ///
    /// `{{` and `}}` are literal braces; `{name}` is a slot.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] on an unclosed slot, unmatched brace, or empty
    /// slot name.
    pub fn parse(src: &str) -> Result<Template, TemplateError> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = src.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        literal.push('{');
                        continue;
                    }
                    // Start of a slot.
                    if !literal.is_empty() {
                        parts.push(TemplatePart::Literal(std::mem::take(&mut literal)));
                    }
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(ch) => name.push(ch),
                            None => return Err(TemplateError::UnclosedSlot),
                        }
                    }
                    if name.is_empty() {
                        return Err(TemplateError::EmptySlot);
                    }
                    parts.push(TemplatePart::Slot(name));
                }
                '}' => {
                    if chars.peek() == Some(&'}') {
                        chars.next();
                        literal.push('}');
                    } else {
                        return Err(TemplateError::UnmatchedBrace);
                    }
                }
                other => literal.push(other),
            }
        }
        if !literal.is_empty() {
            parts.push(TemplatePart::Literal(literal));
        }
        Ok(Template { parts })
    }

    /// The names of the slots this template references, in order (duplicates
    /// included). Useful for validation.
    pub fn slot_names(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Slot(name) => Some(name.as_str()),
                TemplatePart::Literal(_) => None,
            })
            .collect()
    }

    /// Renders this template by filling each slot via `resolver`, concatenating
    /// literals and resolved slots into a single [`Rendered`] fragment.
    ///
    /// The resolver is where recursion happens: filling a slot typically renders
    /// a child construct (with its own context and possibly its own templates).
    ///
    /// **Indentation is line-indent–derived.** A slot's indent is the
    /// *indentation* (leading-whitespace width) of the line on which its
    /// `{slot}` appears. When the slot's rendered value spans multiple lines,
    /// that indent is prepended to every line *after the first* (the first
    /// line's text is already positioned by the literal text preceding the
    /// slot). Blank lines are left untouched (no trailing whitespace). This
    /// makes nested indentation accumulate naturally through the recursion:
    /// templates are authored at column 0 and inherit indentation purely from
    /// where they are placed by their caller. Because it is the line's
    /// *indentation* (not its full character column), a multi-line slot placed
    /// after non-whitespace text on a line (e.g. an `else` arm) stays aligned to
    /// that line's indent rather than being pushed out by the preceding text.
    ///
    /// # Errors
    ///
    /// Propagates any error the resolver returns for a slot.
    pub fn render<R: SlotResolver>(&self, resolver: &mut R) -> Result<Rendered, R::Error> {
        let mut out = Rendered::empty();
        for part in &self.parts {
            match part {
                TemplatePart::Literal(text) => out.push_str(text),
                TemplatePart::Slot(name) => {
                    let indent = current_column(&out.text);
                    let fragment = resolver.resolve(name)?;
                    out.push(indent_continuation_lines(fragment, indent));
                }
            }
        }
        Ok(out)
    }
}

/// The **indentation** of the current (last, incomplete) output line: the width
/// of its leading whitespace.
///
/// This is deliberately the leading-whitespace width, NOT the full character
/// column, matching the language-definition format contract: continuation lines
/// of a multi-line slot value inherit *the current line's indentation*, so a
/// nested block placed after non-whitespace text (e.g. an `else` arm authored as
/// `}} else {else}`) is left-aligned to that line's indent rather than pushed
/// out by the width of the `}} else ` prefix. When the slot begins a line (only
/// whitespace precedes it — the overwhelmingly common case), the leading-
/// whitespace width equals the column, so this is a no-op there.
fn current_column(text: &str) -> usize {
    let line = match text.rfind('\n') {
        Some(idx) => &text[idx + 1..],
        None => text,
    };
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Prepends `indent` spaces to every line of `fragment` *after the first*,
/// leaving blank lines untouched. Returns the fragment unchanged when `indent`
/// is zero or the fragment is single-line. Provenance offsets are recomputed to
/// account for the inserted whitespace.
fn indent_continuation_lines(fragment: Rendered, indent: usize) -> Rendered {
    if indent == 0 || !fragment.text.contains('\n') {
        return fragment;
    }
    let pad: String = " ".repeat(indent);
    let mut new_text = String::with_capacity(fragment.text.len());
    // Map old byte offset -> new byte offset, so provenance can be shifted.
    let mut offset_map: Vec<(usize, usize)> = Vec::new();

    for (i, line) in fragment.text.split_inclusive('\n').enumerate() {
        let old_start = offset_of_line(&fragment.text, i);
        offset_map.push((old_start, new_text.len()));
        if i > 0 {
            // Only indent non-blank lines (a line that is just "\n" or "").
            let line_body = line.strip_suffix('\n').unwrap_or(line);
            if !line_body.is_empty() {
                new_text.push_str(&pad);
            }
        }
        new_text.push_str(line);
    }

    // Recompute provenance offsets against the padded text.
    let provenance = fragment
        .provenance
        .into_iter()
        .map(|p| {
            let start = shift_offset(&offset_map, p.start, &fragment.text, indent);
            let end = shift_offset(&offset_map, p.end, &fragment.text, indent);
            Provenance {
                start,
                end,
                label: p.label,
            }
        })
        .collect();

    Rendered {
        text: new_text,
        provenance,
    }
}

/// Byte offset where the `n`th line (0-indexed) begins in `text`.
fn offset_of_line(text: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut count = 0;
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            count += 1;
            if count == n {
                return idx + 1;
            }
        }
    }
    text.len()
}

/// Shifts an old byte offset into the padded text by adding `indent` for each
/// non-first, non-blank line that begins at or before `old`.
fn shift_offset(offset_map: &[(usize, usize)], old: usize, text: &str, indent: usize) -> usize {
    // Count how many pads were inserted before `old`.
    let mut added = 0;
    for (line_idx, &(old_line_start, _)) in offset_map.iter().enumerate() {
        if line_idx == 0 {
            continue;
        }
        if old_line_start <= old {
            // A pad was added at this line if the line is non-blank.
            let line_end = offset_map
                .get(line_idx + 1)
                .map(|&(s, _)| s)
                .unwrap_or(text.len());
            let body = &text[old_line_start..line_end];
            let body = body.strip_suffix('\n').unwrap_or(body);
            if !body.is_empty() {
                added += indent;
            }
        } else {
            break;
        }
    }
    old + added
}

/// Fills template slots. Implementors decide what each named slot renders to,
/// which is where recursion into child constructs occurs.
pub trait SlotResolver {
    /// The error type a resolver may produce.
    type Error;

    /// Renders the slot named `name` into a [`Rendered`] fragment.
    fn resolve(&mut self, name: &str) -> Result<Rendered, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MapResolver(std::collections::HashMap<&'static str, &'static str>);

    impl SlotResolver for MapResolver {
        type Error = String;
        fn resolve(&mut self, name: &str) -> Result<Rendered, String> {
            self.0
                .get(name)
                .map(|s| Rendered::text(*s))
                .ok_or_else(|| format!("no slot {name}"))
        }
    }

    fn resolver(pairs: &[(&'static str, &'static str)]) -> MapResolver {
        MapResolver(pairs.iter().copied().collect())
    }

    #[test]
    fn parses_and_renders_slots() {
        let t = Template::parse("fn {name}({params}) {body}").expect("parse");
        let mut r = resolver(&[("name", "answer"), ("params", ""), ("body", "{ 1 }")]);
        // Note: body literal here is provided by resolver, not the template, so
        // no brace-escaping needed in this case.
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "fn answer() { 1 }");
    }

    #[test]
    fn literal_braces_via_doubling() {
        let t = Template::parse("fn {name}() {{\n{stmts}\n}}").expect("parse");
        let mut r = resolver(&[("name", "a"), ("stmts", "    return 1;")]);
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "fn a() {\n    return 1;\n}");
    }

    #[test]
    fn slot_names_listed() {
        let t = Template::parse("{export}fn {name}({params}){ret} {body}").expect("parse");
        assert_eq!(t.slot_names(), vec!["export", "name", "params", "ret", "body"]);
    }

    #[test]
    fn rejects_unclosed_slot() {
        assert_eq!(Template::parse("fn {name"), Err(TemplateError::UnclosedSlot));
    }

    #[test]
    fn rejects_empty_slot() {
        assert_eq!(Template::parse("fn {}"), Err(TemplateError::EmptySlot));
    }

    #[test]
    fn rejects_unmatched_brace() {
        assert_eq!(Template::parse("fn a}"), Err(TemplateError::UnmatchedBrace));
    }

    #[test]
    fn provenance_offsets_shift_on_push() {
        let mut a = Rendered::text("pub ");
        a.push(Rendered::text("fn").label("kw"));
        // The "fn" label should now point at bytes 4..6 of "pub fn".
        assert_eq!(a.text, "pub fn");
        assert_eq!(a.provenance.len(), 1);
        assert_eq!(a.provenance[0].start, 4);
        assert_eq!(a.provenance[0].end, 6);
        assert_eq!(a.provenance[0].label, "kw");
    }

    #[test]
    fn indents_multiline_slot_by_column() {
        // `{body}` sits at column 4; a two-line body must have its SECOND line
        // indented 4 spaces (the first line's indent is the literal "    ").
        let t = Template::parse("fn a() {{\n    {body}\n}}").expect("parse");
        let mut r = resolver(&[("body", "return 1;\nreturn 2;")]);
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "fn a() {\n    return 1;\n    return 2;\n}");
    }

    #[test]
    fn multiline_slot_after_nonwhitespace_uses_line_indent_not_column() {
        // A multi-line slot placed AFTER non-whitespace text on a line (e.g. an
        // `else` arm authored as `}} else {else}`) must indent its continuation
        // lines by the LINE'S indentation (here 0), not the character column
        // (here 7, after "} else "). This keeps the else block left-aligned to
        // the `if`.
        let t = Template::parse("if c {{\n    {then}\n}} else {else}").expect("parse");
        let mut r = resolver(&[("then", "a;"), ("else", "{\n    b;\n}")]);
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "if c {\n    a;\n} else {\n    b;\n}");
    }

    #[test]
    fn single_line_slot_is_unchanged() {
        let t = Template::parse("fn a() {{\n    {body}\n}}").expect("parse");
        let mut r = resolver(&[("body", "return 1;")]);
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "fn a() {\n    return 1;\n}");
    }

    #[test]
    fn blank_lines_in_slot_get_no_trailing_whitespace() {
        let t = Template::parse("x {{\n    {body}\n}}").expect("parse");
        let mut r = resolver(&[("body", "a\n\nb")]);
        let out = t.render(&mut r).expect("render");
        // The blank middle line must stay empty, not "    ".
        assert_eq!(out.text, "x {\n    a\n\n    b\n}");
    }

    #[test]
    fn zero_column_slot_no_indent() {
        let t = Template::parse("{body}").expect("parse");
        let mut r = resolver(&[("body", "a\nb")]);
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "a\nb");
    }
}
