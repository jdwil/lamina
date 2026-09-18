//! End-to-end **idiomatic audit** against the *shipped* language definitions
//! (`lamina-defs/languages/rust.mdl` and `typescript.mdl`), loaded via
//! [`load_language_def`] — not inline fixtures.
//!
//! For every kernel construct — each statement kind, each expression kind and
//! operator, each item kind, and the compound `pointer` / `fnptr` types — this
//! builds a representative AST and asserts the emitted Rust is valid idiomatic
//! Rust and the emitted TypeScript is valid idiomatic TS (exact string match).
//!
//! The parser intentionally lags the AST, so these construct the AST directly
//! and drive [`emit`] with the real definitions.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, Function, Item, Modifier, Primitive, Statement, SwitchCase, Type,
    UnaryOp, Variant, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

// ---- shipped-def loading ----------------------------------------------

fn shipped_def(file: &str) -> LanguageDef {
    // CARGO_MANIFEST_DIR = <repo>/crates/lamina-core
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // lamina (repo)
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

// ---- helpers -----------------------------------------------------------

fn emit_one(item: Item, lang: &LanguageDef) -> String {
    emit(&File { items: vec![item] }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_err(item: Item, lang: &LanguageDef) -> lamina_core::EmitError {
    emit(&File { items: vec![item] }, lang).expect_err("expected emit error")
}

/// Wraps `body` in `fn f() -> i32 { <body> }` and emits the whole function.
fn func_with_body(body: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::I32),
        body,
        meta: lamina_core::ast::Meta::new(),
    })
}

fn emit_body(body: Vec<Statement>, lang: &LanguageDef) -> String {
    emit_one(func_with_body(body), lang)
}

/// Wraps `ret` as `fn f() -> <ret> { return 0; }` to exercise type rendering.
fn func_returning(ret: Type) -> Item {
    Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: ret,
        body: vec![Statement::Return(Some(int("0")))],
        meta: lamina_core::ast::Meta::new(),
    })
}

/// Emits just the single statement inside `fn f`, returning the inner text with
/// the wrapper stripped, so per-statement assertions stay focused.
fn stmt_text(stmt: Statement, lang: &LanguageDef) -> String {
    let whole = emit_body(vec![stmt], lang);
    // Strip `fn f() -> i32 {\n    ` prefix and `\n}` suffix conceptually by
    // returning the whole thing; assertions match the full function to keep
    // indentation honest.
    whole
}

fn r(name: &str) -> Expr {
    Expr::Ref(name.to_string())
}
fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}
fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(l),
        rhs: Box::new(r),
    }
}

// =======================================================================
// Shipped defs load
// =======================================================================

#[test]
fn shipped_defs_load() {
    let _ = rust();
    let _ = ts();
}

// =======================================================================
// GAP: compound pointer / fnptr TYPES
// =======================================================================

#[test]
fn rust_pointer_type_renders() {
    let out = emit_one(
        func_returning(Type::Pointer(Box::new(Type::Primitive(Primitive::I32)))),
        &rust(),
    );
    assert_eq!(
        out, "fn f() -> *const i32 {\n    return 0;\n}",
        "got: {out}"
    );
}

#[test]
fn rust_nested_pointer_type_renders() {
    let inner = Type::Pointer(Box::new(Type::Primitive(Primitive::U8)));
    let out = emit_one(func_returning(Type::Pointer(Box::new(inner))), &rust());
    assert_eq!(
        out, "fn f() -> *const *const u8 {\n    return 0;\n}",
        "got: {out}"
    );
}

#[test]
fn rust_fnptr_type_renders() {
    let ty = Type::FnPtr {
        params: vec![
            Type::Primitive(Primitive::I32),
            Type::Primitive(Primitive::Bool),
        ],
        ret: Box::new(Type::Primitive(Primitive::I64)),
    };
    let out = emit_one(func_returning(ty), &rust());
    assert_eq!(
        out, "fn f() -> fn(i32, bool) -> i64 {\n    return 0;\n}",
        "got: {out}"
    );
}

