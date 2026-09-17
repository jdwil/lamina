//! End-to-end tests for construction (struct literals), assignment, casts,
//! function-as-value, and the Part-2 one-level structural-predicate idioms
//! (compound assignment) against the *shipped* language definitions.
//!
//! As with the other integration suites, there is no concrete source syntax for
//! these constructs yet, so the tests build the [`Statement`]/[`Expr`] ASTs
//! directly and transpile them with the real `rust.mdl` / `typescript.mdl`
//! documents from the sibling `lamina-defs` checkout.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Primitive, Statement, Type, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::error::EmitError;
use lamina_core::lang::{Capability, LanguageDef};
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout,
/// located relative to this crate's manifest dir so tests run from anywhere.
fn shipped_def(file: &str) -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // lamina
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
fn emit_body(body: Vec<Statement>, lang: &LanguageDef) -> Result<String, EmitError> {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::I32),
            body,
        })],
    };
    emit(&file, lang)
}

fn r(name: &str) -> Expr {
    Expr::Ref(name.to_string())
}

// ---- Assignment ---------------------------------------------------------

#[test]
fn assign_to_ref_renders_both_targets() {
    let body = vec![Statement::assign(r("x"), Expr::IntLiteral("1".into())).expect("lvalue")];
    assert!(emit_body(body.clone(), &rust()).unwrap().contains("x = 1;"));
    assert!(emit_body(body, &ts()).unwrap().contains("x = 1;"));
}

#[test]
fn assign_to_field_renders() {
    // `obj.count = 5;`
    let target = Expr::Field {
        obj: Box::new(r("obj")),
        field: "count".into(),
    };
    let body = vec![Statement::assign(target, Expr::IntLiteral("5".into())).expect("lvalue")];
    assert!(emit_body(body.clone(), &rust())
        .unwrap()
        .contains("obj.count = 5;"));
    assert!(emit_body(body, &ts()).unwrap().contains("obj.count = 5;"));
}

// ---- Compound-assignment idiom (Part 2) ---------------------------------

/// `x = x + 1` recognized and emitted as `x += 1` (byte-exact, both targets).
#[test]
fn compound_add_idiom_emits_plus_equals() {
    let assign = Statement::assign(
        r("x"),
        Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(r("x")),
            rhs: Box::new(Expr::IntLiteral("1".into())),
        },
    )
    .expect("lvalue");
    let out_rust = emit_body(vec![assign.clone()], &rust()).unwrap();
    assert_eq!(out_rust, "fn f() -> i32 {\n    x += 1;\n}", "got: {out_rust}");
    let out_ts = emit_body(vec![assign], &ts()).unwrap();
    assert_eq!(
        out_ts,
        "function f(): number {\n    x += 1;\n}",
        "got: {out_ts}"
    );
}

/// A def *without* the compound rows degrades to `x = x + 1;` — proving the
/// idiom is a language-def choice, not a kernel node. We simulate an
/// unsupporting def by stripping the three compound rows from the shipped rust
/// def.
#[test]
fn without_compound_rows_degrades_to_plain_assign() {
    // Read the rust def text, remove the compound-assign rows, and load the
    // stripped def directly.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.pop();
    path.push("lamina-defs");
    path.push("languages");
    path.push("rust.mdl");
    let text = std::fs::read_to_string(&path).expect("read rust.mdl");
    let stripped: String = text
        .lines()
        .filter(|l| !l.contains("value.op is"))
        .collect::<Vec<_>>()
        .join("\n");
    let lang = lamina_core::lang_doc::parse_language_def(&stripped).expect("stripped def parses");

    let assign = Statement::assign(
        r("x"),
        Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(r("x")),
            rhs: Box::new(Expr::IntLiteral("1".into())),
        },
    )
    .expect("lvalue");
    let out = emit_body(vec![assign], &lang).unwrap();
    assert_eq!(out, "fn f() -> i32 {\n    x = x + 1;\n}", "got: {out}");
}

/// A binary whose left operand is NOT the target does not trigger the compound
/// idiom — `x = y + 1` stays a plain assignment.
#[test]
fn non_matching_lhs_stays_plain_assign() {
    let assign = Statement::assign(
        r("x"),
        Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(r("y")),
            rhs: Box::new(Expr::IntLiteral("1".into())),
        },
    )
    .expect("lvalue");
    let out = emit_body(vec![assign], &rust()).unwrap();
    assert!(out.contains("x = y + 1;"), "got: {out}");
}

// ---- Casts --------------------------------------------------------------

