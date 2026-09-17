//! End-to-end statement tests against the *shipped* language definitions.
//!
//! There is no concrete source syntax for the full statement set yet (the
//! parser intentionally lags), so these tests build [`Statement`] ASTs directly
//! and transpile them with the real `rust.mdl` / `typescript.mdl` documents from
//! `lamina-defs`, proving both definitions dispatch across every statement kind
//! and render end-to-end.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, File, Function, Item, Statement, SwitchCase, Type, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Absolute path to a shipped language definition in the sibling `lamina-defs`
/// checkout. The tests locate it relative to this crate's manifest dir so they
/// run from any working directory.
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

/// Wraps `body` in `fn f() -> i32 { <body> }` and emits it, returning the whole
/// rendered function.
fn emit_body(body: Vec<Statement>, lang: &LanguageDef) -> String {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(lamina_core::ast::Primitive::I32),
            body,
            meta: lamina_core::ast::Meta::new(),
        })],
    };
    emit(&file, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn r(name: &str) -> Expr {
    Expr::Ref(name.to_string())
}

fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}

// ---- shipped defs load -------------------------------------------------

#[test]
fn shipped_defs_load() {
    let _ = rust();
    let _ = ts();
}

// ---- return (with and without value) -----------------------------------

#[test]
fn bare_return_renders_rust_and_ts() {
    let body = vec![Statement::Return(None)];
    assert!(emit_body(body.clone(), &rust()).contains("return;"));
    assert!(emit_body(body, &ts()).contains("return;"));
}

#[test]
fn return_value_renders() {
    let body = vec![Statement::Return(Some(int("42")))];
    assert!(emit_body(body.clone(), &rust()).contains("return 42;"));
    assert!(emit_body(body, &ts()).contains("return 42;"));
}

// ---- let (with/without type and value) ---------------------------------

#[test]
fn let_with_type_and_value_rust() {
    let body = vec![Statement::Let {
        name: "x".to_string(),
        ty: Some(Type::Primitive(lamina_core::ast::Primitive::I32)),
        value: Some(int("0")),
    }];
    let out = emit_body(body, &rust());
    assert!(out.contains("let x: i32 = 0;"), "got: {out}");
}

#[test]
fn let_without_type_or_value_renders() {
    let body = vec![Statement::Let {
        name: "x".to_string(),
        ty: None,
        value: None,
    }];
    let rust_out = emit_body(body.clone(), &rust());
    assert!(rust_out.contains("let x;"), "got: {rust_out}");
    let ts_out = emit_body(body, &ts());
    assert!(ts_out.contains("let x;"), "got: {ts_out}");
}

#[test]
fn let_value_only_ts() {
    let body = vec![Statement::Let {
        name: "y".to_string(),
        ty: None,
        value: Some(int("5")),
    }];
    let out = emit_body(body, &ts());
    assert!(out.contains("let y = 5;"), "got: {out}");
}

// ---- if / else-if / else ------------------------------------------------

#[test]
fn if_without_else_rust() {
    let body = vec![Statement::If {
        cond: Expr::Binary {
            op: BinaryOp::Lt,
            lhs: Box::new(r("a")),
            rhs: Box::new(int("3")),
        },
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: None,
    }];
    let out = emit_body(body, &rust());
    assert!(out.contains("if a < 3 {"), "got: {out}");
    assert!(out.contains("return 1;"), "got: {out}");
}

#[test]
fn if_else_block_renders_both_arms() {
    let body = vec![Statement::If {
        cond: r("cond"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    }];
    let out = emit_body(body, &ts());
    assert!(out.contains("if (cond) {"), "got: {out}");
    assert!(out.contains("} else {"), "got: {out}");
    assert!(out.contains("return 2;"), "got: {out}");
}

#[test]
fn else_if_chain_renders() {
    // if a { .. } else if b { .. } else { .. }
    let inner = Statement::If {
        cond: r("b"),
        then_block: vec![Statement::Return(Some(int("2")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("3"),
        ))]))),
    };
    let body = vec![Statement::If {
        cond: r("a"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(inner)),
    }];
    let out = emit_body(body, &rust());
    // The else branch renders the nested `if` directly (else-if chain), not a
    // wrapping block.
    assert!(out.contains("} else if b {"), "got: {out}");
}

// ---- while --------------------------------------------------------------

#[test]
fn while_renders() {
    let body = vec![Statement::While {
        cond: r("running"),
        body: vec![Statement::Break],
    }];
    let rust_out = emit_body(body.clone(), &rust());
    assert!(rust_out.contains("while running {"), "got: {rust_out}");
    assert!(rust_out.contains("break;"), "got: {rust_out}");
    let ts_out = emit_body(body, &ts());
    assert!(ts_out.contains("while (running) {"), "got: {ts_out}");
}