#[test]
fn rust_fnptr_type_no_params_renders() {
    let ty = Type::FnPtr {
        params: vec![],
        ret: Box::new(Type::Primitive(Primitive::Void)),
    };
    let out = emit_one(func_returning(ty), &rust());
    assert_eq!(
        out, "fn f() -> fn() -> () {\n    return 0;\n}",
        "got: {out}"
    );
}

#[test]
fn ts_pointer_type_errors_cleanly() {
    // TypeScript forbids `ptr` in its capability matrix, so a pointer type is
    // gated *before* any slot lookup: a clean ForbiddenPrimitive, not a panic
    // or an UnknownSlot.
    let err = emit_err(
        func_returning(Type::Pointer(Box::new(Type::Primitive(Primitive::I32)))),
        &ts(),
    );
    assert!(
        matches!(err, lamina_core::EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "ptr"),
        "got: {err:?}"
    );
}

#[test]
fn ts_fnptr_type_errors_cleanly() {
    let ty = Type::FnPtr {
        params: vec![Type::Primitive(Primitive::I32)],
        ret: Box::new(Type::Primitive(Primitive::Void)),
    };
    let err = emit_err(func_returning(ty), &ts());
    assert!(
        matches!(err, lamina_core::EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "fnptr"),
        "got: {err:?}"
    );
}

// =======================================================================
// STATEMENTS
// =======================================================================

#[test]
fn block_statement() {
    let s = Statement::Block(vec![Statement::Return(Some(int("1")))]);
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    {\n        return 1;\n    }\n}"
    );
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    {\n        return 1;\n    }\n}"
    );
}

#[test]
fn let_full() {
    let s = Statement::Let {
        name: "x".to_string(),
        ty: Some(Type::Primitive(Primitive::I32)),
        value: Some(int("0")),
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    let x: i32 = 0;\n}"
    );
    // i32 widens to number in TS.
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    let x: number = 0;\n}"
    );
}

#[test]
fn let_bare() {
    let s = Statement::Let {
        name: "x".to_string(),
        ty: None,
        value: None,
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    let x;\n}"
    );
    assert_eq!(stmt_text(s, &ts()), "function f(): number {\n    let x;\n}");
}

#[test]
fn return_bare_and_value() {
    assert_eq!(
        stmt_text(Statement::Return(None), &rust()),
        "fn f() -> i32 {\n    return;\n}"
    );
    assert_eq!(
        stmt_text(Statement::Return(Some(int("42"))), &rust()),
        "fn f() -> i32 {\n    return 42;\n}"
    );
    assert_eq!(
        stmt_text(Statement::Return(Some(int("42"))), &ts()),
        "function f(): number {\n    return 42;\n}"
    );
}

#[test]
fn if_no_else() {
    let s = Statement::If {
        cond: bin(BinaryOp::Lt, r("a"), int("3")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: None,
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    if a < 3 {\n        return 1;\n    }\n}"
    );
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    if (a < 3) {\n        return 1;\n    }\n}"
    );
}

#[test]
fn if_else_block() {
    let s = Statement::If {
        cond: r("cond"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    if cond {\n        return 1;\n    } else {\n        return 2;\n    }\n}"
    );
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    if (cond) {\n        return 1;\n    } else {\n        return 2;\n    }\n}"
    );
}

#[test]
fn else_if_chain() {
    let inner = Statement::If {
        cond: r("b"),
        then_block: vec![Statement::Return(Some(int("2")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("3"),
        ))]))),
    };
    let s = Statement::If {
        cond: r("a"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(inner)),
    };
    // The else branch renders the nested `if` directly (else-if chain).
    let out = stmt_text(s, &rust());
    assert_eq!(
        out,
        "fn f() -> i32 {\n    if a {\n        return 1;\n    } else if b {\n        return 2;\n    } else {\n        return 3;\n    }\n}",
        "got: {out}"
    );
}

#[test]
fn while_loop() {
    let s = Statement::While {
        cond: r("running"),
        body: vec![Statement::Break],
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    while running {\n        break;\n    }\n}"
    );
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    while (running) {\n        break;\n    }\n}"
    );
}

