//! Parser for rigid `.mdl` language-definition documents.
//!
//! A language definition is authored as a markdown-*compatible* document, but
//! the format is **strictly validated** - it is not free-form markdown. The
//! parser enforces a fixed shape and fails loudly on anything malformed:
//!
//! ~~~text
//! # Lamina Language Definition: <name>
//!
//! <free prose, ignored by the parser>
//!
//! ## Function
//! ```lang-function
//! keyword = "<kw>"
//! return_type_sep = "<sep>"
//! emit_return_type = true|false
//! ```
//!
//! ## Capabilities
//! ```lang-capabilities
//! <primitive> <action> [target-type]
//! ...
//! ```
//! ~~~
//!
//! Prose between sections is documentation (and future raise-input); it is not
//! validated. Only the title line and the typed fenced blocks are load-bearing.
//!
//! The engine ships with NO built-in language definitions - a definition is
//! always loaded from a document like this. That is what keeps the engine
//! "dumb": no target is hardcoded in core.

use std::collections::HashMap;

use crate::ast::Primitive;
use crate::error::LangDocError;
use crate::lang::{Capability, FunctionSyntax, LanguageDef};

const TITLE_PREFIX: &str = "# Lamina Language Definition:";
const FUNCTION_HEADING: &str = "## Function";
const CAPABILITIES_HEADING: &str = "## Capabilities";
const FUNCTION_BLOCK_TAG: &str = "lang-function";
const CAPABILITIES_BLOCK_TAG: &str = "lang-capabilities";

/// Parses a rigid `.mdl` language-definition document into a [`LanguageDef`].
///
/// # Errors
///
/// Returns a [`LangDocError`] if the document is missing a required element or
/// any block is malformed.
pub fn parse_language_def(src: &str) -> Result<LanguageDef, LangDocError> {
    let name = parse_title(src)?;

    require_heading(src, FUNCTION_HEADING)?;
    require_heading(src, CAPABILITIES_HEADING)?;

    let function_lines = extract_block(src, FUNCTION_BLOCK_TAG)?;
    let function_syntax = parse_function_block(&function_lines)?;

    let capability_lines = extract_block(src, CAPABILITIES_BLOCK_TAG)?;
    let capabilities = parse_capabilities_block(&capability_lines)?;

    Ok(LanguageDef {
        name,
        capabilities,
        function_syntax,
    })
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

/// Extracts the body lines of the fenced block opened by ```` ```<tag> ````.
///
/// Returns the lines between the opening and closing fences. Errors if the
/// block is absent or unterminated.
fn extract_block(src: &str, tag: &str) -> Result<Vec<String>, LangDocError> {
    let opening = format!("```{tag}");
    let mut lines = src.lines();
    let mut found_open = false;

    // Find the opening fence. The fence's info string must equal the tag
    // exactly (after the backticks) so `lang-capabilities` does not match a
    // hypothetical `lang-capabilities-extra`.
    for line in lines.by_ref() {
        if line.trim_end() == opening {
            found_open = true;
            break;
        }
    }
    if !found_open {
        return Err(LangDocError::MissingBlock {
            tag: tag.to_string(),
        });
    }

    let mut body = Vec::new();
    for line in lines.by_ref() {
        if line.trim_end() == "```" {
            return Ok(body);
        }
        body.push(line.to_string());
    }

    Err(LangDocError::UnterminatedBlock {
        tag: tag.to_string(),
    })
}

/// Parses the `lang-function` block into a [`FunctionSyntax`].
fn parse_function_block(lines: &[String]) -> Result<FunctionSyntax, LangDocError> {
    let mut map: HashMap<String, String> = HashMap::new();

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| LangDocError::MalformedLine {
            tag: FUNCTION_BLOCK_TAG.to_string(),
            line: raw.clone(),
        })?;
        map.insert(key.trim().to_string(), value.trim().to_string());
    }

    let keyword = take_string(&map, "keyword", FUNCTION_BLOCK_TAG)?;
    let return_type_sep = take_string(&map, "return_type_sep", FUNCTION_BLOCK_TAG)?;
    let emit_return_type = take_bool(&map, "emit_return_type")?;

    Ok(FunctionSyntax {
        keyword,
        return_type_sep,
        emit_return_type,
    })
}

