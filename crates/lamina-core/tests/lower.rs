//! Tests for the minimal target-aware lowering pass ([`lamina_core::lower`]).
//!
//! These build raw ASTs directly (NOT via the parser) and assert `lower`:
//! - resolves a grouped multi-arm `Raw` to the arm matching the build target;
//! - falls back to the `else` arm when no target arm matches;
//! - fails with [`LowerError::NoRawArmForTarget`] when there is no match and no
//!   `else` (raw is never silently dropped);
//! - leaves a non-raw AST structurally unchanged (pass-through).
//!
//! A resolved raw node collapses to a single arm whose `code` is the selected
//! arm's verbatim text, so structural equality against the matching
//! single-arm [`RawArm`] confirms the resolution.

use lamina_core::ast::{Expr, File, Function, Item, Meta, Primitive, RawArm, Statement, Type, Visibility};
use lamina_core::lower::lower;
use lamina_core::LowerError;

/// Wraps `body` in `fn f() -> i32 { … }` so a raw statement can be lowered in a
/// realistic position.
fn func_with_body(body: Vec<Statement>) -> File {
    File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::I32),
            body,
            meta: Meta::new(),
        })],
    }
}

/// A grouped raw statement with `rust` + `python` arms and an `else` fallback.
fn grouped_raw_stmt() -> Statement {
    Statement::Raw {
        arms: vec![
            RawArm {
                target: "rust".to_string(),
                version: None,
                code: "RUST_CODE;".to_string(),
            },
            RawArm {
                target: "python".to_string(),
                version: None,
                code: "PYTHON_CODE".to_string(),
            },
        ],
        default: Some("FALLBACK;".to_string()),
        meta: Meta::new(),
    }
}

/// Extracts the single function body from a lowered `File`.
fn body_of(file: &File) -> &[Statement] {
    match &file.items[0] {
        Item::Function(f) => &f.body,
        other => panic!("expected a function item, got {other:?}"),
    }
}

#[test]
fn grouped_raw_lowers_to_matching_target_arm() {
    let file = func_with_body(vec![grouped_raw_stmt()]);
    let lowered = lower(&file, "rust").expect("lower for rust");
    // The resolved node is a single-arm raw carrying the `rust` arm's code.
    assert_eq!(body_of(&lowered)[0], Statement::raw("RUST_CODE;"));

    let lowered_py = lower(&file, "python").expect("lower for python");
    assert_eq!(body_of(&lowered_py)[0], Statement::raw("PYTHON_CODE"));
}

#[test]
fn raw_falls_back_to_else_when_no_arm_matches() {
    let file = func_with_body(vec![grouped_raw_stmt()]);
    // `c` has no arm, so the `else` fallback is selected.
    let lowered = lower(&file, "c").expect("lower for c (uses else)");
    assert_eq!(body_of(&lowered)[0], Statement::raw("FALLBACK;"));
}

#[test]
fn raw_no_match_no_else_is_a_lower_error() {
    // No `else` fallback this time.
    let stmt = Statement::Raw {
        arms: vec![RawArm {
            target: "rust".to_string(),
            version: None,
            code: "RUST_CODE;".to_string(),
        }],
        default: None,
        meta: Meta::new(),
    };
    let file = func_with_body(vec![stmt]);
    let err = lower(&file, "python").expect_err("no arm and no else must fail");
    match err {
        LowerError::NoRawArmForTarget { target, declared } => {
            assert_eq!(target, "python");
            assert_eq!(declared, "rust");
        }
    }
}

#[test]
fn expr_and_item_raw_lower_too() {
    // A raw EXPRESSION (as a returned value) and a raw ITEM both lower.
    let expr_raw = Expr::Raw {
        arms: vec![
            RawArm {
                target: "rust".to_string(),
                version: Some(">=1.70".to_string()),
                code: "x.sqrt()".to_string(),
            },
            RawArm {
                target: "ts".to_string(),
                version: None,
                code: "Math.sqrt(x)".to_string(),
            },
        ],
        default: None,
        meta: Meta::new(),
    };
    let item_raw = Item::Raw {
        arms: vec![RawArm {
            target: "rust".to_string(),
            version: None,
            code: "// prelude".to_string(),
        }],
        default: None,
        meta: Meta::new(),
    };
    let file = File {
        items: vec![
            item_raw,
            Item::Function(Function {
                name: "f".to_string(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::I32),
                body: vec![Statement::Return(Some(expr_raw))],
                meta: Meta::new(),
            }),
        ],
    };
    let lowered = lower(&file, "rust").expect("lower for rust");
    assert_eq!(lowered.items[0], Item::raw("// prelude"));
    match &lowered.items[1] {
        Item::Function(f) => {
            assert_eq!(f.body[0], Statement::Return(Some(Expr::raw("x.sqrt()"))));
        }
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn non_raw_ast_passes_through_unchanged() {
    // A program with NO raw nodes lowers to a structurally identical `File`.
    let file = func_with_body(vec![
        Statement::Let {
            name: "x".to_string(),
            ty: Some(Type::Primitive(Primitive::I32)),
            value: Some(Expr::IntLiteral("1".to_string())),
        },
        Statement::Return(Some(Expr::Ref("x".to_string()))),
    ]);
    let lowered = lower(&file, "rust").expect("lower passes through");
    assert_eq!(lowered, file, "non-raw AST must be unchanged by lower");
}