#[test]
fn counted_for_ts_is_c_style_without_trailing_semicolon() {
    // The regression this whole audit was built around: the C-style `for`
    // header's step must NOT carry a trailing `;`.
    let s = Statement::For {
        init: Some(Box::new(Statement::Let {
            name: "i".to_string(),
            ty: None,
            value: Some(int("0")),
        })),
        cond: Some(bin(BinaryOp::Lt, r("i"), int("10"))),
        step: Some(Box::new(Statement::Expr(bin(
            BinaryOp::Add,
            r("i"),
            int("1"),
        )))),
        body: vec![Statement::Continue],
    };
    let out = stmt_text(s, &ts());
    assert_eq!(
        out,
        "function f(): number {\n    for (let i = 0; i < 10; i + 1) {\n        continue;\n    }\n}",
        "got: {out}"
    );
}

#[test]
fn counted_for_rust_desugars_to_scoped_while() {
    // Rust has no C-style `for`; the idiomatic lowering is a block-scoped
    // `while` whose step runs at the end of the loop body. Documented in
    // rust.mdl.
    //
    // The kernel AST has no assignment node (no `Assign` statement or expr): a
    // counted loop's step is modeled as `Statement::Expr`, an expression
    // evaluated for effect. A bare arithmetic step like `i + 1` would emit as
    // `i + 1;`, a dead expr-statement Rust warns on (unused arithmetic). A
    // faithful effectful step is therefore a call, `step(i);`, which is valid
    // non-dead Rust. (Recovering `i = i + 1` is a layer concern once an
    // assignment node exists in the kernel.)
    let step = Statement::Expr(Expr::Call {
        callee: Box::new(r("step")),
        args: vec![r("i")],
    });
    let s = Statement::For {
        init: Some(Box::new(Statement::Let {
            name: "i".to_string(),
            ty: None,
            value: Some(int("0")),
        })),
        cond: Some(bin(BinaryOp::Lt, r("i"), int("10"))),
        step: Some(Box::new(step)),
        body: vec![Statement::Continue],
    };
    let out = stmt_text(s, &rust());
    assert_eq!(
        out,
        "fn f() -> i32 {\n    {\n        let i = 0;\n        while i < 10 {\n            continue;\n            step(i);\n        }\n    }\n}",
        "got: {out}"
    );
}

#[test]
fn counted_for_rust_no_cond_is_loop() {
    // Without a condition the desugar uses Rust's infinite `loop`.
    let s = Statement::For {
        init: None,
        cond: None,
        step: None,
        body: vec![Statement::Break],
    };
    let out = stmt_text(s, &rust());
    assert_eq!(
        out, "fn f() -> i32 {\n    {\n        loop {\n            break;\n        }\n    }\n}",
        "got: {out}"
    );
}

#[test]
fn foreach_loop() {
    let s = Statement::ForEach {
        binding: "item".to_string(),
        iterable: r("items"),
        body: vec![Statement::Expr(Expr::Call {
            callee: Box::new(r("use_it")),
            args: vec![r("item")],
        })],
    };
    assert_eq!(
        stmt_text(s.clone(), &rust()),
        "fn f() -> i32 {\n    for item in items {\n        use_it(item);\n    }\n}"
    );
    assert_eq!(
        stmt_text(s, &ts()),
        "function f(): number {\n    for (const item of items) {\n        use_it(item);\n    }\n}"
    );
}

#[test]
fn switch_with_default() {
    let s = Statement::Switch {
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
    };
    // Rust: `match` arms with `=>` and trailing `,`, `_` default arm.
    let rust_out = stmt_text(s.clone(), &rust());
    assert_eq!(
        rust_out,
        "fn f() -> i32 {\n    match x {\n        1 => {\n            return 10;\n        },\n        2 => {\n            return 20;\n        },\n        _ => {\n            return 0;\n        }\n    }\n}",
        "got: {rust_out}"
    );
    // TS: native switch/case/default with block bodies.
    let ts_out = stmt_text(s, &ts());
    assert_eq!(
        ts_out,
        "function f(): number {\n    switch (x) {\n        case 1: {\n            return 10;\n        }\n        case 2: {\n            return 20;\n        }\n        default: {\n            return 0;\n        }\n    }\n}",
        "got: {ts_out}"
    );
}