/// Reads a required double-quoted string value from a parsed key/value map.
fn take_string(
    map: &HashMap<String, String>,
    key: &str,
    tag: &str,
) -> Result<String, LangDocError> {
    let value = map.get(key).ok_or_else(|| LangDocError::MissingKey {
        key: key.to_string(),
        tag: tag.to_string(),
    })?;
    unquote(key, value)
}

/// Reads a required boolean value (`true`/`false`) from a parsed key/value map.
fn take_bool(map: &HashMap<String, String>, key: &str) -> Result<bool, LangDocError> {
    let value = map.get(key).ok_or_else(|| LangDocError::MissingKey {
        key: key.to_string(),
        tag: FUNCTION_BLOCK_TAG.to_string(),
    })?;
    match value.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(LangDocError::InvalidBool {
            key: key.to_string(),
            value: value.clone(),
        }),
    }
}

/// Strips the surrounding double quotes from a string value, preserving any
/// interior whitespace. Errors if the value is not properly quoted.
fn unquote(key: &str, value: &str) -> Result<String, LangDocError> {
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        Ok(value[1..value.len() - 1].to_string())
    } else {
        Err(LangDocError::UnquotedString {
            key: key.to_string(),
            value: value.to_string(),
        })
    }
}

/// Parses the `lang-capabilities` block into a capability matrix.
fn parse_capabilities_block(
    lines: &[String],
) -> Result<HashMap<Primitive, Capability>, LangDocError> {
    let mut capabilities = HashMap::new();

    for raw in lines {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let primitive_name = parts.next().ok_or_else(|| LangDocError::MalformedLine {
            tag: CAPABILITIES_BLOCK_TAG.to_string(),
            line: raw.clone(),
        })?;
        let action = parts.next().ok_or_else(|| LangDocError::MalformedLine {
            tag: CAPABILITIES_BLOCK_TAG.to_string(),
            line: raw.clone(),
        })?;
        let target = parts.next();

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

/// Parses a capability matrix authored as a markdown pipe table.
///
/// This is the locked capability-matrix format: a table with a header row and a
/// separator row, then one data row per primitive:
///
/// ~~~text
/// | Primitive | Action   | Target | Notes |
/// |-----------|----------|--------|-------|
/// | i32       | identity | i32    | |
/// | str       | wrap     | String | optional prose |
/// ~~~
///
/// The `Notes` column is free prose and is ignored. Rows for `forbid` omit the
/// target. Header and separator rows are skipped.
///
/// # Errors
///
/// Returns a [`LangDocError`] if a row is malformed, names an unknown primitive
/// or action, or omits a required target.
pub fn parse_capability_table(
    table: &str,
) -> Result<HashMap<Primitive, Capability>, LangDocError> {
    let mut capabilities = HashMap::new();

    for raw in table.lines() {
        let line = raw.trim();
        // Only consider pipe-table rows.
        if !line.starts_with('|') {
            continue;
        }
        // Skip the separator row (cells made only of dashes/colons/spaces).
        let cells: Vec<&str> = line
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim())
            .collect();
        if cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        // Skip the header row (identified by the literal column name).
        if cells.first().map(|c| c.eq_ignore_ascii_case("primitive")) == Some(true) {
            continue;
        }

        if cells.len() < 2 {
            return Err(LangDocError::MalformedLine {
                tag: "capabilities-table".to_string(),
                line: raw.to_string(),
            });
        }

        let primitive_name = cells[0];
        let action = cells[1];
        let target = cells.get(2).copied().filter(|t| !t.is_empty());

        let primitive =
            Primitive::from_name(primitive_name).ok_or_else(|| LangDocError::UnknownPrimitive {
                name: primitive_name.to_string(),
            })?;
        let capability = build_capability(action, target, primitive_name)?;
        capabilities.insert(primitive, capability);
    }

    Ok(capabilities)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST_DOC: &str = "# Lamina Language Definition: rust\n\
        \n\
        Some prose describing the target.\n\
        \n\
        ## Function\n\
        ```lang-function\n\
        keyword = \"fn\"\n\
        return_type_sep = \" -> \"\n\
        emit_return_type = true\n\
        ```\n\
        \n\
        ## Capabilities\n\
        ```lang-capabilities\n\
        i32 identity i32\n\
        ```\n";

    #[test]
    fn parses_a_well_formed_document() {
        let def = parse_language_def(RUST_DOC).expect("should parse");
        assert_eq!(def.name, "rust");
        assert_eq!(def.function_syntax.keyword, "fn");
        assert_eq!(def.function_syntax.return_type_sep, " -> ");
        assert!(def.function_syntax.emit_return_type);
        assert_eq!(
            def.capability(Primitive::I32),
            Some(&Capability::Identity("i32".to_string()))
        );
    }

    #[test]
    fn preserves_significant_whitespace_in_quoted_values() {
        let def = parse_language_def(RUST_DOC).expect("parse");
        // The separator must retain its leading and trailing spaces.
        assert_eq!(def.function_syntax.return_type_sep, " -> ");
    }

    #[test]
    fn rejects_missing_title() {
        let doc = "## Function\n```lang-function\nkeyword = \"fn\"\n```\n";
        let err = parse_language_def(doc).expect_err("no title");
        assert!(matches!(err, LangDocError::MissingTitle { .. }));
    }

    #[test]
    fn rejects_wrong_title() {
        let doc = "# Some Other Title\n\n## Function\n";
        let err = parse_language_def(doc).expect_err("wrong title");
        assert!(matches!(err, LangDocError::MissingTitle { .. }));
    }

    #[test]
    fn rejects_missing_function_section() {
        let doc = "# Lamina Language Definition: x\n\n## Capabilities\n```lang-capabilities\ni32 identity i32\n```\n";
        let err = parse_language_def(doc).expect_err("no function section");
        assert_eq!(
            err,
            LangDocError::MissingSection {
                heading: "## Function".to_string()
            }
        );
    }

    #[test]
    fn rejects_missing_function_block() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n\n## Capabilities\n```lang-capabilities\ni32 identity i32\n```\n";
        let err = parse_language_def(doc).expect_err("no function block");
        assert_eq!(
            err,
            LangDocError::MissingBlock {
                tag: "lang-function".to_string()
            }
        );
    }

    #[test]
    fn rejects_unterminated_block() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\n\n## Capabilities\n";
        let err = parse_language_def(doc).expect_err("unterminated");
        assert_eq!(
            err,
            LangDocError::UnterminatedBlock {
                tag: "lang-function".to_string()
            }
        );
    }

    #[test]
    fn rejects_unquoted_string_value() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = fn\nreturn_type_sep = \" -> \"\nemit_return_type = true\n```\n\n## Capabilities\n```lang-capabilities\ni32 identity i32\n```\n";
        let err = parse_language_def(doc).expect_err("unquoted");
        assert!(matches!(err, LangDocError::UnquotedString { .. }));
    }

    #[test]
    fn rejects_invalid_bool() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\nreturn_type_sep = \" -> \"\nemit_return_type = yes\n```\n\n## Capabilities\n```lang-capabilities\ni32 identity i32\n```\n";
        let err = parse_language_def(doc).expect_err("bad bool");
        assert!(matches!(err, LangDocError::InvalidBool { .. }));
    }

    #[test]
    fn rejects_unknown_primitive() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\nreturn_type_sep = \" -> \"\nemit_return_type = true\n```\n\n## Capabilities\n```lang-capabilities\nq99 identity q99\n```\n";
        let err = parse_language_def(doc).expect_err("unknown primitive");
        assert_eq!(
            err,
            LangDocError::UnknownPrimitive {
                name: "q99".to_string()
            }
        );
    }

    #[test]
    fn rejects_unknown_action() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\nreturn_type_sep = \" -> \"\nemit_return_type = true\n```\n\n## Capabilities\n```lang-capabilities\ni32 transmute i32\n```\n";
        let err = parse_language_def(doc).expect_err("unknown action");
        assert_eq!(
            err,
            LangDocError::UnknownAction {
                action: "transmute".to_string()
            }
        );
    }

    #[test]
    fn rejects_missing_capability_target() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\nreturn_type_sep = \" -> \"\nemit_return_type = true\n```\n\n## Capabilities\n```lang-capabilities\ni32 identity\n```\n";
        let err = parse_language_def(doc).expect_err("missing target");
        assert_eq!(
            err,
            LangDocError::MissingCapabilityTarget {
                action: "identity".to_string(),
                primitive: "i32".to_string(),
            }
        );
    }

    #[test]
    fn forbid_needs_no_target() {
        let doc = "# Lamina Language Definition: x\n\n## Function\n```lang-function\nkeyword = \"fn\"\nreturn_type_sep = \" -> \"\nemit_return_type = true\n```\n\n## Capabilities\n```lang-capabilities\ni32 forbid\n```\n";
        let def = parse_language_def(doc).expect("forbid parses");
        assert_eq!(def.capability(Primitive::I32), Some(&Capability::Forbid));
    }

    // --- Markdown-table capability matrix (the locked format) ---

    #[test]
    fn table_parses_header_separator_and_rows() {
        let table = "\
| Primitive | Action   | Target | Notes |\n\
|-----------|----------|--------|-------|\n\
| i32       | identity | i32    | |\n";
        let caps = parse_capability_table(table).expect("table parses");
        assert_eq!(
            caps.get(&Primitive::I32),
            Some(&Capability::Identity("i32".to_string()))
        );
        // Header and separator rows must not produce entries.
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn table_parses_wrap_action_with_target() {
        // `str` is not a known primitive in this slice, so use i32 to exercise
        // the `wrap` action wiring itself.
        let table = "| i32 | wrap | Boxed |\n";
        let caps = parse_capability_table(table).expect("wrap parses");
        assert_eq!(
            caps.get(&Primitive::I32),
            Some(&Capability::Wrap("Boxed".to_string()))
        );
    }

    #[test]
    fn table_forbid_row_omits_target() {
        let table = "| i32 | forbid | |\n";
        let caps = parse_capability_table(table).expect("forbid parses");
        assert_eq!(caps.get(&Primitive::I32), Some(&Capability::Forbid));
    }

    #[test]
    fn table_notes_column_is_ignored() {
        let table = "| i32 | identity | i32 | some human prose here |\n";
        let caps = parse_capability_table(table).expect("parses with notes");
        assert_eq!(
            caps.get(&Primitive::I32),
            Some(&Capability::Identity("i32".to_string()))
        );
    }

    #[test]
    fn table_rejects_unknown_action() {
        let table = "| i32 | transmute | i32 |\n";
        let err = parse_capability_table(table).expect_err("unknown action");
        assert_eq!(
            err,
            LangDocError::UnknownAction {
                action: "transmute".to_string()
            }
        );
    }

    #[test]
    fn table_rejects_missing_target_for_nonforbid() {
        let table = "| i32 | identity | |\n";
        let err = parse_capability_table(table).expect_err("missing target");
        assert_eq!(
            err,
            LangDocError::MissingCapabilityTarget {
                action: "identity".to_string(),
                primitive: "i32".to_string(),
            }
        );
    }

    #[test]
    fn table_ignores_non_table_prose() {
        let table = "\
Some prose before the table.\n\
\n\
| i32 | identity | i32 |\n\
\n\
More prose after.\n";
        let caps = parse_capability_table(table).expect("parses around prose");
        assert_eq!(caps.len(), 1);
    }
}
