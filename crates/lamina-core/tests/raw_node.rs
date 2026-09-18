//! Raw / verbatim pass-through node tests (the layer escape hatch).
//!
//! A raw node (`Expr::Raw` / `Statement::Raw` / `Item::Raw`) holds a string of
//! literal target code the engine emits UNCHANGED at the slot position. It is
//! deliberately NOT target-keyed: a raw node exists in the AST only when a
//! layer lowered it for the current target, so the engine performs no target
//! check, capability gating, or error path — it just passes the string
//! through. These tests build such ASTs directly and transpile them with the
//! real `rust.mdl` / `typescript.mdl` documents from `lamina-defs`, asserting
//! the verbatim string appears unchanged in the output for BOTH targets.
//!
//! ## Indentation of multi-line raw code (documented behavior)
//!
//! A raw node's string is inserted at its slot position and re-indented by the
//! renderer's ordinary column-derived continuation-line rule (the SAME rule
//! that applies to every multi-line rendered fragment): the leading whitespace
//! of the current template line is prepended to every line *after the first* of
//! the raw string. So a multi-line raw statement placed in a function body
//! (whose `{body}` slot sits at column 4 in the shipped defs) has its second
//! and later lines indented by 4 spaces; the first line's indent is the literal
//! template text preceding the slot. Blank lines get no trailing whitespace.
//! The engine never re-flows, trims, or otherwise transforms the raw text
//! beyond this uniform continuation-line indentation.

use std::path::PathBuf;

use lamina_core::ast::{Expr, File, Function, Item, Meta, Primitive, Statement, Type, Visibility};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout.
fn shipped_def(file: &str) -> LanguageDef {
    // CARGO_MANIFEST_DIR = <repo>/crates/lamina-core
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push(file);
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped def {file} should load: {e}"))
}

fn rust() -> LanguageDef {
    shipped_def("rust.mdl")
}

fn ts() -> LanguageDef {
    shipped_def("typescript.mdl")
}