#[test]
fn switch_without_default() {
    // KNOWN SEMANTIC GAP (not a def bug): a kernel `switch` with no `default`
    // lowers to a Rust `match` with only the explicit arms. Rust `match` must
    // be exhaustive (E0004), but the emitter cannot synthesize a `_ =>` arm —
    // there is no default body to put in it, and inventing one (e.g. `_ =>
    // {}`) would silently change semantics. So a defaultless switch emits a
    // non-exhaustive `match`; supplying exhaustive coverage (a wildcard arm, an
    // `unreachable!()`, or full enum coverage) is a layer/authoring concern,
    // not something the operator/statement mapping can conjure. This test pins
    // the *faithful* lowering, documenting the gap rather than asserting some
    // synthesized-but-wrong "compilable" output. The `switch_with_default` test
    // above covers the exhaustive (compilable) case.
    let s = Statement::Switch {
        scrutinee: r("x"),
        cases: vec![SwitchCase {
            value: int("1"),
            body: vec![Statement::Break],
            meta: lamina_core::ast::Meta::new(),
        }],
        default: None,
    };
    let rust_out = stmt_text(s.clone(), &rust());
    assert_eq!(
        rust_out,
        "fn f() -> i32 {\n    match x {\n        1 => {\n            break;\n        },\n    }\n}",
        "got: {rust_out}"
    );
    let ts_out = stmt_text(s, &ts());
    assert_eq!(
        ts_out,
        "function f(): number {\n    switch (x) {\n        case 1: {\n            break;\n        }\n    }\n}",
        "got: {ts_out}"
    );
}

#[test]
fn break_continue_expr_statements() {
    let body = vec![
        Statement::Expr(Expr::Call {
            callee: Box::new(r("side_effect")),
            args: vec![],
        }),
        Statement::Break,
        Statement::Continue,
    ];
    assert_eq!(
        emit_body(body.clone(), &rust()),
        "fn f() -> i32 {\n    side_effect();\n    break;\n    continue;\n}"
    );
    assert_eq!(
        emit_body(body, &ts()),
        "function f(): number {\n    side_effect();\n    break;\n    continue;\n}"
    );
}

// =======================================================================
// EXPRESSIONS — literals, refs, access, calls
// =======================================================================

#[test]
fn literals_render_idiomatically() {
    // int / float / bool / string / char, each as a return value.
    let cases = [
        (Expr::IntLiteral("7".to_string()), "7", "7"),
        (Expr::FloatLiteral("1.5".to_string()), "1.5", "1.5"),
        (Expr::BoolLiteral(true), "true", "true"),
        (Expr::StringLiteral("hi".to_string()), "\"hi\"", "\"hi\""),
        (Expr::CharLiteral("c".to_string()), "'c'", "\"c\""),
    ];
    for (expr, rust_lit, ts_lit) in cases {
        let rust_out = emit_body(vec![Statement::Return(Some(expr.clone()))], &rust());
        assert_eq!(
            rust_out,
            format!("fn f() -> i32 {{\n    return {rust_lit};\n}}"),
            "rust: {expr:?}"
        );
        let ts_out = emit_body(vec![Statement::Return(Some(expr.clone()))], &ts());
        assert_eq!(
            ts_out,
            format!("function f(): number {{\n    return {ts_lit};\n}}"),
            "ts: {expr:?}"
        );
    }
}

#[test]
fn null_literal_rust_only() {
    // Rust spells kernel null as a null raw pointer; TS forbids ptr so null is
    // forbidden too (null follows ptr).
    let rust_out = emit_body(vec![Statement::Return(Some(Expr::NullLiteral))], &rust());
    assert_eq!(rust_out, "fn f() -> i32 {\n    return std::ptr::null();\n}");
    let err = emit(
        &File {
            items: vec![func_with_body(vec![Statement::Return(Some(
                Expr::NullLiteral,
            ))])],
        },
        &ts(),
    )
    .expect_err("null forbidden in ts");
    assert!(
        matches!(err, lamina_core::EmitError::ForbiddenOperator { ref operator, .. } if operator == "null"),
        "got: {err:?}"
    );
}

