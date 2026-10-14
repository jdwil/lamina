//! Engine tests for INFERRED mutability (the removed `mut` keyword).
//!
//! There is no `mut` keyword in the source and no `mutable` field on the AST.
//! The engine populates the closed `mut` fact (`stmt is let && mut`, which the
//! Rust/Kotlin defs consume) by INFERENCE: a `let` binding is mutable iff its
//! name is the ROOT of some assignment target anywhere in its scope — a bare
//! reassignment (`x = …`), a compound (`x = x + 1`, which parses to a plain
//! `Assign`), or a field/index assignment (`x.f = …` / `x[i] = …`, all of which
//! require `x` be mutable in Rust/Kotlin). The scan descends into nested
//! statement blocks (`if`/`while`/`for`/`foreach`/`switch` arms), so a binding
//! reassigned only inside a nested loop still infers mutable.
//!
//! These use a small, self-contained fixture def parsed with the real
//! `parse_language_def` whose `let` row renders `let mut <name>` when the `mut`
//! fact holds and `let <name>` otherwise — so the emitted text directly reveals
//! the inferred fact, independent of any shipped def.

use lamina_core::ast::{Expr, File, Function, Item, Meta, Primitive, Statement, Type, Visibility};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

/// The 26-row capability matrix every def needs.
const CAPS: &str = concat!(
    "## Capabilities\n\n",
    "| Primitive | Action | Target |\n",
    "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
    "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
    "| void | alias | void |\n| never | alias | never |\n| byte | alias | u8 |\n",
    "| bytes | wrap | Bytes |\n| char | identity | char |\n| str | wrap | Str |\n",
    "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
);