/// Wraps `expr` in `fn f() -> i32 { return <expr>; }` and returns the rendered
/// text between `return ` and `;`.
fn emit_returned(expr: Expr, lang: &LanguageDef) -> Result<String, EmitError> {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::I32),
            body: vec![Statement::Return(Some(expr))],
        })],
    };
    let out = emit(&file, lang)?;
    let start = out.find("return ").expect("has return") + "return ".len();
    let end = out[start..].find(';').expect("semicolon") + start;
    Ok(out[start..end].to_string())
}

#[test]
fn cast_renders_per_target() {
    // `x as i64` (Rust) / `x as bigint` (TS widens i64 to bigint).
    let c = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::I64),
    };
    assert_eq!(emit_returned(c.clone(), &rust()).unwrap(), "x as i64");
    assert_eq!(emit_returned(c, &ts()).unwrap(), "x as bigint");
}

#[test]
fn cast_to_forbidden_type_errors() {
    // TypeScript forbids `f128` (no 128-bit float), so a cast to it fails via
    // the capability matrix when the target type is resolved.
    let c = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::F128),
    };
    let err = emit_returned(c, &ts()).expect_err("f128 forbidden in TS");
    assert!(
        matches!(err, EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "f128"),
        "got: {err:?}"
    );
}

// ---- Struct-literal construction ----------------------------------------

#[test]
fn struct_literal_renders_per_target() {
    // Account { balance: 0, deposit: 100 }
    let lit = Expr::StructLit {
        type_name: "Account".into(),
        fields: vec![
            FieldInit {
                name: "balance".into(),
                value: Expr::IntLiteral("0".into()),
            },
            FieldInit {
                name: "deposit".into(),
                value: Expr::IntLiteral("100".into()),
            },
        ],
    };
    assert_eq!(
        emit_returned(lit.clone(), &rust()).unwrap(),
        "Account { balance: 0, deposit: 100 }"
    );
    // TypeScript uses a bare object literal (no type name).
    assert_eq!(
        emit_returned(lit, &ts()).unwrap(),
        "{ balance: 0, deposit: 100 }"
    );
}

#[test]
fn empty_struct_literal_renders() {
    let lit = Expr::StructLit {
        type_name: "Unit".into(),
        fields: vec![],
    };
    assert_eq!(emit_returned(lit.clone(), &rust()).unwrap(), "Unit {  }");
    assert_eq!(emit_returned(lit, &ts()).unwrap(), "{  }");
}

// ---- Function-as-value (fnptr) ------------------------------------------

/// A bare function name stored into a `fnptr`-typed struct field is a
/// function-pointer value: it renders as the plain identifier. Building the
/// struct construction with a `Ref` field value proves the convention.
#[test]
fn function_name_as_fnptr_field_value_renders_as_identifier() {
    // Account { deposit: account_deposit } where `deposit` is fnptr-typed.
    let lit = Expr::StructLit {
        type_name: "Account".into(),
        fields: vec![FieldInit {
            name: "deposit".into(),
            value: r("account_deposit"),
        }],
    };
    assert_eq!(
        emit_returned(lit.clone(), &rust()).unwrap(),
        "Account { deposit: account_deposit }"
    );
    assert_eq!(
        emit_returned(lit, &ts()).unwrap(),
        "{ deposit: account_deposit }"
    );
}

/// A struct with an `fnptr`-typed field is `fnptr`-gated: on a target that
/// forbids `fnptr`, declaring the struct (whose field type is a function
/// pointer) fails. This is the transitive gating for storing a function value.
#[test]
fn fnptr_field_is_gated_by_fnptr_capability() {
    // A struct declaration with an fnptr field.
    let struct_item = Item::Struct {
        name: "Account".into(),
        visibility: Visibility::Public,
        fields: vec![Field {
            name: "deposit".into(),
            ty: Type::FnPtr {
                params: vec![Type::Primitive(Primitive::I32)],
                ret: Box::new(Type::Primitive(Primitive::Void)),
            },
            visibility: Visibility::Public,
        }],
    };
    let file = File {
        items: vec![struct_item],
    };
    // Rust wraps `fnptr` (`fn()`), so the struct declares fine.
    let out = emit(&file, &rust()).expect("rust fnptr field ok");
    assert!(out.contains("deposit: fn(i32) -> ()"), "got: {out}");

    // Force `fnptr` forbidden -> declaring the field's type fails.
    let mut lang = rust();
    lang.capabilities.insert(Primitive::Fnptr, Capability::Forbid);
    let err = emit(&file, &lang).expect_err("fnptr forbidden");
    assert!(
        matches!(err, EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "fnptr"),
        "got: {err:?}"
    );
}