// ---- for (counted) ------------------------------------------------------

#[test]
fn counted_for_renders() {
    let body = vec![Statement::For {
        init: Some(Box::new(Statement::Let {
            name: "i".to_string(),
            ty: None,
            value: Some(int("0")),
        })),
        cond: Some(Expr::Binary {
            op: BinaryOp::Lt,
            lhs: Box::new(r("i")),
            rhs: Box::new(int("10")),
        }),
        step: Some(Box::new(Statement::Expr(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(r("i")),
            rhs: Box::new(int("1")),
        }))),
        body: vec![Statement::Continue],
    }];
    let ts_out = emit_body(body.clone(), &ts());
    // C-style header, step carries NO trailing `;` (the header's own `;`
    // separators are the only terminators).
    assert_eq!(
        ts_out,
        "function f(): number {\n    for (let i = 0; i < 10; i + 1) {\n        continue;\n    }\n}",
        "got: {ts_out}"
    );
    // Rust has no C-style for; it desugars to a block-scoped while whose step
    // runs at the end of the body.
    let rust_out = emit_body(body, &rust());
    assert_eq!(
        rust_out,
        "fn f() -> i32 {\n    {\n        let i = 0;\n        while i < 10 {\n            continue;\n            i + 1;\n        }\n    }\n}",
        "got: {rust_out}"
    );
}

// ---- foreach ------------------------------------------------------------

#[test]
fn foreach_renders_distinct_syntax() {
    let body = vec![Statement::ForEach {
        binding: "item".to_string(),
        iterable: r("items"),
        body: vec![Statement::Expr(Expr::Call {
            callee: Box::new(r("use_it")),
            args: vec![r("item")],
        })],
    }];
    let rust_out = emit_body(body.clone(), &rust());
    assert!(rust_out.contains("for item in items {"), "got: {rust_out}");
    assert!(rust_out.contains("use_it(item);"), "got: {rust_out}");
    let ts_out = emit_body(body, &ts());
    assert!(
        ts_out.contains("for (const item of items) {"),
        "got: {ts_out}"
    );
}

// ---- switch -------------------------------------------------------------

#[test]
fn switch_with_cases_and_default_ts() {
    let body = vec![Statement::Switch {
        scrutinee: r("x"),
        cases: vec![
            SwitchCase {
                value: int("1"),
                body: vec![Statement::Return(Some(int("10")))],
                meta: lamina_core::ast::Meta::new(),
            },
            SwitchCase {
                value: int("2"),
                body: vec![Statement::Return(Some(int("20")))],
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        default: Some(vec![Statement::Return(Some(int("0")))]),
    }];
    let out = emit_body(body, &ts());
    assert!(out.contains("switch (x) {"), "got: {out}");
    assert!(out.contains("case 1: {"), "got: {out}");
    assert!(out.contains("case 2: {"), "got: {out}");
    assert!(out.contains("default: {"), "got: {out}");
    assert!(out.contains("return 20;"), "got: {out}");
}

#[test]
fn switch_without_default_rust() {
    let body = vec![Statement::Switch {
        scrutinee: r("x"),
        cases: vec![SwitchCase {
            value: int("1"),
            body: vec![Statement::Break],
            meta: lamina_core::ast::Meta::new(),
        }],
        default: None,
    }];
    let out = emit_body(body, &rust());
    assert!(out.contains("match x {"), "got: {out}");
    assert!(out.contains("1 => {"), "got: {out}");
    // No default arm when `default` is absent.
    assert!(!out.contains("_ =>"), "got: {out}");
}

// ---- block --------------------------------------------------------------

#[test]
fn nested_block_renders() {
    let body = vec![Statement::Block(vec![
        Statement::Let {
            name: "a".to_string(),
            ty: None,
            value: Some(int("1")),
        },
        Statement::Return(Some(r("a"))),
    ])];
    let out = emit_body(body, &rust());
    assert!(out.contains("let a = 1;"), "got: {out}");
    assert!(out.contains("return a;"), "got: {out}");
}

// ---- break / continue / expr-statement ---------------------------------

#[test]
fn break_continue_expr_statements_render() {
    let body = vec![
        Statement::Expr(Expr::Call {
            callee: Box::new(r("side_effect")),
            args: vec![],
        }),
        Statement::Break,
        Statement::Continue,
    ];
    let out = emit_body(body, &rust());
    assert!(out.contains("side_effect();"), "got: {out}");
    assert!(out.contains("break;"), "got: {out}");
    assert!(out.contains("continue;"), "got: {out}");
}