/// A fixture def whose `### statement` `let` row branches on the closed `mut`
/// fact: an inferred-mutable binding renders `let mut <name>`, an immutable one
/// renders `let <name>`. Assignments and expression-statements render verbatim
/// so the scan has reassignments to find.
fn mut_fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: mutd\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: mutd\ntarget-version: t\n```\n\n",
            "## Function\n\n",
            "```template\nfn {{name}}() {{{{\n    {{body}}\n}}}}\n```\n\n",
            "### statement\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{{stmt}}\" |\n",
            "| else  | \"\\n{{stmt}}\" |\n\n",
            "### stmt\n",
            "| When                  | Template |\n",
            "|-----------------------|----------|\n",
            "| stmt is let && mut    | \"let mut {{name}} = {{value}};\" |\n",
            "| stmt is let           | \"let {{name}} = {{value}};\" |\n",
            "| stmt is assign        | \"{{target}} = {{value}};\" |\n",
            "| stmt is while         | \"while ({{cond}}) {{{{ {{body}} }}}}\" |\n",
            "| stmt is if            | \"if ({{cond}}) {{{{ {{then}} }}}}\" |\n",
            "| else                  | forbid |\n\n",
            "### expr\n",
            "| When         | Template |\n",
            "|--------------|----------|\n",
            "| expr is int  | \"{{value}}\" |\n",
            "| expr is ref  | \"{{value}}\" |\n",
            "| expr is field | \"{{obj}}.{{field}}\" |\n",
            "| expr is index | \"{{obj}}[{{index}}]\" |\n",
            "| expr is binary | \"{{lhs}} {{op}} {{rhs}}\" |\n",
            "| else         | forbid |\n\n",
            "{caps}",
        ),
        caps = CAPS,
    );
    parse_language_def(&doc).unwrap_or_else(|e| panic!("fixture def should parse/load: {e}"))
}

/// Wraps `body` in a private `void` function `f` and emits it through the mut
/// fixture, returning the rendered body text.
fn emit_body(body: Vec<Statement>) -> String {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".into(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::Void),
            body,
            meta: Meta::new(),
        })],
    };
    emit(&file, &mut_fixture()).unwrap_or_else(|e| panic!("emit should succeed: {e}"))
}

fn int(n: &str) -> Expr {
    Expr::IntLiteral(n.into())
}

fn let_stmt(name: &str) -> Statement {
    Statement::Let {
        name: name.into(),
        ty: None,
        value: Some(int("0")),
    }
}

/// A binding reassigned later IN THE SAME block infers mutable.
#[test]
fn reassigned_in_same_block_infers_mut() {
    let out = emit_body(vec![
        let_stmt("x"),
        Statement::Assign {
            target: Expr::Ref("x".into()),
            value: int("1"),
        },
    ]);
    assert!(out.contains("let mut x = 0;"), "got: {out}");
}

/// A binding reassigned via a COMPOUND assignment (parsed to a plain `Assign`
/// whose value is a binary `x + 1`) infers mutable — the target root is `x`.
#[test]
fn compound_reassignment_infers_mut() {
    let out = emit_body(vec![
        let_stmt("x"),
        Statement::Assign {
            target: Expr::Ref("x".into()),
            value: Expr::Binary {
                op: lamina_core::ast::BinaryOp::Add,
                lhs: Box::new(Expr::Ref("x".into())),
                rhs: Box::new(int("1")),
            },
        },
    ]);
    assert!(out.contains("let mut x = 0;"), "got: {out}");
}

/// A binding reassigned ONLY inside a nested `while` body still infers mutable:
/// the scan descends into nested statement blocks.
#[test]
fn reassigned_in_nested_loop_infers_mut() {
    let out = emit_body(vec![
        let_stmt("acc"),
        Statement::While {
            cond: Expr::Ref("acc".into()),
            body: vec![Statement::Assign {
                target: Expr::Ref("acc".into()),
                value: int("1"),
            }],
        },
    ]);
    assert!(out.contains("let mut acc = 0;"), "got: {out}");
}

/// A binding reassigned ONLY inside a nested `if` body still infers mutable.
#[test]
fn reassigned_in_nested_if_infers_mut() {
    let out = emit_body(vec![
        let_stmt("acc"),
        Statement::If {
            cond: Expr::Ref("acc".into()),
            then_block: vec![Statement::Assign {
                target: Expr::Ref("acc".into()),
                value: int("1"),
            }],
            else_block: None,
        },
    ]);
    assert!(out.contains("let mut acc = 0;"), "got: {out}");
}

/// A binding NEVER reassigned infers immutable — byte-identical to a pre-`mut`
/// `let` (`let x`, not `let mut x`).
#[test]
fn never_reassigned_infers_immutable() {
    let out = emit_body(vec![let_stmt("x"), let_stmt("y")]);
    assert!(out.contains("let x = 0;"), "got: {out}");
    assert!(out.contains("let y = 0;"), "got: {out}");
    assert!(!out.contains("let mut"), "no binding should be mutable: {out}");
}

/// A FIELD assignment (`p.x = 1;`) makes the ROOT binding `p` mutable, because
/// mutating a field through `p` requires `p` be `mut`/`var` in Rust/Kotlin
/// (confirmed against real rustc/kotlinc).
#[test]
fn field_assignment_root_infers_mut() {
    let out = emit_body(vec![
        let_stmt("p"),
        Statement::Assign {
            target: Expr::Field {
                obj: Box::new(Expr::Ref("p".into())),
                field: "x".into(),
            },
            value: int("1"),
        },
    ]);
    assert!(out.contains("let mut p = 0;"), "got: {out}");
}

/// An INDEX assignment (`a[0] = 1;`) likewise makes the root binding `a`
/// mutable (its root ref is the bound name).
#[test]
fn index_assignment_root_infers_mut() {
    let out = emit_body(vec![
        let_stmt("a"),
        Statement::Assign {
            target: Expr::Index {
                obj: Box::new(Expr::Ref("a".into())),
                index: Box::new(int("0")),
            },
            value: int("1"),
        },
    ]);
    assert!(out.contains("let mut a = 0;"), "got: {out}");
}
