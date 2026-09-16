//! Parser for rigid `.mdl` language-definition documents (template model).
//!
//! A language definition is a markdown-*compatible* document, strictly
//! validated (not free-form markdown). Shape:
//!
//! ~~~text
//! # Lamina Language Definition: <name>
//!
//! <free prose, ignored>
//!
//! ## Function
//!
//! ### decl
//! | When | Template |
//! |------|----------|
//! | else | "{vis}{async}fn {name}({params}){ret} {{\n{body}\n}}" |
//!
//! ### ret
//! | When         | Template |
//! | ret is void  | "" |
//! | else         | " -> {ret_type}" |
//! ...
//!
//! ## Capabilities
//! | Primitive | Action   | Target |
//! | i32       | identity | i32    |
//! ~~~
//!
//! Each `### <name>` under `## Function` is a `When` table: a markdown pipe
//! table whose first column is a [`Predicate`](crate::predicate) and second is
//! a double-quoted [`Template`]. Rows are evaluated first-match-wins and the
//! table must end in an `else` row. Prose between sections is ignored.
//!
//! The engine ships NO built-in definitions — a definition is always loaded
//! from a document like this, keeping the engine "dumb."

use std::collections::HashMap;

use crate::ast::{slot_binding, BinaryOp, ItemKind, Primitive, SlotScope, SlotShape, UnaryOp};
use crate::error::LangDocError;
use crate::lang::{
    Capability, FunctionDef, ItemDef, LanguageDef, OperatorSpelling, Outcome, SlotDef, WhenRow,
    WhenTable,
};
use crate::predicate::parse_predicate;
use crate::render::Template;

const TITLE_PREFIX: &str = "# Lamina Language Definition:";
const FUNCTION_HEADING: &str = "## Function";
const CAPABILITIES_HEADING: &str = "## Capabilities";
const OPERATORS_HEADING: &str = "## Operators";

/// Parses a rigid template-model `.mdl` language-definition document into a
/// [`LanguageDef`].
///
/// Shape: `## Function` contains exactly one ```` ```template ```` block (the
/// entry template) followed by `### <slot>` subsections (each a `template`
/// block or a `When` table). Every referenced slot must resolve to a subsection
/// or a terminal slot; this is validated at load time.
///
/// # Errors
///
/// Returns a [`LangDocError`] if the document is missing a required element, any
/// table/template/predicate is malformed, or a referenced slot is unresolved.
pub fn parse_language_def(src: &str) -> Result<LanguageDef, LangDocError> {
    let name = parse_title(src)?;

    require_heading(src, FUNCTION_HEADING)?;
    require_heading(src, CAPABILITIES_HEADING)?;

    // The `## Function` section is required and always present; it also hosts
    // the SHARED deep-helper slots (`### expr`, `### statement`, `### pointer`,
    // …) that the expression/type/statement resolvers reach regardless of which
    // top-level item is being rendered.
    let function_src = section_body(src, FUNCTION_HEADING);
    let (entry, slots) = parse_entry_and_slots(&function_src, SlotScope::Function)?;
    let function = FunctionDef { entry, slots };

    // Each non-function item kind gets its OWN `## <Item>` section (a sibling of
    // `## Function`). These sections are OPTIONAL: a definition that omits one
    // simply cannot emit that item kind (using it becomes an emit-time
    // `UnknownItem` error). When present, each is parsed exactly like the
    // function section but validated in its own slot scope.
    let mut items = HashMap::new();
    for kind in ItemKind::all() {
        if kind == ItemKind::Function {
            continue;
        }
        let heading = format!("## {}", kind.heading());
        if !src.lines().any(|l| l.trim_end() == heading) {
            continue;
        }
        let item_src = section_body(src, &heading);
        let (entry, slots) = parse_entry_and_slots(&item_src, kind.scope())?;
        items.insert(kind, ItemDef { entry, slots });
    }

    let capability_src = section_body(src, CAPABILITIES_HEADING);
    let capabilities = parse_capability_table(&capability_src)?;

    // Enforce completeness: every kernel primitive MUST have a matrix row.
    let missing: Vec<&str> = Primitive::all()
        .iter()
        .filter(|p| !capabilities.contains_key(p))
        .map(|p| p.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(LangDocError::IncompleteCapabilityMatrix {
            missing: missing.join(", "),
        });
    }

    // The `## Operators` section is OPTIONAL: an absent section means every
    // operator emits its canonical spelling. When present, it lists only the
    // exceptions (spelling overrides and forbids).
    let operators = if src.lines().any(|l| l.trim_end() == OPERATORS_HEADING) {
        let operator_src = section_body(src, OPERATORS_HEADING);
        parse_operator_table(&operator_src)?
    } else {
        HashMap::new()
    };

    Ok(LanguageDef {
        name,
        capabilities,
        function,
        items,
        operators,
    })
}

