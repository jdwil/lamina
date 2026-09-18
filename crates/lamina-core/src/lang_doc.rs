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
    Capability, FunctionDef, ItemDef, LanguageDef, OperatorSpelling, Outcome, PassPlan, RuleScope,
    SlotDef, Version, WhenRow, WhenTable, LAMINA_FORMAT_VERSION,
};
use crate::predicate::parse_predicate;
use crate::render::{parse_slot_projection, Template};

const TITLE_PREFIX: &str = "# Lamina Language Definition:";
const LANG_META_FENCE: &str = "```lang-meta";
const LANG_PASSES_FENCE: &str = "```lang-passes";
const FUNCTION_HEADING: &str = "## Function";
const CAPABILITIES_HEADING: &str = "## Capabilities";
const OPERATORS_HEADING: &str = "## Operators";
const PASSES_HEADING: &str = "## Passes";
const TREE_HEADING: &str = "## Tree";

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

    // The required `lang-meta` header block declares the versioning contract:
    // the MINIMUM `.mdl` format version the definition needs (engine-actionable
    // at load time) plus the opaque target/target-version selector the engine
    // carries verbatim. Parsed and enforced before any section work, so a
    // format-incompatible definition is refused early.
    let meta = parse_lang_meta(src)?;

    // `## Capabilities` is always required (the capability matrix gates every
    // primitive, declarative or imperative). `## Function` is OPTIONAL: a
    // purely declarative target (HTML, JSON, YAML, …) has no imperative
    // callable to render, so it may omit `## Function` entirely and instead
    // host its shared deep-helper slots (`### expr`, `### attr`, `### child`,
    // …) under a `## Tree` section. At least one of `## Function` / `## Tree`
    // must be present — a definition with neither can render nothing.
    require_heading(src, CAPABILITIES_HEADING)?;

    let has_function = src.lines().any(|l| l.trim_end() == FUNCTION_HEADING);
    let has_tree = src.lines().any(|l| l.trim_end() == TREE_HEADING);
    if !has_function && !has_tree {
        return Err(LangDocError::MissingSection {
            heading: FUNCTION_HEADING.to_string(),
        });
    }

    // The `## Function` section, when present, carries the imperative entry
    // template and hosts the SHARED deep-helper slots (`### expr`,
    // `### statement`, `### pointer`, …) reached by the expression/type/
    // statement resolvers regardless of which top-level item is being rendered.
    // When absent (a declarative-only target), the `FunctionDef` has an empty
    // entry template and its slots come from the `## Tree` section instead.
    let (entry, mut slots) = if has_function {
        let function_src = section_body(src, FUNCTION_HEADING);
        parse_entry_and_slots(&function_src, SlotScope::Function)?
    } else {
        (
            Template::parse("").map_err(|e| LangDocError::BadTemplate {
                table: "<entry>".to_string(),
                detail: e.to_string(),
            })?,
            HashMap::new(),
        )
    };

    // The `## Tree` section is a slots-only home for the declarative tree
    // core's shared helper slots (`### expr` and its `### attr` / `### child`
    // item slots, plus any node sub-slots a target names). It has no entry
    // template — a tree renders through the `### expr` dispatch. Its slots are
    // MERGED into the function slot map (both the expression resolver and the
    // top-level tree item read `lang.function.slots`), then the tree slot graph
    // is validated starting from the `### expr` entry point in expression
    // scope.
    if has_tree {
        let tree_src = section_body(src, TREE_HEADING);
        let tree_slots = parse_slot_subsections(&tree_src)?;
        for (k, v) in tree_slots {
            slots.insert(k, v);
        }
        validate_tree_slot_graph(&slots)?;
    }

    let function = FunctionDef { entry, slots };

    // Each non-function item kind gets its OWN `## <Item>` section (a sibling of
    // `## Function`). These sections are OPTIONAL: a definition that omits one
    // simply cannot emit that item kind (using it becomes an emit-time
    // `UnknownItem` error). When present, each is parsed exactly like the
    // function section but validated in its own slot scope. `Tree` is excluded:
    // a top-level tree value renders through the shared `### expr` dispatch, not
    // a `## <Item>` entry template, so `## Tree` is a helper section (handled
    // above), never an item section.
    let mut items = HashMap::new();
    for kind in ItemKind::all() {
        if kind == ItemKind::Function || kind == ItemKind::Tree {
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

    // The `## Passes` section is OPTIONAL. When absent, the definition declares
    // no passes and the emitter uses the legacy single-pass, inline-output path
    // (byte-identical to before this mechanism existed). When present, it
    // carries the ordered passes, the author-named regions, and the assembly
    // layout, all validated together (see `parse_passes`).
    let passes = if src.lines().any(|l| l.trim_end() == PASSES_HEADING) {
        parse_passes(src)?
    } else {
        PassPlan::default()
    };

    // With the pass plan known, validate that every `pass:` / `region:`
    // annotation across the whole slot/table graph names a DECLARED pass/region
    // (and that no annotation appears without a `## Passes` section). This is
    // the single cross-cutting check that keeps annotations honest.
    validate_annotations(&passes, &function, &items)?;

    Ok(LanguageDef {
        name,
        target: meta.target,
        target_version: meta.target_version,
        capabilities,
        function,
        items,
        operators,
        passes,
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

/// The parsed, validated contents of a definition's `lang-meta` header block.
struct LangMeta {
    /// The target language name (`target:`), carried verbatim.
    target: String,
    /// The opaque target-language version band (`target-version:`), carried
    /// verbatim (never parsed or compared by the engine).
    target_version: String,
}

/// Parses and validates the required `lang-meta` header block.
///
/// The block is a ```` ```lang-meta ```` fenced block near the title carrying
/// `key: value` lines. Three keys are required (and are the only ones allowed):
///
/// - `lamina-format` — the MINIMUM `.mdl` format version required (semver). The
///   engine refuses the definition if this is NEWER than
///   [`LAMINA_FORMAT_VERSION`], and loads it otherwise (backward-compatible).
/// - `target` — the target language name (carried verbatim).
/// - `target-version` — the opaque target-language version band (carried
///   verbatim; never parsed or branched on by the engine).
///
/// # Errors
///
/// Returns a [`LangDocError`] if the block is absent, a line is not `key: value`,
/// a key is unknown, a required key is missing, the format version is malformed,
/// or the required format version is newer than the engine's.
fn parse_lang_meta(src: &str) -> Result<LangMeta, LangDocError> {
    let body = extract_lang_meta_block(src)?.ok_or(LangDocError::MissingLangMeta)?;

    let mut lamina_format: Option<String> = None;
    let mut target: Option<String> = None;
    let mut target_version: Option<String> = None;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| LangDocError::MalformedLangMetaLine {
                line: raw.to_string(),
            })?;
        let key = key.trim();
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(LangDocError::MalformedLangMetaLine {
                line: raw.to_string(),
            });
        }
        match key {
            "lamina-format" => lamina_format = Some(value),
            "target" => target = Some(value),
            "target-version" => target_version = Some(value),
            other => {
                return Err(LangDocError::UnknownLangMetaKey {
                    key: other.to_string(),
                })
            }
        }
    }

    let lamina_format = lamina_format.ok_or_else(|| LangDocError::MissingLangMetaKey {
        key: "lamina-format".to_string(),
    })?;
    let target = target.ok_or_else(|| LangDocError::MissingLangMetaKey {
        key: "target".to_string(),
    })?;
    let target_version = target_version.ok_or_else(|| LangDocError::MissingLangMetaKey {
        key: "target-version".to_string(),
    })?;

    // The ONLY engine-actionable version logic: refuse a definition whose
    // required (minimum) format version is newer than the engine's. Equal or
    // older loads (the engine is backward-compatible). The target-version above
    // is opaque and gets NO such treatment.
    let required = Version::parse(&lamina_format)?;
    let engine = Version::parse(LAMINA_FORMAT_VERSION)?;
    if required > engine {
        return Err(LangDocError::FormatVersionTooNew {
            required: required.to_string(),
            engine: engine.to_string(),
        });
    }

    Ok(LangMeta {
        target,
        target_version,
    })
}

/// Extracts the body (inner lines) of the first ```` ```lang-meta ```` fenced
/// block in `src`, or `None` if there is none. The block sits near the title;
/// its contents are `key: value` lines parsed by [`parse_lang_meta`].
fn extract_lang_meta_block(src: &str) -> Result<Option<String>, LangDocError> {
    let mut lines = src.lines();
    let mut found = false;
    for line in lines.by_ref() {
        if line.trim_end() == LANG_META_FENCE {
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
            return Ok(Some(body));
        }
        body.push_str(line);
        body.push('\n');
    }
    Err(LangDocError::MalformedLangMetaLine {
        line: "unterminated ```lang-meta``` block".to_string(),
    })
}

/// Parses the `## Passes` section's ```` ```lang-passes ```` block into a
/// [`PassPlan`].
///
/// The block carries exactly three `key: value` lines — `passes:`, `regions:`,
/// `layout:` — each a comma-separated list of author-named identifiers. All
/// three keys are required; an unknown key, a missing key, a duplicate key, or
/// a `layout` naming an undeclared region is a load-time error. The engine
/// attaches no meaning to the names; they are opaque and only used to scope
/// rules and order the assembled output.
fn parse_passes(src: &str) -> Result<PassPlan, LangDocError> {
    let body = extract_lang_passes_block(src)?.ok_or_else(|| LangDocError::MalformedPasses {
        detail: "missing required ```lang-passes``` block in the `## Passes` section".to_string(),
    })?;

    let mut passes: Option<Vec<String>> = None;
    let mut regions: Option<Vec<String>> = None;
    let mut layout: Option<Vec<String>> = None;

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| LangDocError::MalformedPasses {
                detail: format!("expected `key: value`, found {raw:?}"),
            })?;
        let list: Vec<String> = value
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let set_once =
            |slot: &mut Option<Vec<String>>, key: &str| -> Result<(), LangDocError> {
                if slot.is_some() {
                    return Err(LangDocError::MalformedPasses {
                        detail: format!("duplicate `{key}:` key"),
                    });
                }
                Ok(())
            };
        match key.trim() {
            "passes" => {
                set_once(&mut passes, "passes")?;
                passes = Some(list);
            }
            "regions" => {
                set_once(&mut regions, "regions")?;
                regions = Some(list);
            }
            "layout" => {
                set_once(&mut layout, "layout")?;
                layout = Some(list);
            }
            other => {
                return Err(LangDocError::MalformedPasses {
                    detail: format!("unknown key {other:?} (expected passes, regions, or layout)"),
                })
            }
        }
    }

    let require = |slot: Option<Vec<String>>, key: &str| -> Result<Vec<String>, LangDocError> {
        match slot {
            Some(v) if !v.is_empty() => Ok(v),
            _ => Err(LangDocError::MalformedPasses {
                detail: format!("missing or empty required `{key}:` key"),
            }),
        }
    };
    let passes = require(passes, "passes")?;
    let regions = require(regions, "regions")?;
    let layout = require(layout, "layout")?;

    // Every layout entry must be a declared region (`body` is a conventional
    // region a def may name explicitly; if it appears in `layout` it must also
    // appear in `regions`, keeping the surface fully explicit).
    for region in &layout {
        if !regions.contains(region) {
            return Err(LangDocError::UndeclaredLayoutRegion {
                region: region.clone(),
                declared: regions.join(", "),
            });
        }
    }

    Ok(PassPlan {
        passes,
        regions,
        layout,
    })
}

