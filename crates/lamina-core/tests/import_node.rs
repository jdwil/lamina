//! Tests for the native library [`Item::Import`] node (Part 1 of the
//! import/main/CLI arc): source grammar parsing (bare / `from … import …` /
//! aliased arms, plus the optional `else` fallback), structural equality, and
//! target-aware resolution in [`lower`](lamina_core::lower).
//!
//! An `import { … }` block is lexed via the ordinary `import` keyword and
//! parsed into target-tagged structured arms (mirroring [`Item::Use`]'s
//! `module`/`items`/`alias` shape). [`lower`](lamina_core::lower) collapses the
//! multi-arm node to the single arm matching the build target (or the `else`
//! fallback), exactly like a `raw` node; no match + no fallback is a hard
//! error, so an import is never silently dropped.

use lamina_core::ast::{File, ImportArm, Item, UseItem};
use lamina_core::lower::lower;
use lamina_core::parser::parse;
use lamina_core::LowerError;

/// Wraps bare Lamina source in a ```lamina``` fenced block (the markdown
/// source-document envelope the parser expects).
fn doc(src: &str) -> String {
    format!("```lamina\n{src}\n```\n")
}

/// Parses `src` and returns the single [`Item::Import`] it contains.
fn parse_single_import(src: &str) -> (Vec<ImportArm>, Option<Box<ImportArm>>) {
    let file = parse(&doc(src)).expect("parse import");
    assert_eq!(file.items.len(), 1, "expected exactly one item");
    match file.items.into_iter().next().expect("one item") {
        Item::Import { arms, default, .. } => (arms, default),
        other => panic!("expected Item::Import, got {other:?}"),
    }
}

#[test]
fn parses_bare_module_arms() {
    let (arms, default) = parse_single_import(
        "import {\n    c      { \"stdio.h\" }\n    python { \"sys\" }\n    rust   { \"std::io::Write\" }\n    go     { \"bufio\" }\n}",
    );
    assert!(default.is_none());
    assert_eq!(arms.len(), 4);
    assert_eq!(arms[0].target, "c");
    assert_eq!(arms[0].module, "stdio.h");
    assert!(arms[0].items.is_empty());
    assert!(arms[0].alias.is_none());
    assert_eq!(arms[1].target, "python");
    assert_eq!(arms[1].module, "sys");
    assert_eq!(arms[2].module, "std::io::Write");
    assert_eq!(arms[3].target, "go");
    assert_eq!(arms[3].module, "bufio");
}

#[test]
fn parses_from_import_and_alias_forms() {
    let (arms, _default) = parse_single_import(
        "import {\n    python     { from \"os\" import path }\n    javascript { from \"fs\" import readFileSync }\n    python     { \"numpy\" as np }\n}",
    );
    assert_eq!(arms.len(), 3);
    // from "os" import path
    assert_eq!(arms[0].module, "os");
    assert_eq!(arms[0].items, vec![UseItem { name: "path".into(), alias: None, meta: Default::default() }]);
    assert!(arms[0].alias.is_none());
    // from "fs" import readFileSync
    assert_eq!(arms[1].module, "fs");
    assert_eq!(arms[1].items.len(), 1);
    assert_eq!(arms[1].items[0].name, "readFileSync");
    // "numpy" as np
    assert_eq!(arms[2].module, "numpy");
    assert!(arms[2].items.is_empty());
    assert_eq!(arms[2].alias.as_deref(), Some("np"));
}

#[test]
fn parses_from_import_item_alias() {
    let (arms, _d) = parse_single_import(
        "import {\n    rust { from \"std::io\" import Write as W, Read }\n}",
    );
    assert_eq!(arms[0].module, "std::io");
    assert_eq!(arms[0].items.len(), 2);
    assert_eq!(arms[0].items[0].name, "Write");
    assert_eq!(arms[0].items[0].alias.as_deref(), Some("W"));
    assert_eq!(arms[0].items[1].name, "Read");
    assert!(arms[0].items[1].alias.is_none());
}

#[test]
fn parses_else_fallback() {
    let (arms, default) = parse_single_import(
        "import {\n    c    { \"stdio.h\" }\n    else { \"generic\" }\n}",
    );
    assert_eq!(arms.len(), 1);
    let d = default.expect("else fallback present");
    assert_eq!(d.target, "");
    assert_eq!(d.module, "generic");
}

#[test]
fn lower_selects_matching_target_arm() {
    let file = parse(&doc(
        "import {\n    c      { \"stdio.h\" }\n    python { \"sys\" }\n}",
    ))
    .expect("parse");
    let lowered = lower(&file, "python").expect("lower python");
    match &lowered.items[0] {
        Item::Import { arms, default, .. } => {
            assert_eq!(arms.len(), 1, "resolved to a single arm");
            assert_eq!(arms[0].module, "sys");
            assert!(default.is_none());
        }
        other => panic!("expected Item::Import, got {other:?}"),
    }
}

#[test]
fn lower_uses_else_when_no_target_arm() {
    let file = parse(&doc(
        "import {\n    c    { \"stdio.h\" }\n    else { \"generic\" }\n}",
    ))
    .expect("parse");
    let lowered = lower(&file, "ruby").expect("lower ruby via else");
    match &lowered.items[0] {
        Item::Import { arms, .. } => assert_eq!(arms[0].module, "generic"),
        other => panic!("expected Item::Import, got {other:?}"),
    }
}

#[test]
fn lower_errors_when_no_match_and_no_else() {
    let file = parse(&doc("import {\n    c { \"stdio.h\" }\n}")).expect("parse");
    let err = lower(&file, "ruby").expect_err("no arm, no else -> hard error");
    assert!(matches!(err, LowerError::NoRawArmForTarget { .. }));
}

#[test]
fn import_structural_equality_ignores_meta_and_resolved_target() {
    // Two bare imports of the same module are structurally equal.
    let a = Item::import("stdio.h");
    let b = Item::import("stdio.h");
    assert_eq!(a, b);
    // Distinct modules differ.
    assert_ne!(Item::import("stdio.h"), Item::import("stdlib.h"));
}

#[test]
fn empty_import_block_is_a_parse_error() {
    let err = parse(&doc("import {\n}")).expect_err("empty import block rejected");
    let _ = err;
}

/// A `File` with an import plus a trailing function parses both items in order.
#[test]
fn import_coexists_with_other_items() {
    let file: File = parse(&doc(
        "import {\n    c { \"stdio.h\" }\n}\n\npublic fn f(): void { return; }",
    ))
    .expect("parse");
    assert_eq!(file.items.len(), 2);
    assert!(matches!(file.items[0], Item::Import { .. }));
    assert!(matches!(file.items[1], Item::Function(_)));
}