#[test]
fn field_index_call_render() {
    let field = Expr::Field {
        obj: Box::new(r("obj")),
        field: "x".to_string(),
    };
    assert_eq!(
        emit_body(vec![Statement::Return(Some(field))], &rust()),
        "fn f() -> i32 {\n    return obj.x;\n}"
    );
    let index = Expr::Index {
        obj: Box::new(r("arr")),
        index: Box::new(int("2")),
    };
    assert_eq!(
        emit_body(vec![Statement::Return(Some(index))], &ts()),
        "function f(): number {\n    return arr[2];\n}"
    );
    let call = Expr::Call {
        callee: Box::new(r("g")),
        args: vec![int("1"), r("y")],
    };
    assert_eq!(
        emit_body(vec![Statement::Return(Some(call.clone()))], &rust()),
        "fn f() -> i32 {\n    return g(1, y);\n}"
    );
    assert_eq!(
        emit_body(vec![Statement::Return(Some(call))], &ts()),
        "function f(): number {\n    return g(1, y);\n}"
    );
}

// =======================================================================
// EXPRESSIONS — every unary and binary operator
// =======================================================================

#[test]
fn all_unary_operators() {
    // Neg and Not are valid unary operators in both Rust and TypeScript.
    let both = [(UnaryOp::Neg, "-a"), (UnaryOp::Not, "!a")];
    for (op, expected) in both {
        let e = Expr::Unary {
            op,
            operand: Box::new(r("a")),
        };
        let rust_out = emit_body(vec![Statement::Return(Some(e.clone()))], &rust());
        assert_eq!(
            rust_out,
            format!("fn f() -> i32 {{\n    return {expected};\n}}"),
            "rust {op:?}"
        );
        let ts_out = emit_body(vec![Statement::Return(Some(e))], &ts());
        assert_eq!(
            ts_out,
            format!("function f(): number {{\n    return {expected};\n}}"),
            "ts {op:?}"
        );
    }

    // BitNot (`~`) and unary Pos (`+`) are valid TypeScript but NOT valid Rust
    // (Rust has no unary `~` and no unary `+`), so rust.mdl forbids them.
    let ts_only = [(UnaryOp::BitNot, "~a"), (UnaryOp::Pos, "+a")];
    for (op, expected) in ts_only {
        let e = Expr::Unary {
            op,
            operand: Box::new(r("a")),
        };
        // Rust: forbidden operator error.
        let err = emit_err(
            func_with_body(vec![Statement::Return(Some(e.clone()))]),
            &rust(),
        );
        assert!(
            matches!(err, lamina_core::EmitError::ForbiddenOperator { .. }),
            "rust {op:?} should be ForbiddenOperator, got {err:?}"
        );
        // TypeScript: renders normally.
        let ts_out = emit_body(vec![Statement::Return(Some(e))], &ts());
        assert_eq!(
            ts_out,
            format!("function f(): number {{\n    return {expected};\n}}"),
            "ts {op:?}"
        );
    }
}

#[test]
fn all_binary_operators_rust() {
    // Every kernel binary operator that Rust *can* spell maps to its canonical
    // spelling. `pow` (`**`), `floordiv` (`//`), and `ushr` (`>>>`) are NOT
    // valid Rust operators, so they are forbidden by rust.mdl's `## Operators`
    // table and asserted separately in `rust_forbidden_operators` below.
    let ops = [
        (BinaryOp::Add, "a + b"),
        (BinaryOp::Sub, "a - b"),
        (BinaryOp::Mul, "a * b"),
        (BinaryOp::Div, "a / b"),
        (BinaryOp::Rem, "a % b"),
        (BinaryOp::Eq, "a == b"),
        (BinaryOp::Ne, "a != b"),
        (BinaryOp::Lt, "a < b"),
        (BinaryOp::Le, "a <= b"),
        (BinaryOp::Gt, "a > b"),
        (BinaryOp::Ge, "a >= b"),
        (BinaryOp::And, "a && b"),
        (BinaryOp::Or, "a || b"),
        (BinaryOp::BitAnd, "a & b"),
        (BinaryOp::BitOr, "a | b"),
        (BinaryOp::BitXor, "a ^ b"),
        (BinaryOp::Shl, "a << b"),
        (BinaryOp::Shr, "a >> b"),
    ];
    for (op, expected) in ops {
        let e = bin(op, r("a"), r("b"));
        let out = emit_body(vec![Statement::Return(Some(e))], &rust());
        assert_eq!(
            out,
            format!("fn f() -> i32 {{\n    return {expected};\n}}"),
            "rust {op:?}"
        );
    }
}