/// Parses one item/function section body into its entry template and slot map,
/// then validates the slot graph starting in `scope`.
///
/// The shape is identical for every `## <Item>` section (including
/// `## Function`): a single `template` block (the entry) followed by
/// `### <slot>` subsections. Validation is scope-aware so `{name}` binds to the
/// right thing at each level (a struct's name at struct scope, a field's name
/// within a `### field` item slot, …).
fn parse_entry_and_slots(
    section_src: &str,
    scope: SlotScope,
) -> Result<(Template, HashMap<String, SlotDef>), LangDocError> {
    let entry_str = extract_entry_template(section_src)?;
    let entry = Template::parse(&entry_str).map_err(|e| LangDocError::BadTemplate {
        table: "<entry>".to_string(),
        detail: e.to_string(),
    })?;
    let slots = parse_slot_subsections(section_src)?;
    validate_slot_graph(&entry, &slots, scope)?;
    Ok((entry, slots))
}

/// Parses the required title line and returns the captured target name.
fn parse_title(src: &str) -> Result<String, LangDocError> {
    let first = src.lines().find(|line| !line.trim().is_empty());
    match first {
        Some(line) if line.trim_start().starts_with(TITLE_PREFIX) => {
            let name = line.trim_start()[TITLE_PREFIX.len()..].trim().to_string();
            if name.is_empty() {
                Err(LangDocError::MissingTitle {
                    found: line.to_string(),
                })
            } else {
                Ok(name)
            }
        }
        Some(line) => Err(LangDocError::MissingTitle {
            found: line.to_string(),
        }),
        None => Err(LangDocError::MissingTitle {
            found: String::new(),
        }),
    }
}

/// Verifies that a required `##` section heading is present.
fn require_heading(src: &str, heading: &str) -> Result<(), LangDocError> {
    if src.lines().any(|line| line.trim_end() == heading) {
        Ok(())
    } else {
        Err(LangDocError::MissingSection {
            heading: heading.to_string(),
        })
    }
}

