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
    Slot(SlotRef),
}

/// A parsed slot reference: the raw slot name (which may itself contain the
/// existing projection `:` syntax and closed-helper `(...)` calls, unchanged)
/// plus any NEW caller-supplied named arguments.
///
/// A bare `{name}` (or the existing `{name:item}` / `{helper(x)}` forms) parses
/// to a `SlotRef` with an EMPTY `args` list and is resolved BYTE-IDENTICALLY to
/// before (the backward-compat guarantee): the whole reference text is handed to
/// [`SlotResolver::resolve`] exactly as it always was.
///
/// A `{name(argname: value, ...)}` reference (NEW) captures each `value` as a
/// nested [`Template`] rendered in the CALLER's scope; the rendered fragments
/// are pushed as named args for the duration of resolving `name` and its nested
/// sub-renders (see [`Template::render`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SlotRef {
    /// The slot name as handed to the resolver (may contain `:` projection and
    /// closed-helper `(...)` calls — those are the resolver's concern, not the
    /// template parser's). For a no-arg reference this is the entire text
    /// between the braces.
    name: String,
    /// Caller-supplied named arguments, each an `(argname, value-template)`
    /// pair. Empty for every existing (no-arg) slot form.
    args: Vec<(String, Template)>,
}

/// A read-only, borrowed view of a single `SlotRef` for the load-time
/// slot-graph validator.
///
/// `SlotRef` itself is a private parser type; this view exposes exactly the two
/// things the validator needs — the resolver-facing base `name` (which may
/// still carry the existing `:` projection or a closed-helper `(...)` call, the
/// resolver's concern) and the caller-supplied `args`. Each arg is an
/// `(argname, value-template)` pair: the value-template is rendered in the
/// caller's scope at render time, so the validator validates the slots IT
/// references in the caller's current scope, and the argname is treated as
/// satisfied-by-injection within the referenced subsection's sub-graph.
///
/// Exposed via [`Template::slot_refs`]; [`Template::slot_names`] and its callers
/// are unchanged.
#[derive(Debug, Clone, Copy)]
pub struct SlotRefView<'a> {
    /// The slot name as handed to the resolver (may contain `:` projection and
    /// closed-helper `(...)` calls).
    pub name: &'a str,
    /// The caller-supplied named arguments (empty for a no-arg reference).
    pub args: &'a [(String, Template)],
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
    /// A slot-argument list `{name(...)}` was malformed: an arg had no `name:`
    /// label, an empty arg name, or an unbalanced/empty parenthesis group.
    MalformedSlotArgs(String),
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateError::UnclosedSlot => write!(f, "unclosed `{{` in template"),
            TemplateError::UnmatchedBrace => write!(f, "unmatched `}}` in template"),
            TemplateError::EmptySlot => write!(f, "empty slot `{{}}` in template"),
            TemplateError::MalformedSlotArgs(msg) => {
                write!(f, "malformed slot arguments: {msg}")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl Template {
    /// Parses a template string into literal chunks and slot holes.
    ///
    /// `{{` and `}}` are literal braces; `{name}` is a slot.
    ///
    /// A slot may carry NEW caller-supplied named arguments:
    /// `{name(argname: value, ...)}`, where each `value` is itself a template
    /// fragment (it MAY contain nested `{...}` slots, which are resolved in the
    /// CALLER's scope when the slot is rendered). The parser balances braces and
    /// parentheses inside the argument list so nested `{...}` are captured
    /// whole. A slot with NO argument list (`{name}`, `{name:item}`, or a closed
    /// helper call like `{escape(x, c)}`) parses byte-identically to before —
    /// the entire inner text becomes the slot name and no args are attached.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] on an unclosed slot, unmatched brace, empty
    /// slot name, or a malformed argument list.
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
                    // Capture the raw slot body up to the matching `}`, honoring
                    // nested `{...}` and `(...)` so a slot-argument value that
                    // itself contains `{sub}` (e.g. `{type(name: {name})}`) is
                    // read whole rather than terminating at the inner `}`.
                    let body = capture_slot_body(&mut chars)?;
                    if body.is_empty() {
                        return Err(TemplateError::EmptySlot);
                    }
                    parts.push(TemplatePart::Slot(parse_slot_ref(&body)?));
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
    /// included). Useful for validation. The name is the resolver-facing slot
    /// name (excluding any NEW argument list); argument *values* are nested
    /// templates and are not surfaced here.
    pub fn slot_names(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Slot(slot) => Some(slot.name.as_str()),
                TemplatePart::Literal(_) => None,
            })
            .collect()
    }

    /// The **structured** slot references this template makes, in order
    /// (duplicates included). Unlike [`slot_names`](Template::slot_names) — which
    /// surfaces only the resolver-facing base name — this also exposes each
    /// reference's caller-supplied named arguments (`argname` + the nested
    /// value-template rendered in the caller's scope).
    ///
    /// This is a **new, non-breaking** accessor added for the argument-aware
    /// load-time slot-graph validator: `slot_names()` and all of its existing
    /// callers are left byte-identical (a def with no arg-bearing references
    /// yields the same base names either way). The validator uses `slot_refs()`
    /// so it can (a) validate each arg *value* template in the caller's scope
    /// and (b) treat the injected argnames as satisfied within the callee's
    /// sub-graph. See [`SlotRefView`].
    pub fn slot_refs(&self) -> Vec<SlotRefView<'_>> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                TemplatePart::Slot(slot) => Some(SlotRefView {
                    name: slot.name.as_str(),
                    args: &slot.args,
                }),
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
                TemplatePart::Slot(slot) => {
                    let indent = current_column(&out.text);
                    let fragment = self.resolve_slot(slot, resolver)?;
                    out.push(indent_continuation_lines(fragment, indent));
                }
            }
        }
        Ok(out)
    }

    /// Resolves a single slot reference, handling caller-supplied named
    /// arguments.
    ///
    /// Resolution order:
    /// 1. **Argument shadowing (no-arg reference):** a bare `{argname}` whose
    ///    name matches an argument currently in scope resolves to that pushed
    ///    argument value, SHADOWING any same-named normal slot. This is what
    ///    makes a passed arg reachable (`{name}` inside the C declarator type)
    ///    and lets it shadow a normal slot of the same name.
    /// 2. **Argument-bearing reference (`{name(a: v, ...)}`):** each `v` is
    ///    rendered NOW, in the CALLER's scope (`self` + `resolver`), producing a
    ///    fragment. Those fragments are pushed as named args, `name` is resolved
    ///    (its sub-renders see the args, and may shadow them at a deeper level),
    ///    then the args are popped. Args AUGMENT, never replace, the callee's
    ///    normal scope.
    /// 3. **Plain reference:** handed to [`SlotResolver::resolve`] unchanged
    ///    (byte-identical to the pre-argument behavior).
    fn resolve_slot<R: SlotResolver>(
        &self,
        slot: &SlotRef,
        resolver: &mut R,
    ) -> Result<Rendered, R::Error> {
        // (1) A no-arg reference that names an in-scope pushed argument resolves
        // to that argument, shadowing a normal slot of the same name. Only bare
        // references (no `:` projection, no `(...)` helper/args) can be argument
        // names, so this never intercepts projections or helper calls.
        if slot.args.is_empty() && is_bare_identifier(&slot.name) {
            if let Some(value) = resolver.resolve_arg(&slot.name) {
                return Ok(value);
            }
        }

        // (3) Plain reference (no args): unchanged path.
        if slot.args.is_empty() {
            return resolver.resolve(&slot.name);
        }

        // (2) Argument-bearing reference: render each value in the caller's
        // scope, push, resolve, pop (pop even on error).
        let mut rendered_args = Vec::with_capacity(slot.args.len());
        for (arg_name, value_template) in &slot.args {
            let value = value_template.render(resolver)?;
            rendered_args.push((arg_name.clone(), value));
        }
        resolver.push_args(rendered_args);
        let result = resolver.resolve(&slot.name);
        resolver.pop_args();
        result
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

/// Reads the raw body of a slot from `chars`, having already consumed the
/// opening `{`, up to and consuming the matching closing `}`.
///
/// Braces and parentheses are BALANCED: a `}` only terminates the slot at
/// brace-depth 0, so a slot-argument value that itself contains a nested
/// `{sub}` (e.g. `{type(name: {name})}`) is captured whole. Parentheses are
/// tracked too so a `}` inside a `(...)` group is treated as literal body text.
/// This is strictly more permissive than the old "read to first `}`" scan and
/// is byte-compatible for every existing slot form (which contain no nested
/// braces).
///
/// # Errors
///
/// [`TemplateError::UnclosedSlot`] if the input ends before the matching `}`.
fn capture_slot_body(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, TemplateError> {
    let mut body = String::new();
    let mut brace_depth: usize = 0;
    let mut paren_depth: usize = 0;
    loop {
        match chars.next() {
            None => return Err(TemplateError::UnclosedSlot),
            Some('}') if brace_depth == 0 => return Ok(body),
            Some('}') => {
                brace_depth -= 1;
                body.push('}');
            }
            Some('{') => {
                brace_depth += 1;
                body.push('{');
            }
            Some('(') => {
                paren_depth += 1;
                body.push('(');
            }
            Some(')') => {
                paren_depth = paren_depth.saturating_sub(1);
                body.push(')');
            }
            Some(ch) => body.push(ch),
        }
    }
}

/// Parses a captured slot body into a [`SlotRef`].
///
/// The body is EITHER the existing no-arg form (a plain name, a `name:item`
/// projection, or a closed helper call such as `escape(x, c)`) — in which case
/// the whole body becomes the resolver-facing name with no args — OR the NEW
/// argument form `name(argname: value, ...)`.
///
/// The two are distinguished structurally: the argument form has a
/// parenthesized tail whose top-level content parses as one or more
/// `argname: value` pairs (an identifier, then `:`, then a value). A closed
/// helper call like `escape(x, c)` has NO `:` in its argument list, so it is
/// left as a plain name and handed to the resolver unchanged (backward compat).
///
/// # Errors
///
/// [`TemplateError::MalformedSlotArgs`] if a `(...)` tail looks like the
/// argument form (contains a top-level `:`) but is malformed (empty arg name,
/// missing value, unbalanced parens, or a trailing/empty segment).
fn parse_slot_ref(body: &str) -> Result<SlotRef, TemplateError> {
    // Find a top-level `(...)` tail: the LAST `(` at paren-depth 0 whose group
    // extends to the end of the body. Only a trailing, fully-balanced `(...)`
    // group is an argument list.
    let Some(open) = find_arg_paren(body) else {
        return Ok(SlotRef {
            name: body.to_string(),
            args: Vec::new(),
        });
    };
    let inner = &body[open + 1..body.len() - 1];
    // The argument form is recognized only when the parenthesized content has a
    // top-level `argname:` label. Otherwise it is a closed helper call (or some
    // other `(...)` the resolver understands) and is left untouched.
    if !has_top_level_colon(inner) {
        return Ok(SlotRef {
            name: body.to_string(),
            args: Vec::new(),
        });
    }
    let name = body[..open].to_string();
    if name.is_empty() {
        return Err(TemplateError::MalformedSlotArgs(
            "slot name before `(` is empty".to_string(),
        ));
    }
    let args = parse_slot_args(inner)?;
    Ok(SlotRef { name, args })
}

/// Locates the opening index of a trailing, balanced top-level `(...)` group
/// that spans to the end of `body`, or `None` if the body has no such tail.
fn find_arg_paren(body: &str) -> Option<usize> {
    if !body.ends_with(')') {
        return None;
    }
    // Walk backward, matching the trailing `)` to its `(`.
    let bytes = body.as_bytes();
    let mut depth: usize = 0;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether `s` contains a `:` outside any nested `{...}` or `(...)` group — the
/// marker that a parenthesized tail is an argument list rather than a closed
/// helper call. Nested-group `:`s (inside an arg value's `{sub:proj}`) are
/// ignored.
fn has_top_level_colon(s: &str) -> bool {
    let mut brace: usize = 0;
    let mut paren: usize = 0;
    for c in s.chars() {
        match c {
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            ':' if brace == 0 && paren == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Parses the inner text of an argument list (`argname: value, ...`) into
/// `(argname, value-template)` pairs. Splitting on top-level `,` and `:` honors
/// nested `{...}`/`(...)` groups so an arg value may itself contain commas,
/// colons, and nested slots.
///
/// # Errors
///
/// [`TemplateError::MalformedSlotArgs`] on an empty arg name, a segment with no
/// `:`, or a value that fails to parse as a template.
fn parse_slot_args(inner: &str) -> Result<Vec<(String, Template)>, TemplateError> {
    let mut args = Vec::new();
    for segment in split_top_level(inner, ',') {
        let segment = segment.trim();
        if segment.is_empty() {
            return Err(TemplateError::MalformedSlotArgs(
                "empty argument segment (stray comma?)".to_string(),
            ));
        }
        let Some((arg_name, value)) = split_once_top_level(segment, ':') else {
            return Err(TemplateError::MalformedSlotArgs(format!(
                "argument `{segment}` is missing a `name: value` label"
            )));
        };
        let arg_name = arg_name.trim();
        let value = value.trim();
        if !is_bare_identifier(arg_name) {
            return Err(TemplateError::MalformedSlotArgs(format!(
                "argument name `{arg_name}` is not a bare identifier"
            )));
        }
        let value_template = Template::parse(value)?;
        args.push((arg_name.to_string(), value_template));
    }
    if args.is_empty() {
        return Err(TemplateError::MalformedSlotArgs(
            "empty argument list".to_string(),
        ));
    }
    Ok(args)
}

/// Splits `s` on top-level occurrences of `sep` (outside any `{...}`/`(...)`),
/// returning the segments in order.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut brace: usize = 0;
    let mut paren: usize = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            _ if c == sep && brace == 0 && paren == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Splits `s` on the FIRST top-level occurrence of `sep` (outside any
/// `{...}`/`(...)`), returning the two sides, or `None` if `sep` never appears
/// at top level.
fn split_once_top_level(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut brace: usize = 0;
    let mut paren: usize = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            _ if c == sep && brace == 0 && paren == 0 => {
                return Some((&s[..i], &s[i + c.len_utf8()..]));
            }
            _ => {}
        }
    }
    None
}

/// Whether `s` is a non-empty bare identifier — no `:`, `(`, `)`, `{`, `}`, `,`,
/// or whitespace. Only bare identifiers can be argument names (so an argument
/// reference `{name}` is unambiguous) and can be looked up as pushed args.
fn is_bare_identifier(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().any(|c| {
            c.is_whitespace() || matches!(c, ':' | '(' | ')' | '{' | '}' | ',')
        })
}

/// Splits a slot reference into its base name and an optional projected item
/// slot, on the FIRST `:`.
///
/// A slot reference in a template is either:
/// - `name` — no `:` — returns `("name", None)`. This is the existing form and
///   MUST keep behaving byte-identically (the backward-compat guarantee).
/// - `name:item` — a **projected collection slot** — returns
///   `("name", Some("item"))`. It loops the SAME collection as `name` but
///   renders each element through the `### item` subsection instead of the
///   collection's default item slot.
///
/// The split is on the first `:` only; both sides are slot identifiers. A slot
/// name that contains no `:` (the overwhelming common case, including every
/// special-slot form such as `meta.key`, `resolve_fnptr(x)`, and
/// `fresh_name(a, b)` — none of which contain `:`) returns `None`, so those
/// paths are entirely unaffected.
///
/// Whether the projection is *valid* (the base must be a collection, the item
/// slot must exist) is decided by the caller — this function only splits.
pub fn parse_slot_projection(name: &str) -> (&str, Option<&str>) {
    match name.split_once(':') {
        Some((base, item)) => (base, Some(item)),
        None => (name, None),
    }
}

/// Fills template slots. Implementors decide what each named slot renders to,
/// which is where recursion into child constructs occurs.
pub trait SlotResolver {
    /// The error type a resolver may produce.
    type Error;

    /// Renders the slot named `name` into a [`Rendered`] fragment.
    fn resolve(&mut self, name: &str) -> Result<Rendered, Self::Error>;

    /// Pushes a frame of caller-supplied named arguments onto the resolver's
    /// argument stack, in effect for the duration of resolving one
    /// argument-bearing slot reference and its nested sub-renders.
    ///
    /// The default is a no-op: a resolver that does not participate in argument
    /// passing (e.g. a simple test resolver) simply ignores pushed args, and a
    /// `{argname}` reference then falls through to normal slot resolution. Real
    /// resolvers override this (and [`resolve_arg`](SlotResolver::resolve_arg) /
    /// [`pop_args`](SlotResolver::pop_args)) to store the frame on the shared
    /// render state so it is visible to every nested resolver.
    fn push_args(&mut self, _args: Vec<(String, Rendered)>) {}

    /// Pops the most recently pushed argument frame. Paired with
    /// [`push_args`](SlotResolver::push_args); the default is a no-op.
    fn pop_args(&mut self) {}

    /// Looks up a caller-supplied argument by `name`, returning its
    /// already-rendered value if one is in scope (innermost frame wins, so a
    /// deeper passed arg shadows a shallower one). The default returns `None`,
    /// so a bare `{name}` reference falls through to normal slot resolution.
    fn resolve_arg(&mut self, _name: &str) -> Option<Rendered> {
        None
    }
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
        assert_eq!(
            t.slot_names(),
            vec!["export", "name", "params", "ret", "body"]
        );
    }

    #[test]
    fn rejects_unclosed_slot() {
        assert_eq!(
            Template::parse("fn {name"),
            Err(TemplateError::UnclosedSlot)
        );
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

    #[test]
    fn slot_projection_splits_on_first_colon() {
        assert_eq!(parse_slot_projection("params"), ("params", None));
        assert_eq!(
            parse_slot_projection("params:param_type"),
            ("params", Some("param_type"))
        );
        // Split on the FIRST colon only; a stray colon in the item is kept.
        assert_eq!(parse_slot_projection("a:b:c"), ("a", Some("b:c")));
    }

    #[test]
    fn no_colon_forms_return_none_projection() {
        // Special-slot forms contain no `:`, so they are never treated as
        // projections (backward-compat guarantee).
        assert_eq!(parse_slot_projection("meta.key"), ("meta.key", None));
        assert_eq!(
            parse_slot_projection("resolve_fnptr(value)"),
            ("resolve_fnptr(value)", None)
        );
        assert_eq!(
            parse_slot_projection("fresh_name(loop, w)"),
            ("fresh_name(loop, w)", None)
        );
    }

    // ---- Slot arguments `{name(arg: value, ...)}` ----

    /// An argument-aware resolver: a stack of `(name -> text)` frames, plus a
    /// base map for normal slots. Mirrors how the real emitter delegates the
    /// three trait methods to the shared unit index.
    struct ArgResolver {
        base: std::collections::HashMap<String, String>,
        frames: Vec<Vec<(String, Rendered)>>,
    }

    impl ArgResolver {
        fn new(pairs: &[(&str, &str)]) -> Self {
            ArgResolver {
                base: pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                frames: Vec::new(),
            }
        }
    }

    impl SlotResolver for ArgResolver {
        type Error = String;
        fn resolve(&mut self, name: &str) -> Result<Rendered, String> {
            self.base
                .get(name)
                .map(|s| Rendered::text(s.clone()))
                .ok_or_else(|| format!("no slot {name}"))
        }
        fn push_args(&mut self, args: Vec<(String, Rendered)>) {
            self.frames.push(args);
        }
        fn pop_args(&mut self) {
            self.frames.pop();
        }
        fn resolve_arg(&mut self, name: &str) -> Option<Rendered> {
            self.frames
                .iter()
                .rev()
                .find_map(|f| f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
        }
    }

    #[test]
    fn no_arg_slot_parses_with_empty_args() {
        // Every existing form parses to a SlotRef with empty args (byte-compat).
        let t = Template::parse("{name}").expect("parse");
        assert_eq!(
            t.parts,
            vec![TemplatePart::Slot(SlotRef {
                name: "name".to_string(),
                args: Vec::new(),
            })]
        );
    }

    #[test]
    fn helper_call_without_colon_stays_a_plain_name() {
        // `escape(x, c)` / `field_type(a, b)` have no top-level `:`, so they are
        // NOT treated as the arg form: the whole text is the slot name.
        for form in ["escape(value, c)", "field_type(Point, x)", "resolve_fnptr(v)"] {
            let t = Template::parse(&format!("{{{form}}}")).expect("parse");
            assert_eq!(t.slot_names(), vec![form]);
        }
    }

    #[test]
    fn arg_slot_parses_name_and_values() {
        // `{type(name: {name})}` -> slot "type" with one arg "name" = `{name}`.
        let t = Template::parse("{type(name: {name})}").expect("parse");
        match &t.parts[0] {
            TemplatePart::Slot(s) => {
                assert_eq!(s.name, "type");
                assert_eq!(s.args.len(), 1);
                assert_eq!(s.args[0].0, "name");
                assert_eq!(s.args[0].1.slot_names(), vec!["name"]);
            }
            other => panic!("expected slot, got {other:?}"),
        }
    }

    #[test]
    fn arg_reference_resolves_and_shadows() {
        // Directly exercise resolve_slot semantics: a passed arg is visible as
        // `{argname}` and SHADOWS a same-named base slot during the sub-render.
        // Template: `{callee(name: {name})}` where base has name="OUTER" and
        // callee="<{name}>". The arg `name` = caller's `{name}` = "OUTER"; but
        // to prove shadowing we give the CALLEE a different base `name`.
        //
        // We model the callee as a template by having `resolve("callee")` render
        // a sub-template through the SAME resolver.
        struct NestingResolver {
            frames: Vec<Vec<(String, Rendered)>>,
        }
        impl SlotResolver for NestingResolver {
            type Error = String;
            fn resolve(&mut self, name: &str) -> Result<Rendered, String> {
                match name {
                    // The callee references `{name}` — which must resolve to the
                    // PUSHED arg (shadowing), not this base value.
                    "callee" => Template::parse("<{name}>")
                        .map_err(|e| e.to_string())?
                        .render(self),
                    "name" => Ok(Rendered::text("BASE")),
                    "outer_name" => Ok(Rendered::text("OUTER")),
                    other => Err(format!("no slot {other}")),
                }
            }
            fn push_args(&mut self, args: Vec<(String, Rendered)>) {
                self.frames.push(args);
            }
            fn pop_args(&mut self) {
                self.frames.pop();
            }
            fn resolve_arg(&mut self, name: &str) -> Option<Rendered> {
                self.frames
                    .iter()
                    .rev()
                    .find_map(|f| f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
            }
        }

        // Pass `name: {outer_name}` -> the arg value renders in the CALLER scope
        // to "OUTER"; inside `callee`, `{name}` resolves to the pushed "OUTER"
        // (shadowing the base "BASE").
        let t = Template::parse("{callee(name: {outer_name})}").expect("parse");
        let mut r = NestingResolver { frames: Vec::new() };
        let out = t.render(&mut r).expect("render");
        assert_eq!(out.text, "<OUTER>");
        // After rendering, the frame is popped: a bare `{name}` now sees BASE.
        let t2 = Template::parse("{name}").expect("parse");
        assert_eq!(t2.render(&mut r).expect("render").text, "BASE");
    }

    #[test]
    fn args_propagate_to_nested_sub_renders() {
        // An arg pushed at the outer invocation stays visible through a nested
        // sub-render (the callee renders another slot that references the arg).
        struct DeepResolver {
            frames: Vec<Vec<(String, Rendered)>>,
        }
        impl SlotResolver for DeepResolver {
            type Error = String;
            fn resolve(&mut self, name: &str) -> Result<Rendered, String> {
                match name {
                    "outer" => Template::parse("[{inner}]")
                        .map_err(|e| e.to_string())?
                        .render(self),
                    // `inner` references `{tag}` which was passed to `outer` two
                    // levels up — it must still be in scope here.
                    "inner" => Template::parse("{tag}")
                        .map_err(|e| e.to_string())?
                        .render(self),
                    "src" => Ok(Rendered::text("T")),
                    other => Err(format!("no slot {other}")),
                }
            }
            fn push_args(&mut self, args: Vec<(String, Rendered)>) {
                self.frames.push(args);
            }
            fn pop_args(&mut self) {
                self.frames.pop();
            }
            fn resolve_arg(&mut self, name: &str) -> Option<Rendered> {
                self.frames
                    .iter()
                    .rev()
                    .find_map(|f| f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
            }
        }
        let t = Template::parse("{outer(tag: {src})}").expect("parse");
        let mut r = DeepResolver { frames: Vec::new() };
        assert_eq!(t.render(&mut r).expect("render").text, "[T]");
    }

    #[test]
    fn unpassed_arg_reference_falls_through_to_resolver_error() {
        // A `{arg}` reference with no arg in scope and no base slot is a normal
        // unknown-slot error from the resolver (clean render-time error).
        let t = Template::parse("{missing}").expect("parse");
        let mut r = ArgResolver::new(&[]);
        assert_eq!(t.render(&mut r), Err("no slot missing".to_string()));
    }

    #[test]
    fn arg_value_renders_in_caller_scope_not_callee() {
        // The arg VALUE is rendered in the caller's scope. Prove it with a
        // resolver where `echo` renders `{v}` and the value references a
        // caller-only base slot `{caller_only}`.
        let t = Template::parse("{echo(v: {caller_only})}").expect("parse");
        struct Echo {
            frames: Vec<Vec<(String, Rendered)>>,
            base: std::collections::HashMap<String, String>,
        }
        impl SlotResolver for Echo {
            type Error = String;
            fn resolve(&mut self, name: &str) -> Result<Rendered, String> {
                if name == "echo" {
                    return Template::parse("{v}").map_err(|e| e.to_string())?.render(self);
                }
                self.base
                    .get(name)
                    .map(|s| Rendered::text(s.clone()))
                    .ok_or_else(|| format!("no slot {name}"))
            }
            fn push_args(&mut self, args: Vec<(String, Rendered)>) {
                self.frames.push(args);
            }
            fn pop_args(&mut self) {
                self.frames.pop();
            }
            fn resolve_arg(&mut self, name: &str) -> Option<Rendered> {
                self.frames
                    .iter()
                    .rev()
                    .find_map(|f| f.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
            }
        }
        let mut e = Echo {
            frames: Vec::new(),
            base: [("caller_only".to_string(), "CV".to_string())]
                .into_iter()
                .collect(),
        };
        assert_eq!(t.render(&mut e).expect("render").text, "CV");
    }

    #[test]
    fn rejects_malformed_arg_list() {
        // A parenthesized tail with a top-level `:` but a bad body is an error.
        assert!(matches!(
            Template::parse("{t(: v)}"),
            Err(TemplateError::MalformedSlotArgs(_))
        ));
        // No colon at all -> treated as a plain helper name, NOT an error.
        assert!(Template::parse("{t(a v)}").is_ok());
        assert!(matches!(
            Template::parse("{(a: v)}"),
            Err(TemplateError::MalformedSlotArgs(_))
        ));
    }

    #[test]
    fn arg_value_may_contain_commas_and_nested_slots() {
        // Splitting honors nested `{...}`/`(...)`, so an arg value with a comma
        // inside a nested group stays one value.
        let t = Template::parse("{f(a: {g(x: {y}, z: {w})})}").expect("parse");
        match &t.parts[0] {
            TemplatePart::Slot(s) => {
                assert_eq!(s.name, "f");
                assert_eq!(s.args.len(), 1);
                assert_eq!(s.args[0].0, "a");
            }
            other => panic!("expected slot, got {other:?}"),
        }
    }

    #[test]
    fn multiple_args_parse_in_order() {
        let t = Template::parse("{f(a: {x}, b: {y})}").expect("parse");
        match &t.parts[0] {
            TemplatePart::Slot(s) => {
                assert_eq!(s.args.len(), 2);
                assert_eq!(s.args[0].0, "a");
                assert_eq!(s.args[1].0, "b");
            }
            other => panic!("expected slot, got {other:?}"),
        }
    }
}