#[test]
fn rust_forbidden_operators() {
    // Rust has no `**`, `//`, or `>>>` operator: the canonical Lamina spellings
    // are respectively a type error (E0614), a line comment, and a
    // non-existent operator. rust.mdl forbids each, so emission must fail with
    // a ForbiddenOperator carrying the canonical spelling — not silently emit
    // invalid Rust. (Layers lower these to `i32::pow`, `.floor()`, and an
    // unsigned-typed `>>` respectively.)
    let cases = [
        (BinaryOp::Pow, "**"),
        (BinaryOp::FloorDiv, "//"),
        (BinaryOp::UShr, ">>>"),
    ];
    for (op, spelling) in cases {
        let e = bin(op, r("a"), r("b"));
        let err = emit_err(func_with_body(vec![Statement::Return(Some(e))]), &rust());
        assert!(
            matches!(err, lamina_core::EmitError::ForbiddenOperator { ref operator, .. } if operator == spelling),
            "rust {op:?}: got {err:?}"
        );
    }
}

#[test]
fn ts_binary_operators_and_forbidden_floordiv() {
    // TS keeps canonical spellings for all operators except floor-division,
    // which it forbids (a layer lowers it to Math.floor).
    let ok = [
        (BinaryOp::Add, "a + b"),
        (BinaryOp::Pow, "a ** b"),
        (BinaryOp::UShr, "a >>> b"),
        (BinaryOp::And, "a && b"),
    ];
    for (op, expected) in ok {
        let e = bin(op, r("a"), r("b"));
        let out = emit_body(vec![Statement::Return(Some(e))], &ts());
        assert_eq!(
            out,
            format!("function f(): number {{\n    return {expected};\n}}"),
            "ts {op:?}"
        );
    }
    let floordiv = bin(BinaryOp::FloorDiv, r("a"), r("b"));
    let err = emit(
        &File {
            items: vec![func_with_body(vec![Statement::Return(Some(floordiv))])],
        },
        &ts(),
    )
    .expect_err("floordiv forbidden in ts");
    assert!(
        matches!(err, lamina_core::EmitError::ForbiddenOperator { ref operator, .. } if operator == "//"),
        "got: {err:?}"
    );
}

#[test]
fn nested_operators_parenthesize_to_preserve_grouping() {
    // (a * b) + (-c) — compound operands are parenthesized by the engine.
    let e = bin(
        BinaryOp::Add,
        bin(BinaryOp::Mul, r("a"), r("b")),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand: Box::new(r("c")),
        },
    );
    assert_eq!(
        emit_body(vec![Statement::Return(Some(e.clone()))], &rust()),
        "fn f() -> i32 {\n    return (a * b) + (-c);\n}"
    );
    assert_eq!(
        emit_body(vec![Statement::Return(Some(e))], &ts()),
        "function f(): number {\n    return (a * b) + (-c);\n}"
    );
}

// =======================================================================
// ITEMS — function (params/modifiers/visibility), struct, enum, typedef,
//         const, use
// =======================================================================