/// Extracts the body of the first ```` ```lang-passes ```` fenced block in
/// `src`, or `None` if there is none.
fn extract_lang_passes_block(src: &str) -> Result<Option<String>, LangDocError> {
    let mut lines = src.lines();
    let mut found = false;
    for line in lines.by_ref() {
        if line.trim_end() == LANG_PASSES_FENCE {
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
            return Ok(Some(body));
        }
        body.push_str(line);
        body.push('\n');
    }
    Err(LangDocError::MalformedPasses {
        detail: "unterminated ```lang-passes``` block".to_string(),
    })
}

/// Validates that every `pass:` / `region:` annotation across the whole slot
/// graph names a pass/region DECLARED in the `## Passes` section. When there is
/// no `## Passes` section (an empty [`PassPlan`]), ANY annotation is an error —
/// annotations are meaningless without declared passes/regions, so a stray one
/// is caught loudly rather than silently ignored (which would break the
/// no-pass byte-identity guarantee subtly).
fn validate_annotations(
    plan: &PassPlan,
    function: &FunctionDef,
    items: &HashMap<ItemKind, ItemDef>,
) -> Result<(), LangDocError> {
    let check = |scope: &RuleScope| -> Result<(), LangDocError> {
        if let Some(pass) = &scope.pass {
            if !plan.passes.contains(pass) {
                return Err(LangDocError::UndeclaredAnnotation {
                    kind: "pass".to_string(),
                    name: pass.clone(),
                    declared: plan.passes.join(", "),
                });
            }
        }
        if let Some(region) = &scope.region {
            if !plan.regions.contains(region) {
                return Err(LangDocError::UndeclaredAnnotation {
                    kind: "region".to_string(),
                    name: region.clone(),
                    declared: plan.regions.join(", "),
                });
            }
        }
        Ok(())
    };

    let check_slots = |slots: &HashMap<String, SlotDef>| -> Result<(), LangDocError> {
        for def in slots.values() {
            match def {
                SlotDef::Fixed { scope, .. } => check(scope)?,
                SlotDef::Table(table) => {
                    for row in &table.rows {
                        check(&row.scope)?;
                    }
                }
            }
        }
        Ok(())
    };

    check_slots(&function.slots)?;
    for item in items.values() {
        check_slots(&item.slots)?;
    }
    Ok(())
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
            // A slot subsection may open with `pass:` / `region:` annotation
            // lines (before its `template` block or `When` table). Strip and
            // parse them into the slot-level scope; the remaining body is the
            // template/table.
            let (scope, remaining) = split_slot_annotations(name, body)?;
            let def = parse_slot_body(name, &remaining, scope)?;
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
/// is present (a fixed render outcome, carrying `scope`), otherwise a `When`
/// table. The `scope` is the slot-level `pass:` / `region:` annotation parsed
/// from the subsection heading region; it applies to a fixed slot (a `When`
/// table carries its annotations per row instead).
fn parse_slot_body(name: &str, body: &str, scope: RuleScope) -> Result<SlotDef, LangDocError> {
    if let Some(template_str) = extract_template_block(body)? {
        let template = Template::parse(&template_str).map_err(|e| LangDocError::BadTemplate {
            table: name.to_string(),
            detail: e.to_string(),
        })?;
        return Ok(SlotDef::Fixed {
            outcome: Outcome::Render(template),
            scope,
        });
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
    // A slot-level annotation on a `When`-table slot is not meaningful — a table
    // scopes per row. Reject it explicitly so a misplaced annotation is a loud
    // load-time error rather than silently ignored.
    if scope != RuleScope::none() {
        return Err(LangDocError::MalformedRow {
            table: name.to_string(),
            line: "a slot-level `pass:`/`region:` annotation may only appear on a fixed \
                   `template` slot; annotate individual When-table rows instead"
                .to_string(),
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
    // Seed with the entry template's referenced slots, in the section's scope.
    let work: Vec<(String, SlotScope)> = entry
        .slot_names()
        .iter()
        .map(|s| (s.to_string(), scope))
        .collect();
    validate_slots_from(work, slots)
}

/// Validates the declarative tree core's shared helper slots (hosted under
/// `## Tree`). A tree renders through the `### expr` dispatch, so validation is
/// seeded from the `expr` slot in expression scope; the transitive closure then
/// reaches `### attr` / `### child` (via the `attrs` / `children` sequence
/// bindings) and any node sub-slots the target names. An absent `### expr` is a
/// dangling reference exactly as at function scope.
fn validate_tree_slot_graph(slots: &HashMap<String, SlotDef>) -> Result<(), LangDocError> {
    validate_slots_from(vec![("expr".to_string(), SlotScope::Expr)], slots)
}

/// The shared slot-graph traversal used by both [`validate_slot_graph`] (seeded
/// from an entry template) and [`validate_tree_slot_graph`] (seeded from the
/// `### expr` tree entry point). Every reachable slot must resolve to an
/// engine-bound slot (in the scope it is referenced in) or a `### <slot>`
/// subsection, and any sequence-shaped slot must have its item subsection.
fn validate_slots_from(
    mut work: Vec<(String, SlotScope)>,
    slots: &HashMap<String, SlotDef>,
) -> Result<(), LangDocError> {
    let mut visited: Vec<(String, SlotScope)> = Vec::new();

    while let Some((name, scope)) = work.pop() {
        if visited.contains(&(name.clone(), scope)) {
            continue;
        }
        visited.push((name.clone(), scope));

        // Special slots (Part 1/2): a metadata slot `{meta.<key>}` or a call to
        // a closed render helper (`resolve_fnptr(...)`, `escape(..., style)`).
        // These are engine-provided and resolve at render time; they need no
        // `### <slot>` subsection and reference no further slots to validate.
        // The helper *set* is closed (recognized by name); an argument names an
        // engine sub-slot resolved within the current scope, so nothing further
        // is added to the work list.
        if is_special_slot(&name) {
            continue;
        }

        // A `{collection:item_slot}` projection: the base must be a Sequence
        // (collection) slot, the projected item slot must exist as a `### item`
        // subsection, and its references are validated in the collection's
        // element scope — exactly like the default item slot, only through the
        // chosen subsection. A `{name}` with no `:` returns `None` here and
        // falls through to the unchanged path below (byte-identical behavior).
        let (base, projection) = parse_slot_projection(&name);
        if let Some(item) = projection {
            match slot_binding(base, scope) {
                Some(SlotShape::Sequence { item_scope, .. }) => {
                    let def = slots.get(item).ok_or_else(|| {
                        LangDocError::MissingProjectionItemSlot {
                            collection: base.to_string(),
                            item: item.to_string(),
                        }
                    })?;
                    for r in referenced_by(def) {
                        work.push((r, item_scope));
                    }
                }
                // Projection is only meaningful on a collection: a scalar
                // engine-bound slot, or a named helper slot, cannot be looped.
                _ => {
                    return Err(LangDocError::ProjectionOnNonCollection {
                        slot: base.to_string(),
                        item: item.to_string(),
                    })
                }
            }
            continue;
        }

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
                let def = slots
                    .get(&item_slot)
                    .ok_or_else(|| LangDocError::MissingItemSlot {
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
                let def = slots
                    .get(&name)
                    .ok_or_else(|| LangDocError::UnknownSlotReference { slot: name.clone() })?;
                for r in referenced_by(def) {
                    work.push((r, scope));
                }
            }
        }
    }
    Ok(())
}

/// Returns `true` if `name` is a special (engine-provided) slot form rather
/// than an engine-bound AST slot or a `### <slot>` subsection: a metadata slot
/// `meta.<key>`, or a call to a closed render helper (`resolve_fnptr(<arg>)` /
/// `escape(<arg>, <style>)`). These resolve at render time and need no
/// subsection, so slot-graph validation treats them as satisfied.
///
/// The helper set is CLOSED: only these names are recognized. A misspelled
/// helper (e.g. `resolv_fnptr(x)`) does not match and falls through to the
/// normal path, which reports it as an unknown slot reference.
fn is_special_slot(name: &str) -> bool {
    (name.starts_with("meta.") && name.len() > "meta.".len())
        || (name.starts_with("resolve_fnptr(") && name.ends_with(')'))
        || (name.starts_with("escape(") && name.ends_with(')'))
        || (name.starts_with("field_type(") && name.ends_with(')'))
        || (name.starts_with("fresh_name(") && name.ends_with(')'))
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
        SlotDef::Fixed { outcome, .. } => add(outcome, &mut out),
        SlotDef::Table(table) => {
            for row in &table.rows {
                add(&row.outcome, &mut out);
            }
        }
    }
    out
}

/// Splits a slot subsection `body` into its leading `pass:` / `region:`
/// annotation lines (parsed into a [`RuleScope`]) and the remaining body (the
/// `template` block or `When` table).
///
/// Annotation lines are the load-bearing lines that appear *before* the first
/// `template` fence or pipe-table row and match `pass: <name>` or
/// `region: <name>`. Any other non-blank, non-annotation content before the
/// body (free prose) is left in the remaining body untouched — it is ignored by
/// the template/table extractors exactly as today. Only lines that look like an
/// annotation (`pass:`/`region:` prefix) are consumed and validated.
fn split_slot_annotations(name: &str, body: &str) -> Result<(RuleScope, String), LangDocError> {
    let mut scope = RuleScope::none();
    let mut remaining = String::new();
    let mut in_body = false;
    for line in body.lines() {
        let trimmed = line.trim();
        // Once the template/table body starts, copy everything verbatim (a
        // later line that happens to read like `pass:` is body content).
        if !in_body {
            if trimmed.starts_with("```") || trimmed.starts_with('|') {
                in_body = true;
            } else if let Some(kv) = parse_annotation_line(trimmed) {
                apply_annotation(name, &mut scope, kv)?;
                continue;
            }
        }
        remaining.push_str(line);
        remaining.push('\n');
    }
    Ok((scope, remaining))
}

/// Parses a single slot-annotation line `pass: <name>` / `region: <name>` into
/// a `(key, value)` pair, or `None` if the line is not an annotation.
fn parse_annotation_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key == "pass" || key == "region" {
        Some((key, value.trim()))
    } else {
        None
    }
}

/// Applies one parsed `(key, value)` annotation to `scope`, rejecting an empty
/// value or a duplicate key (a loud load-time error, never silently ignored).
fn apply_annotation(
    name: &str,
    scope: &mut RuleScope,
    (key, value): (&str, &str),
) -> Result<(), LangDocError> {
    if value.is_empty() {
        return Err(LangDocError::MalformedRow {
            table: name.to_string(),
            line: format!("empty `{key}:` annotation value"),
        });
    }
    match key {
        "pass" if scope.pass.is_none() => scope.pass = Some(value.to_string()),
        "region" if scope.region.is_none() => scope.region = Some(value.to_string()),
        _ => {
            return Err(LangDocError::MalformedRow {
                table: name.to_string(),
                line: format!("duplicate or unknown `{key}:` annotation"),
            })
        }
    }
    Ok(())
}

/// Parses the trailing annotation cells of a `When`-table row (the cells beyond
/// the predicate and template) into a [`RuleScope`]. Each cell must be a
/// `pass: <name>` / `region: <name>` annotation; a stray non-annotation cell is
/// a loud error. An empty slice yields the unannotated scope (legacy behavior).
fn parse_rule_scope_cells(name: &str, cells: &[String]) -> Result<RuleScope, LangDocError> {
    let mut scope = RuleScope::none();
    for cell in cells {
        let cell = cell.trim();
        if cell.is_empty() {
            continue;
        }
        match parse_annotation_line(cell) {
            Some(kv) => apply_annotation(name, &mut scope, kv)?,
            None => {
                return Err(LangDocError::MalformedRow {
                    table: name.to_string(),
                    line: format!("unexpected trailing cell `{cell}` (expected `pass:`/`region:`)"),
                })
            }
        }
    }
    Ok(scope)
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
        // Any trailing cells carry the optional `pass:` / `region:` annotations
        // scoping this row. A plain two-cell row has none (legacy behavior).
        let scope = parse_rule_scope_cells(name, &cells[2..])?;

        parsed.push(WhenRow {
            predicate,
            outcome,
            scope,
        });
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
        let is_ident =
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_ident {
            let template =
                Template::parse(&format!("{{{name}}}")).map_err(|e| LangDocError::BadTemplate {
                    table: table.to_string(),
                    detail: e.to_string(),
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
pub fn parse_capability_table(table: &str) -> Result<HashMap<Primitive, Capability>, LangDocError> {
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

    /// A minimal valid `lang-meta` header block for building standalone test
    /// docs that must parse past the (now required) versioning header.
    const META: &str =
        "```lang-meta\nlamina-format: 0.0.0\ntarget: x\ntarget-version: test\n```\n\n";

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
            "# Lamina Language Definition: rust\n\n\
             ```lang-meta\nlamina-format: 0.0.0\ntarget: rust\ntarget-version: 2021\n```\n\n\
             ## Function\n\n{function_section}\n{FULL_CAPS}"
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
                match &table.select(&void_ctx, None).expect("void row").outcome {
                    Outcome::Render(t) => assert!(t.slot_names().is_empty()),
                    Outcome::Forbid => panic!("void row should render"),
                }
                let type_ctx = RenderContext {
                    ret: Some(RetKind::Type),
                    ..Default::default()
                };
                match &table.select(&type_ctx, None).expect("else row").outcome {
                    Outcome::Render(t) => assert!(t.slot_names().contains(&"ret_type")),
                    Outcome::Forbid => panic!("else row should render"),
                }
            }
            SlotDef::Fixed { .. } => panic!("ret should be a table"),
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
            SlotDef::Fixed {
                outcome: Outcome::Render(_),
                ..
            } => {}
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
                assert_eq!(
                    table.select(&priv_ctx, None).map(|r| &r.outcome),
                    Some(&Outcome::Forbid)
                );
            }
            SlotDef::Fixed { .. } => panic!("vis should be a table"),
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
        assert!(
            parse_language_def(&ok).is_ok(),
            "resolvable @ref should parse"
        );

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
                assert!(table.select(&pub_ctx, None).is_some());
            }
            SlotDef::Fixed { .. } => panic!("vis should be a table"),
        }
    }

    #[test]
    fn rejects_incomplete_capability_matrix() {
        // Only i32 present -> 25 missing. Entry uses only scalar terminals so
        // slot-graph validation passes and the completeness check is reached.
        let doc = format!(
            "# Lamina Language Definition: x\n\n{META}## Function\n\n\
            ```template\nfn {{name}}()\n```\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n"
        );
        match parse_language_def(&doc) {
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
        let doc =
            "## Function\n\n```template\n{name}\n```\n## Capabilities\n| i32 | identity | i32 |\n";
        assert!(matches!(
            parse_language_def(doc),
            Err(LangDocError::MissingTitle { .. })
        ));
    }

    #[test]
    fn rejects_missing_entry_template() {
        let doc = format!("# Lamina Language Definition: x\n\n{META}## Function\n\n### ret\n| When | Template |\n| else | \"\" |\n\n## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n");
        assert!(matches!(
            parse_language_def(&doc),
            Err(LangDocError::MissingTable { .. })
        ));
    }

    #[test]
    fn rejects_dangling_slot_reference() {
        // Entry references {ret} but there is no ### ret subsection and `ret`
        // is not a terminal slot.
        let doc = format!("# Lamina Language Definition: x\n\n{META}## Function\n\n\
            ```template\nfn {{name}}(){{ret}}\n```\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n");
        assert_eq!(
            parse_language_def(&doc),
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
    fn projected_collection_slot_validates_through_named_item_slot() {
        // `{params:param_type}` loops the `params` collection through the
        // author-named `### param_type` subsection (in Param scope, so it may
        // use `{type}`). A default `### param` still satisfies the plain
        // `{params}` reference. Both projections must validate.
        let doc = mk("```template\n\
            fn {name}({params}) sig({params:param_type})\n\
            ```\n\
            \n\
            ### param\n\
            | When  | Template |\n\
            |-------|----------|\n\
            | first | \"{name}\" |\n\
            | else  | \", {name}\" |\n\
            \n\
            ### param_type\n\
            | When  | Template |\n\
            |-------|----------|\n\
            | first | \"{type}\" |\n\
            | else  | \" -> {type}\" |");
        assert!(
            parse_language_def(&doc).is_ok(),
            "a projection through an existing item slot must validate"
        );
    }

    #[test]
    fn projection_naming_missing_item_slot_is_load_error() {
        // `{params:param_type}` with no `### param_type` subsection is a
        // load-time error naming the collection and the missing item slot.
        let doc = mk("```template\n\
            fn {name}({params}) sig({params:param_type})\n\
            ```\n\
            \n\
            ### param\n\
            | When  | Template |\n\
            |-------|----------|\n\
            | first | \"{name}\" |\n\
            | else  | \", {name}\" |");
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingProjectionItemSlot {
                collection: "params".to_string(),
                item: "param_type".to_string(),
            })
        );
    }

    #[test]
    fn projection_on_non_collection_slot_is_load_error() {
        // `{name:foo}` projects a SCALAR slot (`name`), which is not a
        // collection — a load-time error. Projection only selects which item
        // template a collection loops with.
        let doc = mk("```template\nfn {name:foo}()\n```");
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::ProjectionOnNonCollection {
                slot: "name".to_string(),
                item: "foo".to_string(),
            })
        );
    }

    #[test]
    fn rejects_table_without_else() {
        let doc = format!("# Lamina Language Definition: x\n\n{META}## Function\n\n\
            ```template\nfn {{name}}(){{ret}}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| ret is void | \"\" |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n");
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingElseRow {
                table: "ret".to_string()
            })
        );
    }

    #[test]
    fn rejects_unquoted_template_in_table() {
        let doc = format!("# Lamina Language Definition: x\n\n{META}## Function\n\n\
            ```template\nfn {{name}}(){{ret}}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| else | bare |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n");
        assert!(matches!(
            parse_language_def(&doc),
            Err(LangDocError::UnquotedTemplate { .. })
        ));
    }

    #[test]
    fn rejects_bad_predicate() {
        let doc = format!("# Lamina Language Definition: x\n\n{META}## Function\n\n\
            ```template\nfn {{name}}(){{ret}}\n```\n\n\
            ### ret\n| When | Template |\n|------|----------|\n| frobnicate | \"x\" |\n| else | \"y\" |\n\n\
            ## Capabilities\n| Primitive | Action | Target |\n| i32 | identity | i32 |\n");
        assert!(matches!(
            parse_language_def(&doc),
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
    fn declarative_only_def_omits_function_section() {
        // A def with a `## Tree` section and NO `## Function` loads: the tree
        // helper slots are merged into the function slot map (which the
        // expression resolver reads), and the entry template is empty.
        let doc = format!(
            "# Lamina Language Definition: html\n\n{META}\
             ## Tree\n\n\
             ### expr\n\
             | When         | Template |\n\
             |--------------|----------|\n\
             | expr is node | @element |\n\
             | expr is text | \"{{value}}\" |\n\
             | else         | forbid |\n\n\
             ### element\n\
             ```template\n\
             <{{node_name}}{{attrs}}>{{children}}</{{node_name}}>\n\
             ```\n\n\
             ### attr\n\
             | When | Template |\n\
             |------|----------|\n\
             | else | \" {{name}}\" |\n\n\
             ### child\n\
             | When | Template |\n\
             |------|----------|\n\
             | else | \"{{value}}\" |\n\n\
             {FULL_CAPS}"
        );
        let def = parse_language_def(&doc).expect("declarative-only def parses");
        assert_eq!(def.name, "html");
        // The tree helper slots landed in the function slot map.
        assert!(def.function.slots.contains_key("expr"));
        assert!(def.function.slots.contains_key("element"));
        assert!(def.function.slots.contains_key("attr"));
        assert!(def.function.slots.contains_key("child"));
        // No imperative entry, so the entry template is empty (no slots).
        assert!(def.function.entry.slot_names().is_empty());
    }

    #[test]
    fn def_with_neither_function_nor_tree_is_rejected() {
        let doc = format!(
            "# Lamina Language Definition: empty\n\n{META}{FULL_CAPS}"
        );
        assert!(matches!(
            parse_language_def(&doc),
            Err(LangDocError::MissingSection { .. })
        ));
    }

    #[test]
    fn tree_section_dangling_slot_is_a_load_error() {
        // A `### expr` node row references `@element` but there is no
        // `### element` subsection -> load-time dangling-reference error.
        let doc = format!(
            "# Lamina Language Definition: html\n\n{META}\
             ## Tree\n\n\
             ### expr\n\
             | When         | Template |\n\
             |--------------|----------|\n\
             | expr is node | @element |\n\
             | else         | forbid |\n\n\
             {FULL_CAPS}"
        );
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::UnknownSlotReference {
                slot: "element".to_string()
            })
        );
    }

    // ---- lang-meta versioning ------------------------------------------

    #[test]
    fn parses_target_and_target_version_verbatim() {
        // The `target` / `target-version` are carried verbatim onto the def; the
        // engine performs no logic on them. `mk` uses target `rust` / `2021`.
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        assert_eq!(def.target, "rust");
        assert_eq!(def.target_version, "2021");
        assert_eq!(def.target_version(), "2021");
    }

    #[test]
    fn opaque_target_version_is_not_parsed() {
        // An arbitrary, non-semver band string is accepted verbatim: the engine
        // never parses or validates `target-version`.
        let doc = format!(
            "# Lamina Language Definition: py\n\n\
             ```lang-meta\nlamina-format: 0.0.0\ntarget: python\ntarget-version: >=3.0 (cpython)\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        let def = parse_language_def(&doc).expect("opaque band parses");
        assert_eq!(def.target_version, ">=3.0 (cpython)");
    }

    #[test]
    fn missing_lang_meta_block_is_a_load_error() {
        // A def with a title but no `lang-meta` block is refused.
        let doc = format!(
            "# Lamina Language Definition: x\n\n## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert_eq!(parse_language_def(&doc), Err(LangDocError::MissingLangMeta));
    }

    #[test]
    fn missing_lamina_format_key_is_a_load_error() {
        let doc = format!(
            "# Lamina Language Definition: x\n\n\
             ```lang-meta\ntarget: x\ntarget-version: test\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::MissingLangMetaKey {
                key: "lamina-format".to_string()
            })
        );
    }

    #[test]
    fn missing_target_keys_are_load_errors() {
        let no_target = format!(
            "# Lamina Language Definition: x\n\n\
             ```lang-meta\nlamina-format: 0.0.0\ntarget-version: test\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert_eq!(
            parse_language_def(&no_target),
            Err(LangDocError::MissingLangMetaKey {
                key: "target".to_string()
            })
        );
        let no_version = format!(
            "# Lamina Language Definition: x\n\n\
             ```lang-meta\nlamina-format: 0.0.0\ntarget: x\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert_eq!(
            parse_language_def(&no_version),
            Err(LangDocError::MissingLangMetaKey {
                key: "target-version".to_string()
            })
        );
    }

    #[test]
    fn unknown_lang_meta_key_is_a_load_error() {
        let doc = format!(
            "# Lamina Language Definition: x\n\n\
             ```lang-meta\nlamina-format: 0.0.0\ntarget: x\ntarget-version: t\nfrobnicate: y\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert_eq!(
            parse_language_def(&doc),
            Err(LangDocError::UnknownLangMetaKey {
                key: "frobnicate".to_string()
            })
        );
    }

    #[test]
    fn malformed_format_version_is_a_load_error() {
        for bad in ["1.2", "1.2.x", "1.2.3.4", "1..3", "v1.2.3"] {
            let doc = format!(
                "# Lamina Language Definition: x\n\n\
                 ```lang-meta\nlamina-format: {bad}\ntarget: x\ntarget-version: t\n```\n\n\
                 ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
            );
            assert!(
                matches!(
                    parse_language_def(&doc),
                    Err(LangDocError::MalformedFormatVersion { .. })
                ),
                "expected malformed-version error for {bad:?}"
            );
        }
    }

    #[test]
    fn empty_lang_meta_value_is_a_malformed_line() {
        // An empty value (`lamina-format:` with nothing after) is a malformed
        // header line, caught before version parsing.
        let doc = format!(
            "# Lamina Language Definition: x\n\n\
             ```lang-meta\nlamina-format:\ntarget: x\ntarget-version: t\n```\n\n\
             ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
        );
        assert!(matches!(
            parse_language_def(&doc),
            Err(LangDocError::MalformedLangMetaLine { .. })
        ));
    }

    #[test]
    fn format_version_newer_than_engine_is_refused() {
        // Engine is 0.0.0, so any strictly-newer minimum is refused.
        for newer in ["0.0.1", "0.1.0", "1.0.0"] {
            let doc = format!(
                "# Lamina Language Definition: x\n\n\
                 ```lang-meta\nlamina-format: {newer}\ntarget: x\ntarget-version: t\n```\n\n\
                 ## Function\n\n{FN_SECTION}\n{FULL_CAPS}"
            );
            match parse_language_def(&doc) {
                Err(LangDocError::FormatVersionTooNew { required, engine }) => {
                    assert_eq!(required, newer);
                    assert_eq!(engine, LAMINA_FORMAT_VERSION);
                }
                other => panic!("expected FormatVersionTooNew for {newer:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn format_version_equal_to_engine_loads() {
        // Equal (0.0.0) is backward-compatible and loads.
        let def = parse_language_def(&mk(FN_SECTION)).expect("equal version loads");
        assert_eq!(def.name, "rust");
    }

    #[test]
    fn version_compare_orders_by_tuple() {
        use crate::lang::Version;
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert!(Version::parse("0.1.0").unwrap() > Version::parse("0.0.9").unwrap());
        assert!(Version::parse("0.0.2").unwrap() > Version::parse("0.0.1").unwrap());
        assert_eq!(
            Version::parse("2.3.4").unwrap(),
            Version::parse("2.3.4").unwrap()
        );
        assert!(Version::parse("0.0.0").unwrap() <= Version::parse("0.0.0").unwrap());
    }

    // ---- passes / regions --------------------------------------------------

    /// A `## Function` body that declares a two-pass, two-region plan and a
    /// `### statement` slot whose rows carry `pass:` / `region:` annotations.
    const PASSES_SECTION: &str = "```template\n{body}\n```\n\
        \n\
        ### statement\n\
        | When          | Template      | annotations |\n\
        |---------------|---------------|-------------|\n\
        | stmt is while | \"H\"           | pass: collect | region: helpers |\n\
        | else          | \"\"            | pass: collect |\n\
        | else          | \"{stmt}\"      | pass: emit |\n\
        \n\
        ### stmt\n\
        | When | Template |\n\
        |------|----------|\n\
        | else | \"s\" |\n\
        \n\
        ## Passes\n\n\
        ```lang-passes\n\
        passes: collect, emit\n\
        regions: helpers, body\n\
        layout: helpers, body\n\
        ```\n";

    #[test]
    fn passes_section_parses_plan_and_annotations() {
        let def = parse_language_def(&mk(PASSES_SECTION)).expect("parses with passes");
        assert!(def.passes.is_multipass());
        assert_eq!(def.passes.passes, vec!["collect", "emit"]);
        assert_eq!(def.passes.regions, vec!["helpers", "body"]);
        assert_eq!(def.passes.layout, vec!["helpers", "body"]);

        // The `### statement` table's rows carry the parsed annotations.
        match def.function.slots.get("statement").expect("statement slot") {
            SlotDef::Table(table) => {
                let first = &table.rows[0];
                assert_eq!(first.scope.pass.as_deref(), Some("collect"));
                assert_eq!(first.scope.region.as_deref(), Some("helpers"));
                let second = &table.rows[1];
                assert_eq!(second.scope.pass.as_deref(), Some("collect"));
                assert_eq!(second.scope.region, None);
                let third = &table.rows[2];
                assert_eq!(third.scope.pass.as_deref(), Some("emit"));
            }
            SlotDef::Fixed { .. } => panic!("statement should be a table"),
        }
    }

    #[test]
    fn absent_passes_section_is_not_multipass() {
        let def = parse_language_def(&mk(FN_SECTION)).expect("parses");
        assert!(!def.passes.is_multipass());
        assert!(def.passes.passes.is_empty());
    }

    #[test]
    fn annotation_without_passes_section_is_rejected() {
        // A `region:` annotation with NO `## Passes` section is a loud error —
        // annotations are meaningless without declared passes/regions.
        let section = "```template\n{body}\n```\n\
            \n\
            ### statement\n\
            | When | Template | region |\n\
            |------|----------|--------|\n\
            | else | \"{stmt}\" | region: helpers |\n\
            \n\
            ### stmt\n\
            | When | Template |\n\
            |------|----------|\n\
            | else | \"s\" |\n";
        assert!(matches!(
            parse_language_def(&mk(section)),
            Err(LangDocError::UndeclaredAnnotation { .. })
        ));
    }

    #[test]
    fn layout_naming_undeclared_region_is_rejected() {
        let section = "```template\n{body}\n```\n\
            \n\
            ### statement\n\
            ```template\ns\n```\n\
            \n\
            ## Passes\n\n\
            ```lang-passes\n\
            passes: only\n\
            regions: a\n\
            layout: a, ghost\n\
            ```\n";
        assert!(matches!(
            parse_language_def(&mk(section)),
            Err(LangDocError::UndeclaredLayoutRegion { .. })
        ));
    }

    #[test]
    fn annotation_naming_undeclared_pass_is_rejected() {
        let section = "```template\n{body}\n```\n\
            \n\
            ### statement\n\
            | When | Template | pass |\n\
            |------|----------|------|\n\
            | else | \"s\" | pass: nope |\n\
            \n\
            ## Passes\n\n\
            ```lang-passes\n\
            passes: only\n\
            regions: body\n\
            layout: body\n\
            ```\n";
        assert!(matches!(
            parse_language_def(&mk(section)),
            Err(LangDocError::UndeclaredAnnotation { .. })
        ));
    }

    #[test]
    fn passes_block_missing_key_is_rejected() {
        let section = "```template\n{body}\n```\n\
            \n\
            ### statement\n\
            ```template\ns\n```\n\
            \n\
            ## Passes\n\n\
            ```lang-passes\n\
            passes: only\n\
            regions: body\n\
            ```\n";
        assert!(matches!(
            parse_language_def(&mk(section)),
            Err(LangDocError::MalformedPasses { .. })
        ));
    }

    #[test]
    fn passes_block_unknown_key_is_rejected() {
        let section = "```template\n{body}\n```\n\
            \n\
            ### statement\n\
            ```template\ns\n```\n\
            \n\
            ## Passes\n\n\
            ```lang-passes\n\
            passes: only\n\
            regions: body\n\
            layout: body\n\
            phases: x\n\
            ```\n";
        assert!(matches!(
            parse_language_def(&mk(section)),
            Err(LangDocError::MalformedPasses { .. })
        ));
    }

    #[test]
    fn slot_level_annotation_on_fixed_template_parses() {
        // A slot subsection may carry a slot-level `region:` annotation before
        // its `template` block; it attaches to the fixed slot.
        let section = "```template\n{pre}{body}\n```\n\
            \n\
            ### pre\n\
            region: helpers\n\
            ```template\nP\n```\n\
            \n\
            ### statement\n\
            ```template\ns\n```\n\
            \n\
            ## Passes\n\n\
            ```lang-passes\n\
            passes: only\n\
            regions: helpers, body\n\
            layout: helpers, body\n\
            ```\n";
        let def = parse_language_def(&mk(section)).expect("parses");
        match def.function.slots.get("pre").expect("pre slot") {
            SlotDef::Fixed { scope, .. } => {
                assert_eq!(scope.region.as_deref(), Some("helpers"));
            }
            SlotDef::Table(_) => panic!("pre should be a fixed slot"),
        }
    }
}