/// Wraps `body` in `fn f() -> i32 { <body> }` and emits the whole function.
fn emit_body(body: Vec<Statement>, lang: &LanguageDef) -> String {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::I32),
            body,
            meta: Meta::new(),
        })],
    };
    emit(&file, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_items(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

// ---- shipped defs still load with the new `## Raw` section --------------

#[test]
fn shipped_defs_load_with_raw_section() {
    let _ = rust();
    let _ = ts();
}

// ---- Expr::Raw ----------------------------------------------------------

#[test]
fn expr_raw_emits_verbatim_rust() {
    // A raw expression used as a returned value: the engine emits its string
    // unchanged (here Rust syntax the kernel could not otherwise express).
    let body = vec![Statement::Return(Some(Expr::Raw {
        code: "unsafe { *ptr }".to_string(),
        meta: Meta::new(),
    }))];
    let out = emit_body(body, &rust());
    assert!(
        out.contains("return unsafe { *ptr };"),
        "raw expr must appear verbatim; got: {out}"
    );
}

#[test]
fn expr_raw_emits_verbatim_ts() {
    let body = vec![Statement::Return(Some(Expr::Raw {
        code: "await foo?.bar!".to_string(),
        meta: Meta::new(),
    }))];
    let out = emit_body(body, &ts());
    assert!(
        out.contains("return await foo?.bar!;"),
        "raw expr must appear verbatim; got: {out}"
    );
}

// ---- Statement::Raw -----------------------------------------------------

#[test]
fn statement_raw_emits_verbatim_rust() {
    // A raw statement supplies its own terminator/newlines — the engine adds
    // nothing. Here it is a single-line raw statement.
    let body = vec![Statement::Raw {
        code: "todo!(\"hand-written\");".to_string(),
        meta: Meta::new(),
    }];
    let out = emit_body(body, &rust());
    assert!(
        out.contains("todo!(\"hand-written\");"),
        "raw stmt must appear verbatim; got: {out}"
    );
}

#[test]
fn statement_raw_emits_verbatim_ts() {
    let body = vec![Statement::Raw {
        code: "throw new Error(\"boom\");".to_string(),
        meta: Meta::new(),
    }];
    let out = emit_body(body, &ts());
    assert!(
        out.contains("throw new Error(\"boom\");"),
        "raw stmt must appear verbatim; got: {out}"
    );
}

// ---- multi-line raw statement indentation (documented behavior) ---------

#[test]
fn multiline_raw_statement_indents_continuation_lines_rust() {
    // The `{body}` slot in rust.mdl sits at column 4. A multi-line raw
    // statement therefore has its FIRST line at the body indent (from the
    // template text preceding the slot) and every SUBSEQUENT line prefixed with
    // 4 spaces by the renderer's column-derived continuation-line rule. The raw
    // text itself is otherwise untouched (no re-flow, no trim).
    let body = vec![Statement::Raw {
        code: "match x {\n    1 => a(),\n    _ => b(),\n}".to_string(),
        meta: Meta::new(),
    }];
    let out = emit_body(body, &rust());
    let expected = "fn f() -> i32 {\n    match x {\n        1 => a(),\n        _ => b(),\n    }\n}";
    assert_eq!(out, expected, "multi-line raw indentation; got: {out}");
}

#[test]
fn multiline_raw_statement_indents_continuation_lines_ts() {
    let body = vec![Statement::Raw {
        code: "switch (x) {\n    case 1:\n        a();\n}".to_string(),
        meta: Meta::new(),
    }];
    let out = emit_body(body, &ts());
    let expected =
        "function f(): number {\n    switch (x) {\n        case 1:\n            a();\n    }\n}";
    assert_eq!(out, expected, "multi-line raw indentation; got: {out}");
}

// ---- Item::Raw ----------------------------------------------------------

#[test]
fn item_raw_emits_verbatim_rust() {
    // A raw top-level item renders through rust.mdl's `## Raw` section, which
    // is a trivial `{value}` pass-through.
    let item = Item::Raw {
        code: "#[macro_export]\nmacro_rules! id { ($x:expr) => { $x }; }".to_string(),
        meta: Meta::new(),
    };
    let out = emit_items(vec![item], &rust());
    assert_eq!(
        out, "#[macro_export]\nmacro_rules! id { ($x:expr) => { $x }; }",
        "raw item must appear verbatim; got: {out}"
    );
}

#[test]
fn item_raw_emits_verbatim_ts() {
    let item = Item::Raw {
        code: "declare const __DEV__: boolean;".to_string(),
        meta: Meta::new(),
    };
    let out = emit_items(vec![item], &ts());
    assert_eq!(
        out, "declare const __DEV__: boolean;",
        "raw item must appear verbatim; got: {out}"
    );
}

#[test]
fn raw_item_interleaves_with_other_items_verbatim() {
    // A raw item mixed with an ordinary function: the raw string is emitted
    // unchanged in its position, proving no perturbation of neighbours.
    let items = vec![
        Item::Raw {
            code: "// hand-written prelude".to_string(),
            meta: Meta::new(),
        },
        Item::Function(Function {
            name: "g".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::I32),
            body: vec![Statement::Return(Some(Expr::IntLiteral("1".to_string())))],
            meta: Meta::new(),
        }),
    ];
    let out = emit_items(items, &rust());
    assert!(
        out.starts_with("// hand-written prelude"),
        "raw item verbatim and first; got: {out}"
    );
    assert!(out.contains("fn g() -> i32 {"), "neighbour intact; got: {out}");
}

// ---- metadata is carried and ignored by structural equality -------------

#[test]
fn raw_nodes_carry_metadata_ignored_by_equality() {
    // Each raw variant carries the standard `meta` channel; structural equality
    // compares the verbatim string and IGNORES metadata (consistent with the
    // rest of the AST).
    let mut meta = Meta::new();
    meta.set("origin", "hand_lowered");

    let expr_a = Expr::Raw {
        code: "foo()".to_string(),
        meta: meta.clone(),
    };
    let expr_b = Expr::Raw {
        code: "foo()".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(expr_a, expr_b, "expr equality ignores metadata");

    let stmt_a = Statement::Raw {
        code: "foo();".to_string(),
        meta: meta.clone(),
    };
    let stmt_b = Statement::Raw {
        code: "foo();".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(stmt_a, stmt_b, "stmt equality ignores metadata");

    let item_a = Item::Raw {
        code: "const X = 1;".to_string(),
        meta,
    };
    let item_b = Item::Raw {
        code: "const X = 1;".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(item_a, item_b, "item equality ignores metadata");

    // Different code strings are NOT equal.
    let other = Expr::Raw {
        code: "bar()".to_string(),
        meta: Meta::new(),
    };
    assert_ne!(expr_a, other, "different raw code is not equal");
}

// ---- metadata is readable via the `has_meta` / `meta.<key>` facts -------

#[test]
fn raw_node_metadata_readable_but_does_not_perturb_output() {
    // A raw node with metadata renders byte-identically to one without: the
    // engine passes the string through and never emits metadata itself.
    let mut meta = Meta::new();
    meta.set("origin", "layer");
    let with_meta = emit_items(
        vec![Item::Raw {
            code: "type Id = number;".to_string(),
            meta,
        }],
        &ts(),
    );
    let without_meta = emit_items(
        vec![Item::Raw {
            code: "type Id = number;".to_string(),
            meta: Meta::new(),
        }],
        &ts(),
    );
    assert_eq!(with_meta, without_meta, "metadata does not perturb output");
    assert_eq!(with_meta, "type Id = number;");
}