/// Returns the lines of a `##` section: everything after the heading up to the
/// next `## ` heading (or end of document).
fn section_body(src: &str, heading: &str) -> String {
    let mut out = String::new();
    let mut in_section = false;
    for line in src.lines() {
        if line.trim_end() == heading {
            in_section = true;
            continue;
        }
        if in_section {
            // A new top-level `## ` heading ends this section (but `### ` sub
            // headings stay within it).
            if line.trim_start().starts_with("## ") && !line.trim_start().starts_with("###") {
                break;
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Extracts the single entry ```` ```template ```` block that appears in the
/// Function section before any `### ` subsection.
fn extract_entry_template(function_src: &str) -> Result<String, LangDocError> {
    // Only look at lines before the first `### ` subsection.
    let mut head = String::new();
    for line in function_src.lines() {
        if line.trim_start().starts_with("### ") {
            break;
        }
        head.push_str(line);
        head.push('\n');
    }
    match extract_template_block(&head)? {
        Some(body) => Ok(body),
        None => Err(LangDocError::MissingTable {
            name: "<entry template>".to_string(),
        }),
    }
}

/// Extracts the body of the first ```` ```template ```` fenced block in `src`,
/// or `None` if there is none. The body is returned verbatim (templates are
/// authored at column 0), with exactly one trailing newline trimmed (the fence
/// sits on its own line).
fn extract_template_block(src: &str) -> Result<Option<String>, LangDocError> {
    let mut lines = src.lines();
    let mut found = false;
    for line in lines.by_ref() {
        if line.trim_end() == "```template" {
            found = true;
            break;
        }
    }
    if !found {
        return Ok(None);
    }
    let mut body = String::new();
    for line in lines.by_ref() {
        if line.trim_end() == "```" {
            // Trim exactly one trailing newline (the closing fence's own line).
            if body.ends_with('\n') {
                body.pop();
            }
            return Ok(Some(body));
        }
        body.push_str(line);
        body.push('\n');
    }
    Err(LangDocError::BadTemplate {
        table: "<template block>".to_string(),
        detail: "unterminated ```template``` block".to_string(),
    })
}

/// Parses all `### <name>` slot subsections in the Function section. Each is a
/// [`SlotDef`]: a `template` block or a `When` table.
fn parse_slot_subsections(function_src: &str) -> Result<HashMap<String, SlotDef>, LangDocError> {
    let mut slots = HashMap::new();
    let mut current_name: Option<String> = None;
    let mut current_body = String::new();

    fn flush(
        slots: &mut HashMap<String, SlotDef>,
        name: &Option<String>,
        body: &mut String,
    ) -> Result<(), LangDocError> {
        if let Some(name) = name {
            let def = parse_slot_body(name, body)?;
            slots.insert(name.clone(), def);
        }
        body.clear();
        Ok(())
    }

    for line in function_src.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("### ") {
            flush(&mut slots, &current_name, &mut current_body)?;
            current_name = Some(rest.trim().to_string());
        } else if current_name.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
        // Lines before the first `### ` (the entry template) are handled
        // separately by `extract_entry_template`.
    }
    flush(&mut slots, &current_name, &mut current_body)?;

    Ok(slots)
}

/// Parses one slot subsection body into a [`SlotDef`]: a `template` block if one
/// is present (a fixed render outcome), otherwise a `When` table.
fn parse_slot_body(name: &str, body: &str) -> Result<SlotDef, LangDocError> {
    if let Some(template_str) = extract_template_block(body)? {
        let template = Template::parse(&template_str).map_err(|e| LangDocError::BadTemplate {
            table: name.to_string(),
            detail: e.to_string(),
        })?;
        return Ok(SlotDef::Fixed(Outcome::Render(template)));
    }
    let rows: Vec<String> = body
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .map(|l| l.to_string())
        .collect();
    if rows.is_empty() {
        return Err(LangDocError::MissingTable {
            name: name.to_string(),
        });
    }
    let table = build_table(name, &rows)?;
    Ok(SlotDef::Table(table))
}

/// Validates that every slot referenced anywhere resolves — either to an
/// engine-bound slot (in the scope it is referenced in) or to a `### <slot>`
/// subsection — and that any sequence-shaped slot has its item subsection.
///
/// This is fully shape-driven: cardinality (scalar vs sequence) and the item
/// slot come from [`slot_binding`], not any hardcoded list. Scope matters —
/// `{name}` in the entry (function scope) binds to the function name, while
/// `{name}` inside a `param` item slot binds to the parameter name.
fn validate_slot_graph(
    entry: &Template,
    slots: &HashMap<String, SlotDef>,
    scope: SlotScope,
) -> Result<(), LangDocError> {
    let mut visited: Vec<(String, SlotScope)> = Vec::new();
    // Seed with the entry template's referenced slots, in the section's scope.
    let mut work: Vec<(String, SlotScope)> = entry
        .slot_names()
        .iter()
        .map(|s| (s.to_string(), scope))
        .collect();

    while let Some((name, scope)) = work.pop() {
        if visited.contains(&(name.clone(), scope)) {
            continue;
        }
        visited.push((name.clone(), scope));

        match slot_binding(&name, scope) {
            Some(SlotShape::Scalar) => {
                // Resolves directly; references nothing further.
            }
            Some(SlotShape::Sequence {
                item_slot,
                item_scope,
            }) => {
                // The item slot subsection must exist, and its referenced slots
                // are validated in the element scope.
                let def = slots.get(&item_slot).ok_or_else(|| LangDocError::MissingItemSlot {
                    collection: name.clone(),
                    item: item_slot.clone(),
                })?;
                for r in referenced_by(def) {
                    work.push((r, item_scope));
                }
            }
            None => {
                // Must be satisfied by a subsection; its references stay in the
                // same scope (function-level helper slots like `ret`, `vis`).
                let def = slots.get(&name).ok_or_else(|| LangDocError::UnknownSlotReference {
                    slot: name.clone(),
                })?;
                for r in referenced_by(def) {
                    work.push((r, scope));
                }
            }
        }
    }
    Ok(())
}

/// The slot names referenced by a slot definition's template(s).
fn referenced_by(def: &SlotDef) -> Vec<String> {
    let mut out = Vec::new();
    let add = |outcome: &Outcome, out: &mut Vec<String>| {
        if let Outcome::Render(t) = outcome {
            out.extend(t.slot_names().iter().map(|s| s.to_string()));
        }
    };
    match def {
        SlotDef::Fixed(outcome) => add(outcome, &mut out),
        SlotDef::Table(table) => {
            for row in &table.rows {
                add(&row.outcome, &mut out);
            }
        }
    }
    out
}

/// Builds a [`WhenTable`] from the raw pipe rows of one `### <name>` table.
///
/// Skips the header row (`| When | Template |`) and the separator row; parses
/// each remaining row into a predicate + template; requires a final `else` row.
fn build_table(name: &str, rows: &[String]) -> Result<WhenTable, LangDocError> {
    let mut parsed = Vec::new();

    for raw in rows {
        let cells = split_row(raw);
        if cells.len() < 2 {
            // Could be a separator row like `|----|----|`.
            if is_separator_row(&cells) {
                continue;
            }
            return Err(LangDocError::MalformedRow {
                table: name.to_string(),
                line: raw.clone(),
            });
        }
        if is_separator_row(&cells) {
            continue;
        }
        // Skip the header row.
        if cells[0].eq_ignore_ascii_case("when") {
            continue;
        }

        let predicate_src = cells[0].trim();
        let outcome_cell = cells[1].trim();

        let predicate = parse_predicate(predicate_src).map_err(|e| LangDocError::BadPredicate {
            table: name.to_string(),
            detail: e.to_string(),
        })?;
        let outcome = parse_outcome_cell(name, outcome_cell)?;

        parsed.push(WhenRow { predicate, outcome });
    }

    // Require a final `else` row (the catch-all).
    match parsed.last() {
        Some(row) if row.predicate == crate::predicate::Predicate::Else => {}
        _ => {
            return Err(LangDocError::MissingElseRow {
                table: name.to_string(),
            })
        }
    }

    Ok(WhenTable { rows: parsed })
}

/// Parses a Template-column cell into an [`Outcome`].
///
/// A cell is one of:
/// - the unquoted bareword `forbid` (→ [`Outcome::Forbid`]);
/// - an unquoted section reference `@name` (→ render the `### name` section
///   slot — equivalent to a template of just `{name}`, but quote-free since a
///   bare reference has no significant whitespace to preserve);
/// - a double-quoted string (→ a literal template to render).
///
/// Any other unquoted text is a [`LangDocError::UnquotedTemplate`]; only
/// `forbid` and `@name` references are permitted unquoted.
fn parse_outcome_cell(table: &str, cell: &str) -> Result<Outcome, LangDocError> {
    if cell == "forbid" {
        return Ok(Outcome::Forbid);
    }
    // A bare `@name` cell references the same-named `### name` section slot. It
    // is exactly equivalent to a template of `{name}`, reusing the normal
    // slot-resolution + load-time validation path.
    if let Some(name) = cell.strip_prefix('@') {
        let is_ident = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_ident {
            let template = Template::parse(&format!("{{{name}}}")).map_err(|e| {
                LangDocError::BadTemplate {
                    table: table.to_string(),
                    detail: e.to_string(),
                }
            })?;
            return Ok(Outcome::Render(template));
        }
        return Err(LangDocError::UnquotedTemplate {
            table: table.to_string(),
            value: cell.to_string(),
        });
    }
    let template_str = unquote_template(table, cell)?;
    let template = Template::parse(&template_str).map_err(|e| LangDocError::BadTemplate {
        table: table.to_string(),
        detail: e.to_string(),
    })?;
    Ok(Outcome::Render(template))
}

/// Splits a markdown pipe row into trimmed cells (dropping leading/trailing
/// pipes).
fn split_row(raw: &str) -> Vec<String> {
    raw.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// Returns `true` if all cells consist only of dashes/colons/spaces (a markdown
/// table separator row).
fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Strips the surrounding double quotes from a template cell and unescapes
/// `\n` and `\t` so multi-line templates can be written on one row.
fn unquote_template(table: &str, value: &str) -> Result<String, LangDocError> {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        Ok(unescape(inner))
    } else {
        Err(LangDocError::UnquotedTemplate {
            table: table.to_string(),
            value: value.to_string(),
        })
    }
}

/// Unescapes the small set of escapes allowed in a template cell: `\n`, `\t`,
/// `\\`, and `\"`.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses a capability matrix authored as a markdown pipe table.
///
/// Columns: `Primitive | Action | Target [| Notes]`. Header and separator rows
/// are skipped; the Notes column (if any) is ignored. `forbid` rows omit the
/// target.
///
/// # Errors
///
/// Returns a [`LangDocError`] on a malformed row, unknown primitive/action, or
/// a missing required target.
pub fn parse_capability_table(
    table: &str,
) -> Result<HashMap<Primitive, Capability>, LangDocError> {
    let mut capabilities = HashMap::new();

    for raw in table.lines() {
        let line = raw.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells = split_row(raw);
        if is_separator_row(&cells) {
            continue;
        }
        if cells.first().map(|c| c.eq_ignore_ascii_case("primitive")) == Some(true) {
            continue;
        }
        if cells.len() < 2 {
            return Err(LangDocError::MalformedCapabilityRow {
                line: raw.to_string(),
            });
        }

        let primitive_name = cells[0].as_str();
        let action = cells[1].as_str();
        let target = cells.get(2).map(|s| s.as_str()).filter(|t| !t.is_empty());

        let primitive =
            Primitive::from_name(primitive_name).ok_or_else(|| LangDocError::UnknownPrimitive {
                name: primitive_name.to_string(),
            })?;
        let capability = build_capability(action, target, primitive_name)?;
        capabilities.insert(primitive, capability);
    }

    Ok(capabilities)
}

/// Builds a [`Capability`] from an action word and optional target type.
fn build_capability(
    action: &str,
    target: Option<&str>,
    primitive_name: &str,
) -> Result<Capability, LangDocError> {
    let require_target = || {
        target
            .map(|t| t.to_string())
            .ok_or_else(|| LangDocError::MissingCapabilityTarget {
                action: action.to_string(),
                primitive: primitive_name.to_string(),
            })
    };

    match action {
        "identity" => Ok(Capability::Identity(require_target()?)),
        "alias" => Ok(Capability::Alias(require_target()?)),
        "widen" => Ok(Capability::Widen(require_target()?)),
        "wrap" => Ok(Capability::Wrap(require_target()?)),
        "forbid" => Ok(Capability::Forbid),
        _ => Err(LangDocError::UnknownAction {
            action: action.to_string(),
        }),
    }
}

/// Parses an optional operator table authored as a markdown pipe table.
///
/// Columns: `Operator | Action | Target [| Notes]`. The operator name is a
/// stable machine name (e.g. `ushr`, `floordiv`; see [`UnaryOp::name`] /
/// [`BinaryOp::name`]). Actions:
/// - `spell` — override the emitted text with the `Target` cell, and
/// - `forbid` — mark the operator inexpressible (no target required).
///
/// Header/separator rows are skipped and the `Notes` column (if any) is
/// ignored. This is the MINIMAL viable operator-capability format: operators
/// absent from the table keep their canonical spelling, so only exceptions are
/// listed. A richer format (precedence, placement, call-style lowering) is
/// deferred and escalated.
///
/// # Errors
///
/// Returns a [`LangDocError`] on a malformed row, an unknown operator name, an
/// unknown action, or a `spell` action missing its target.
pub fn parse_operator_table(
    table: &str,
) -> Result<HashMap<String, OperatorSpelling>, LangDocError> {
    let mut operators = HashMap::new();

    for raw in table.lines() {
        let line = raw.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells = split_row(raw);
        if is_separator_row(&cells) {
            continue;
        }
        if cells.first().map(|c| c.eq_ignore_ascii_case("operator")) == Some(true) {
            continue;
        }
        if cells.len() < 2 {
            return Err(LangDocError::MalformedOperatorRow {
                line: raw.to_string(),
            });
        }

        let op_name = cells[0].as_str();
        let action = cells[1].as_str();
        let target = cells.get(2).map(|s| s.as_str()).filter(|t| !t.is_empty());

        // The operator must be a known kernel operator (unary or binary).
        if UnaryOp::from_name(op_name).is_none() && BinaryOp::from_name(op_name).is_none() {
            return Err(LangDocError::UnknownOperator {
                name: op_name.to_string(),
            });
        }

        let spelling = match action {
            "spell" => {
                let target = target.ok_or_else(|| LangDocError::MissingOperatorTarget {
                    operator: op_name.to_string(),
                })?;
                OperatorSpelling::Spell(target.to_string())
            }
            "forbid" => OperatorSpelling::Forbid,
            _ => {
                return Err(LangDocError::UnknownOperatorAction {
                    action: action.to_string(),
                })
            }
        };
        operators.insert(op_name.to_string(), spelling);
    }

    Ok(operators)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{Outcome, SlotDef};
    use crate::predicate::{RenderContext, RetKind, VisKind};

    /// A complete 26-primitive capability table (every kernel primitive gets a
    /// row), so load-time completeness enforcement passes. Mappings here are
    /// only for test purposes.
    const FULL_CAPS: &str = "## Capabilities\n\
        | Primitive | Action   | Target |\n\
        |-----------|----------|--------|\n\
        | i8    | identity | i8    |\n\
        | i16   | identity | i16   |\n\
        | i32   | identity | i32   |\n\
        | i64   | identity | i64   |\n\
        | i128  | identity | i128  |\n\
        | u8    | identity | u8    |\n\
        | u16   | identity | u16   |\n\
        | u32   | identity | u32   |\n\
        | u64   | identity | u64   |\n\
        | u128  | identity | u128  |\n\
        | isize | identity | isize |\n\
        | usize | identity | usize |\n\
        | f16   | identity | f16   |\n\
        | bf16  | identity | bf16  |\n\
        | f32   | identity | f32   |\n\
        | f64   | identity | f64   |\n\
        | f128  | identity | f128  |\n\
        | bool  | identity | bool  |\n\
        | void  | alias    | ()    |\n\
        | never | alias    | !     |\n\
        | byte  | alias    | u8    |\n\
        | bytes | wrap     | Vec   |\n\
        | char  | identity | char  |\n\
        | str   | wrap     | String |\n\
        | ptr   | wrap     | Ptr   |\n\
        | fnptr | wrap     | Fn    |\n";

    /// Builds a full document from a `## Function` section body, appending a
    /// complete capability matrix so completeness enforcement passes.
    fn mk(function_section: &str) -> String {
        format!(
            "# Lamina Language Definition: rust\n\n## Function\n\n{function_section}\n{FULL_CAPS}"
        )
    }

    /// The standard function section used by several tests.
    const FN_SECTION: &str = "```template\n\
        {vis}fn {name}({params}){ret} {{\n\
        {body}\n\
        }}\n\
        ```\n\
        \n\
        ### ret\n\
        | When         | Template |\n\
        |--------------|----------|\n\
        | ret is void  | \"\" |\n\
        | else         | \" -> {ret_type}\" |\n\
        \n\
        ### vis\n\
        | When           | Template |\n\
        |----------------|----------|\n\
        | vis is public  | \"pub \" |\n\
        | else           | \"\" |\n\
        \n\
        ### param\n\
        | When  | Template |\n\
        |-------|----------|\n\
        | first | \"{name}: {type}\" |\n\
        | else  | \", {name}: {type}\" |\n\
        \n\
        ### statement\n\
        ```template\n\
        return {value};\n\
        ```";

    #[test]
    fn parses_full_document() {
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        assert_eq!(def.name, "rust");
        assert!(def.function.entry.slot_names().contains(&"vis"));
        assert!(def.function.entry.slot_names().contains(&"body"));
        assert!(def.function.slots.contains_key("ret"));
        assert!(def.function.slots.contains_key("vis"));
        assert_eq!(
            def.capability(Primitive::I32),
            Some(&Capability::Identity("i32".to_string()))
        );
    }

    #[test]
    fn ret_slot_is_a_table_that_branches() {
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        match def.function.slots.get("ret").expect("ret slot") {
            SlotDef::Table(table) => {
                let void_ctx = RenderContext {
                    ret: Some(RetKind::Void),
                    ..Default::default()
                };
                match table.select(&void_ctx).expect("void row") {
                    Outcome::Render(t) => assert!(t.slot_names().is_empty()),
                    Outcome::Forbid => panic!("void row should render"),
                }
                let type_ctx = RenderContext {
                    ret: Some(RetKind::Type),
                    ..Default::default()
                };
                match table.select(&type_ctx).expect("else row") {
                    Outcome::Render(t) => assert!(t.slot_names().contains(&"ret_type")),
                    Outcome::Forbid => panic!("else row should render"),
                }
            }
            SlotDef::Fixed(_) => panic!("ret should be a table"),
        }
    }

    #[test]
    fn a_fixed_slot_can_be_a_template_block() {
        // A custom (non-iterable) slot `{prefix}` resolved by a fixed template
        // block; `name`/`body` are terminal/iterable and provided.
        let doc = mk("```template\n{prefix}fn {name}() {{ {body} }}\n```\n\n\
            ### prefix\n\
            ```template\nunsafe \n```\n\n\
            ### statement\n\
            ```template\nreturn {value};\n```");
        let def = parse_language_def(&doc).expect("parses");
        match def.function.slots.get("prefix").expect("prefix slot") {
            SlotDef::Fixed(Outcome::Render(_)) => {}
            _ => panic!("prefix should be a fixed template"),
        }
    }

    #[test]
    fn vis_else_row_is_forbid_directive() {
        let doc = mk("```template\n{vis}fn {name}() {{ {body} }}\n```\n\n\
            ### vis\n| When | Template |\n|------|----------|\n| vis is public | \"pub \" |\n| else | forbid |\n\n\
            ### statement\n```template\nreturn {value};\n```");
        let def = parse_language_def(&doc).expect("parses");
        match def.function.slots.get("vis").expect("vis slot") {
            SlotDef::Table(table) => {
                let priv_ctx = RenderContext {
                    vis: Some(VisKind::Private),
                    ..Default::default()
                };
                assert_eq!(table.select(&priv_ctx), Some(&Outcome::Forbid));
            }
            SlotDef::Fixed(_) => panic!("vis should be a table"),
        }
    }

    #[test]
    fn bare_at_reference_cell_resolves_to_section_slot() {
        // A table cell `@if_stmt` (no quotes) references the `### if_stmt`
        // section, equivalent to a template of `{if_stmt}`. A dangling
        // `@missing` would fail slot-graph validation, so a resolvable one must
        // parse and a missing one must error.
        let ok = mk("```template\n{vis}fn {name}() {{ {body} }}\n```\n\n\
            ### vis\n```template\npub \n```\n\n\
            ### statement\n| When | Template |\n|------|----------|\n| stmt is return | @ret_stmt |\n| else | forbid |\n\n\
            ### ret_stmt\n```template\nreturn {value};\n```");
        assert!(parse_language_def(&ok).is_ok(), "resolvable @ref should parse");

        let dangling = mk("```template\n{vis}fn {name}() {{ {body} }}\n```\n\n\
            ### vis\n```template\npub \n```\n\n\
            ### statement\n| When | Template |\n|------|----------|\n| stmt is return | @missing_section |\n| else | forbid |");
        assert!(
            matches!(
                parse_language_def(&dangling),
                Err(LangDocError::UnknownSlotReference { .. })
            ),
            "dangling @ref must fail slot-graph validation"
        );
    }

    #[test]
    fn vis_public_row_present() {
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        match def.function.slots.get("vis").expect("vis slot") {
            SlotDef::Table(table) => {
                let pub_ctx = RenderContext {
                    vis: Some(VisKind::Public),
                    ..Default::default()
                };
                assert!(table.select(&pub_ctx).is_some());
            }
            SlotDef::Fixed(_) => panic!("vis should be a table"),
        }
    }

    #[test]
    fn rejects_incomplete_capability_matrix() {
        // Only i32 present -> 25 missing. Entry uses only scalar terminals so
        // slot-graph validation passes and the completeness check is reached.
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n\
            ```template\nfn {name}()\n```\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        match parse_language_def(doc) {
            Err(LangDocError::IncompleteCapabilityMatrix { missing }) => {
                assert!(missing.contains("i8"));
                assert!(missing.contains("fnptr"));
                assert!(missing.contains("str"));
                assert!(!missing.contains("i32"));
            }
            other => panic!("expected IncompleteCapabilityMatrix, got {other:?}"),
        }
    }

    #[test]
    fn complete_matrix_passes_enforcement() {
        // The FULL_CAPS table covers all 26 primitives.
        let def = parse_language_def(&mk(FN_SECTION)).expect("complete matrix should pass");
        assert_eq!(def.capabilities.len(), 26);
    }

    #[test]
    fn rejects_missing_title() {
        let doc = "## Function\n\n```template\n{name}\n```\n## Capabilities\n| i32 | identity | i32 |\n";
        assert!(matches!(
            parse_language_def(doc),
            Err(LangDocError::MissingTitle { .. })
        ));
    }

    #[test]
    fn rejects_missing_entry_template() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n### ret\n| When | Template |\n| else | \"\" |\n\n## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        assert!(matches!(
            parse_language_def(doc),
            Err(LangDocError::MissingTable { .. })
        ));
    }

    #[test]
    fn rejects_dangling_slot_reference() {
        // Entry references {ret} but there is no ### ret subsection and `ret`
        // is not a terminal slot.
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n\
            ```template\nfn {name}(){ret}\n```\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        assert_eq!(
            parse_language_def(doc),
            Err(LangDocError::UnknownSlotReference {
                slot: "ret".to_string()
            })
        );
    }

    #[test]
    fn terminal_slots_need_no_subsection() {
        // Entry references only scalar terminal slots (name, ret_type) — no
        // subsections needed, must validate (with a complete matrix).
        let doc = mk("```template\nfn {name}() -> {ret_type}\n```");
        assert!(parse_language_def(&doc).is_ok());
    }

    #[test]
    fn iterable_slot_requires_item_subsection() {
        // Referencing {params} without a ### param subsection is a load-time
        // error.
        let doc = mk("```template\nfn {name}({params})\n```");
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingItemSlot {
                collection: "params".to_string(),
                item: "param".to_string(),
            })
        );
    }

    #[test]
    fn body_requires_statement_item_subsection() {
        let doc = mk("```template\nfn {name}() {{ {body} }}\n```");
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingItemSlot {
                collection: "body".to_string(),
                item: "statement".to_string(),
            })
        );
    }

    #[test]
    fn rejects_table_without_else() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n\
            ```template\nfn {name}(){ret}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| ret is void | \"\" |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        assert_eq!(
            parse_language_def(doc),
            Err(LangDocError::MissingElseRow {
                table: "ret".to_string()
            })
        );
    }

    #[test]
    fn rejects_unquoted_template_in_table() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n\
            ```template\nfn {name}(){ret}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| else | bare |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        assert!(matches!(
            parse_language_def(doc),
            Err(LangDocError::UnquotedTemplate { .. })
        ));
    }

    #[test]
    fn rejects_bad_predicate() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n\
            ```template\nfn {name}(){ret}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| frobnicate | \"x\" |\n| else | \"y\" |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n";
        assert!(matches!(
            parse_language_def(doc),
            Err(LangDocError::BadPredicate { .. })
        ));
    }

    #[test]
    fn capability_wrap_and_forbid() {
        let table = "| i32 | wrap | Boxed |\n";
        let caps = parse_capability_table(table).expect("wrap");
        assert_eq!(
            caps.get(&Primitive::I32),
            Some(&Capability::Wrap("Boxed".to_string()))
        );

        let forbid = "| i32 | forbid | |\n";
        let caps = parse_capability_table(forbid).expect("forbid");
        assert_eq!(caps.get(&Primitive::I32), Some(&Capability::Forbid));
    }

    #[test]
    fn operator_table_parses_spell_and_forbid() {
        let table = "| Operator | Action | Target |\n\
            |----------|--------|--------|\n\
            | pow  | spell  | .pow |\n\
            | ushr | forbid |      |\n";
        let ops = parse_operator_table(table).expect("operators");
        assert_eq!(
            ops.get("pow"),
            Some(&OperatorSpelling::Spell(".pow".to_string()))
        );
        assert_eq!(ops.get("ushr"), Some(&OperatorSpelling::Forbid));
        // Operators absent from the table keep their canonical spelling.
        assert_eq!(ops.get("add"), None);
    }

    #[test]
    fn operator_table_rejects_unknown_operator() {
        let table = "| matmul | spell | @ |\n";
        assert!(matches!(
            parse_operator_table(table),
            Err(LangDocError::UnknownOperator { .. })
        ));
    }

    #[test]
    fn operator_table_rejects_unknown_action() {
        let table = "| add | frobnicate | + |\n";
        assert!(matches!(
            parse_operator_table(table),
            Err(LangDocError::UnknownOperatorAction { .. })
        ));
    }

    #[test]
    fn operator_table_spell_requires_target() {
        let table = "| add | spell | |\n";
        assert!(matches!(
            parse_operator_table(table),
            Err(LangDocError::MissingOperatorTarget { .. })
        ));
    }

    #[test]
    fn optional_operators_section_is_parsed_into_def() {
        // A full document WITH an `## Operators` section populates `operators`.
        let doc = format!(
            "{}\n## Operators\n\
             | Operator | Action | Target |\n\
             |----------|--------|--------|\n\
             | ushr | forbid | |\n",
            mk(FN_SECTION)
        );
        let def = parse_language_def(&doc).expect("parses with operators");
        assert_eq!(def.operator("ushr"), Some(&OperatorSpelling::Forbid));
        assert_eq!(def.operator("add"), None);
    }

    #[test]
    fn absent_operators_section_yields_empty_map() {
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        assert!(def.operators.is_empty());
    }

    #[test]
    fn item_sections_parse_into_items_map() {
        use crate::ast::ItemKind;
        // A document with a `## Struct` and a `## Use` section (siblings of
        // `## Function`) parses each into its own ItemDef; absent item sections
        // (Enum/TypeDef/Const here) simply do not appear in the map.
        let doc = format!(
            "{}\n\
             ## Struct\n\n\
             ```template\n{{name}} {{{{ {{fields}} }}}}\n```\n\n\
             ### field\n\
             | When  | Template |\n\
             |-------|----------|\n\
             | first | \"{{name}}: {{type}}\" |\n\
             | else  | \", {{name}}: {{type}}\" |\n\n\
             ## Use\n\n\
             ```template\nuse {{path}};\n```\n",
            mk(FN_SECTION)
        );
        let def = parse_language_def(&doc).expect("parses with item sections");
        assert!(def.item_def(ItemKind::Struct).is_some());
        assert!(def.item_def(ItemKind::Use).is_some());
        // Item sections are optional: an omitted one is absent from the map.
        assert!(def.item_def(ItemKind::Enum).is_none());
        assert!(def.item_def(ItemKind::Const).is_none());
        // The struct's `field` item slot lives in its own section's slots.
        let struct_def = def.item_def(ItemKind::Struct).expect("struct def");
        assert!(struct_def.slots.contains_key("field"));
        // The shared `statement`/`param` helpers stay under `## Function`.
        assert!(def.function.slots.contains_key("statement"));
    }

    #[test]
    fn item_section_dangling_slot_is_a_load_error() {
        // A `## Struct` entry references `{vis}` with no `### vis` subsection
        // and `vis` is not engine-bound at struct scope -> load-time error.
        let doc = format!(
            "{}\n\
             ## Struct\n\n\
             ```template\n{{vis}}struct {{name}} {{{{ }}}}\n```\n",
            mk(FN_SECTION)
        );
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::UnknownSlotReference {
                slot: "vis".to_string()
            })
        );
    }

    #[test]
    fn item_section_missing_field_item_slot_errors() {
        // Referencing `{fields}` without a `### field` subsection is a
        // load-time MissingItemSlot error (shape-driven, from slot_binding).
        let doc = format!(
            "{}\n\
             ## Struct\n\n\
             ```template\nstruct {{name}} {{{{ {{fields}} }}}}\n```\n",
            mk(FN_SECTION)
        );
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingItemSlot {
                collection: "fields".to_string(),
                item: "field".to_string(),
            })
        );
    }
}
