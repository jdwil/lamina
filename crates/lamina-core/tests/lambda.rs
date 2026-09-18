//! End-to-end tests for the `Expr::Lambda` kernel primitive — the functional-
//! core addition (a first-class anonymous function value).
//!
//! There is no concrete source syntax for lambdas yet (the parser intentionally
//! lags), so each test builds the AST directly and transpiles it with the real
//! `rust.mdl` / `typescript.mdl` documents from `lamina-defs`, proving both
//! shipped definitions render a lambda idiomatically (a Rust closure and a
//! TypeScript arrow function) and that a lambda composes as a value (e.g. as a
//! call argument, higher-order use).

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, File, Function, Item, Meta, Param, Primitive, Statement, Type, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn shipped_def(file: &str) -> LanguageDef {
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

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}

fn r(name: &str) -> Expr {
    Expr::Ref(name.to_string())
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.to_string(),
        ty,
        meta: Meta::new(),
    }
}

/// `x + 1` for a parameter named `x`.
fn x_plus_one() -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        lhs: Box::new(r("x")),
        rhs: Box::new(int("1")),
    }
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
            return_type: i32t(),
            body,
            meta: Meta::new(),
        })],
    };
    emit(&file, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

// ---- shipped defs load with the new lambda slots -----------------------

#[test]
fn shipped_defs_load_with_lambda_slots() {
    let _ = rust();
    let _ = ts();
}

// ---- Rust closure ------------------------------------------------------

#[test]
fn rust_lambda_renders_as_closure() {
    // let g = |x: i32| { return x + 1; };
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &rust());
    let expected = "fn f() -> i32 {\n    let g = |x: i32| {\n         return x + 1;\n     };\n}";
    assert_eq!(out, expected);
}

#[test]
fn rust_lambda_with_return_type_renders_annotated_closure() {
    // let g = |x: i32| -> i32 { return x + 1; };
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &rust());
    let expected =
        "fn f() -> i32 {\n    let g = |x: i32| -> i32 {\n         return x + 1;\n     };\n}";
    assert_eq!(out, expected);
}

#[test]
fn rust_multi_statement_lambda_body_renders_as_block() {
    // let g = |x: i32| { let y = x + 1; return y; };
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![
            Statement::Let {
                name: "y".to_string(),
                ty: None,
                value: Some(x_plus_one()),
            },
            Statement::Return(Some(r("y"))),
        ],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &rust());
    let expected =
        "fn f() -> i32 {\n    let g = |x: i32| {\n         let y = x + 1;\n         return y;\n     };\n}";
    assert_eq!(out, expected);
}

// ---- TypeScript arrow function -----------------------------------------

#[test]
fn ts_lambda_renders_as_arrow_function() {
    // let g = (x: number) => { return x + 1; };
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &ts());
    let expected =
        "function f(): number {\n    let g = (x: number) => {\n         return x + 1;\n     };\n}";
    assert_eq!(out, expected);
}

#[test]
fn ts_lambda_with_return_type_renders_annotated_arrow() {
    // let g = (x: number): number => { return x + 1; };
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &ts());
    let expected =
        "function f(): number {\n    let g = (x: number): number => {\n         return x + 1;\n     };\n}";
    assert_eq!(out, expected);
}

// ---- Higher-order use (lambda as a call argument) ----------------------

#[test]
fn rust_lambda_as_call_argument() {
    // return apply(|x: i32| { return x + 1; });
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let call = Expr::Call {
        callee: Box::new(r("apply")),
        args: vec![lambda],
    };
    let body = vec![Statement::Return(Some(call))];
    let out = emit_body(body, &rust());
    let expected =
        "fn f() -> i32 {\n    return apply(|x: i32| {\n        return x + 1;\n    });\n}";
    assert_eq!(out, expected);
}

#[test]
fn ts_lambda_as_call_argument() {
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let call = Expr::Call {
        callee: Box::new(r("apply")),
        args: vec![lambda],
    };
    let body = vec![Statement::Return(Some(call))];
    let out = emit_body(body, &ts());
    let expected =
        "function f(): number {\n    return apply((x: number) => {\n        return x + 1;\n    });\n}";
    assert_eq!(out, expected);
}

// ---- No-parameter lambda -----------------------------------------------

#[test]
fn rust_zero_param_lambda() {
    // let g = || { return 1; };
    let lambda = Expr::Lambda {
        params: vec![],
        return_type: None,
        body: vec![Statement::Return(Some(int("1")))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let out = emit_body(body, &rust());
    let expected = "fn f() -> i32 {\n    let g = || {\n         return 1;\n     };\n}";
    assert_eq!(out, expected);
}

// ---- Structural equality ignores lambda metadata -----------------------

#[test]
fn lambda_structural_equality_ignores_metadata() {
    let bare = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    let mut tagged_meta = Meta::new();
    tagged_meta.set("origin", "lambda");
    let tagged = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: tagged_meta,
    };
    // Same structure, differing only in metadata -> structurally equal.
    assert_eq!(bare, tagged);

    // A different return type IS structural -> not equal.
    let with_ret = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(x_plus_one()))],
        meta: Meta::new(),
    };
    assert_ne!(bare, with_ret);
}

// ---- Dispatch kind -----------------------------------------------------

#[test]
fn lambda_kind_is_lambda() {
    let lambda = Expr::Lambda {
        params: vec![],
        return_type: None,
        body: vec![],
        meta: Meta::new(),
    };
    assert_eq!(lambda.kind().as_str(), "lambda");
}