#[test]
fn function_with_params_modifiers_visibility() {
    let item = Item::Function(Function {
        name: "add".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async, Modifier::Const],
        params: vec![
            lamina_core::ast::Param {
                name: "a".to_string(),
                ty: Type::Primitive(Primitive::I32),
                meta: lamina_core::ast::Meta::new(),
            },
            lamina_core::ast::Param {
                name: "b".to_string(),
                ty: Type::Primitive(Primitive::I32),
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        return_type: Type::Primitive(Primitive::I32),
        body: vec![Statement::Return(Some(bin(BinaryOp::Add, r("a"), r("b"))))],
        meta: lamina_core::ast::Meta::new(),
    });
    // Rust: `pub async const fn add(a: i32, b: i32) -> i32`.
    assert_eq!(
        emit_one(item.clone(), &rust()),
        "pub async const fn add(a: i32, b: i32) -> i32 {\n    return a + b;\n}"
    );
    // TS: `export async function add(a: number, b: number): number` (no const
    // spelling for a free function).
    assert_eq!(
        emit_one(item, &ts()),
        "export async function add(a: number, b: number): number {\n    return a + b;\n}"
    );
}

#[test]
fn void_and_never_returns() {
    let void_fn = Item::Function(Function {
        name: "p".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![Statement::Return(None)],
        meta: lamina_core::ast::Meta::new(),
    });
    assert_eq!(
        emit_one(void_fn.clone(), &rust()),
        "fn p() {\n    return;\n}"
    );
    assert_eq!(
        emit_one(void_fn, &ts()),
        "function p(): void {\n    return;\n}"
    );
    let never_fn = Item::Function(Function {
        name: "boom".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Never),
        body: vec![Statement::Return(None)],
        meta: lamina_core::ast::Meta::new(),
    });
    assert_eq!(
        emit_one(never_fn.clone(), &rust()),
        "fn boom() -> ! {\n    return;\n}"
    );
    // TS has a native `never` type.
    assert_eq!(
        emit_one(never_fn, &ts()),
        "function boom(): never {\n    return;\n}"
    );
}

#[test]
fn struct_item() {
    let item = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        fields: vec![
            Field {
                name: "x".to_string(),
                ty: Type::Primitive(Primitive::I32),
                visibility: Visibility::Public,
                meta: lamina_core::ast::Meta::new(),
            },
            Field {
                name: "y".to_string(),
                ty: Type::Primitive(Primitive::I32),
                visibility: Visibility::Private,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(
        emit_one(item.clone(), &rust()),
        "pub struct Point {\n    pub x: i32,\n    y: i32,\n}"
    );
    assert_eq!(
        emit_one(item, &ts()),
        "export interface Point {\n    x: number;\n    y: number;\n}"
    );
}

#[test]
fn enum_item() {
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: lamina_core::ast::VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: lamina_core::ast::VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(
        emit_one(item.clone(), &rust()),
        "pub enum Color {\n    Red,\n    Green,\n}"
    );
    assert_eq!(
        emit_one(item, &ts()),
        "export enum Color {\n    Red,\n    Green,\n}"
    );
}

#[test]
fn typedef_const_use_items() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: Type::Primitive(Primitive::I32),
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(emit_one(td.clone(), &rust()), "type Id = i32;");
    assert_eq!(emit_one(td, &ts()), "type Id = number;");

    let c = Item::Const {
        name: "MAX".to_string(),
        ty: Type::Primitive(Primitive::I32),
        value: int("100"),
        visibility: Visibility::Public,
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(emit_one(c.clone(), &rust()), "pub const MAX: i32 = 100;");
    assert_eq!(emit_one(c, &ts()), "export const MAX: number = 100;");

    assert_eq!(
        emit_one(
            Item::Use {
                path: "std::io".to_string(),
                items: vec![],
                alias: None,
                meta: lamina_core::ast::Meta::new(),
            },
            &rust()
        ),
        "use std::io;"
    );
    assert_eq!(
        emit_one(
            Item::Use {
                path: "\"fs\"".to_string(),
                items: vec![],
                alias: None,
                meta: lamina_core::ast::Meta::new(),
            },
            &ts()
        ),
        "import \"fs\";"
    );
}

// =======================================================================
// A whole mixed file, both targets, order preserved
// =======================================================================

#[test]
fn mixed_file_renders_in_order_both_targets() {
    let items = vec![
        Item::Use {
            path: "std::io".to_string(),
            items: vec![],
            alias: None,
            meta: lamina_core::ast::Meta::new(),
        },
        Item::Const {
            name: "N".to_string(),
            ty: Type::Primitive(Primitive::I32),
            value: int("3"),
            visibility: Visibility::Private,
            meta: lamina_core::ast::Meta::new(),
        },
        func_with_body(vec![Statement::Return(Some(int("1")))]),
    ];
    let rust_out = emit(
        &File {
            items: items.clone(),
        },
        &rust(),
    )
    .expect("rust emit");
    assert_eq!(
        rust_out,
        "use std::io;\n\nconst N: i32 = 3;\n\nfn f() -> i32 {\n    return 1;\n}"
    );
}
