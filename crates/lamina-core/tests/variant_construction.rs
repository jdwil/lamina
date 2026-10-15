//! Integration test for the context-aware `expr is variant_construction` fact
//! and its `{enum_name}` render-scope slot (spec 21, Part 1): the
//! construction-side mirror of the switch-case `enum_name` slot.
//!
//! The engine recognizes a variant construction with NO keyword, purely from
//! context: a tuple-variant construction is an `Expr::Call` whose callee is a
//! bare variant name, a struct-variant construction is an `Expr::StructLit`
//! whose type name is a variant, and a unit-variant construction is a bare
//! `Expr::Ref` to a variant. When the name resolves to a *unique* in-scope enum
//! variant, the fact is true and `{enum_name}` resolves to the owning enum;
//! every ordinary (non-variant) call/struct-lit/reference leaves the fact false
//! and `{enum_name}` empty, byte-identical to before the fact existed. An
//! ambiguous name (a variant of two enums, or both a variant and a function) is
//! a loud `AmbiguousVariantConstruction` error — the engine never guesses.
//!
//! These use small, self-contained fixture definitions parsed with the real
//! `parse_language_def`, exercising the engine directly (independent of any
//! shipped def).

use lamina_core::ast::{
    Expr, File, Function, Item, Meta, Param, Primitive, Statement, Type, Variant, VariantPayload,
    Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::error::EmitError;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

/// The 26-row capability matrix every def needs, all `identity`/`alias`/`wrap`.
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

/// A fixture def whose `### expr` branches on `expr is variant_construction`
/// (BEFORE the generic `expr is call`), rendering a qualified construction form
/// `{enum_name}::{callee}({args})` for a tuple variant and `{enum_name}::{value}`
/// for a unit variant; an ordinary call stays `{callee}({args})` byte-identical.
fn construction_fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: ctor\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: ctor\ntarget-version: t\n```\n\n",
            "## Function\n\n",
            "```template\nfn {{name}}() {{{{\n    {{body}}\n}}}}\n```\n\n",
            "### statement\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{{stmt}}\" |\n",
            "| else  | \"\\n{{stmt}}\" |\n\n",
            "### stmt\n",
            "| When           | Template |\n",
            "|----------------|----------|\n",
            "| stmt is return | \"return {{value}};\" |\n",
            "| stmt is expr   | \"{{value}};\" |\n",
            "| else           | forbid |\n\n",
            // The construction row is BEFORE the generic call row.
            "### expr\n",
            "| When                        | Template |\n",
            "|-----------------------------|----------|\n",
            "| expr is variant_construction && expr is call | \"{{enum_name}}::{{callee}}({{args}})\" |\n",
            "| expr is variant_construction && expr is ref  | \"{{enum_name}}::{{value}}\" |\n",
            "| expr is call                | \"{{callee}}({{args}})\" |\n",
            "| expr is ref                 | \"{{value}}\" |\n",
            "| expr is float               | \"{{value}}\" |\n",
            "| else                        | forbid |\n\n",
            "### expr_arg\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{{value}}\" |\n",
            "| else  | \", {{value}}\" |\n\n",
            "## Enum\n\n",
            "```template\nenum {{name}} {{{{}}}}\n```\n\n",
            "{caps}",
        ),
        caps = CAPS,
    );
    parse_language_def(&doc).unwrap_or_else(|e| panic!("fixture def should parse/load: {e}"))
}

/// An enum `Op` with a tuple variant `Add(f64)` and a unit variant `Clear`.
fn op_enum() -> Item {
    Item::Enum {
        name: "Op".into(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Add".into(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::F64)]),
                meta: Meta::new(),
            },
            Variant {
                name: "Clear".into(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: Vec::new(),
        meta: Meta::new(),
    }
}

/// Builds a unit with the `Op` enum and a function whose body is a single
/// `return <expr>;`.
fn unit_with_return(expr: Expr) -> File {
    File {
        items: vec![
            op_enum(),
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Named("Op".into()),
                body: vec![Statement::Return(Some(expr))],
                meta: Meta::new(),
            }),
        ],
    }
}

#[test]
fn tuple_variant_call_is_recognized_and_exposes_enum_name() {
    // `Add(5.0)` is a tuple-variant construction: the fact is true and the
    // enum name resolves to `Op`, so the qualified form renders.
    let file = unit_with_return(Expr::Call {
        callee: Box::new(Expr::Ref("Add".into())),
        args: vec![Expr::FloatLiteral("5.0".into())],
    });
    let out = emit(&file, &construction_fixture()).expect("emit");
    assert!(
        out.contains("return Op::Add(5.0);"),
        "tuple-variant construction must render qualified via enum_name:\n{out}"
    );
}

#[test]
fn unit_variant_ref_is_recognized() {
    // `Clear` as a bare reference is a unit-variant construction.
    let file = unit_with_return(Expr::Ref("Clear".into()));
    let out = emit(&file, &construction_fixture()).expect("emit");
    assert!(
        out.contains("return Op::Clear;"),
        "unit-variant construction must render the qualified nullary form:\n{out}"
    );
}

#[test]
fn non_variant_call_stays_byte_identical() {
    // A call to a plain top-level function (NOT an enum variant) is NOT a
    // variant construction: the fact is false and the generic call row renders.
    let file = File {
        items: vec![
            op_enum(),
            Item::Function(Function {
                name: "helper".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::F64),
                body: vec![Statement::Return(Some(Expr::FloatLiteral("1.0".into())))],
                meta: Meta::new(),
            }),
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::F64),
                body: vec![Statement::Return(Some(Expr::Call {
                    callee: Box::new(Expr::Ref("helper".into())),
                    args: vec![],
                }))],
                meta: Meta::new(),
            }),
        ],
    };
    let out = emit(&file, &construction_fixture()).expect("emit");
    assert!(
        out.contains("return helper();"),
        "a non-variant call must stay byte-identical (no enum qualifier):\n{out}"
    );
    assert!(
        !out.contains("::helper"),
        "a non-variant call must NOT acquire an enum qualifier:\n{out}"
    );
}

#[test]
fn variant_of_two_enums_is_ambiguous_error() {
    // `Dup` is a variant of BOTH `A` and `B` — constructing it is ambiguous.
    let dup_variant = || Variant {
        name: "Dup".into(),
        payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::F64)]),
        meta: Meta::new(),
    };
    let file = File {
        items: vec![
            Item::Enum {
                name: "A".into(),
                visibility: Visibility::Public,
                variants: vec![dup_variant()],
                attributes: Vec::new(),
                meta: Meta::new(),
            },
            Item::Enum {
                name: "B".into(),
                visibility: Visibility::Public,
                variants: vec![dup_variant()],
                attributes: Vec::new(),
                meta: Meta::new(),
            },
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Named("A".into()),
                body: vec![Statement::Return(Some(Expr::Call {
                    callee: Box::new(Expr::Ref("Dup".into())),
                    args: vec![Expr::FloatLiteral("1.0".into())],
                }))],
                meta: Meta::new(),
            }),
        ],
    };
    let err = emit(&file, &construction_fixture()).expect_err("ambiguous construction must error");
    match err {
        EmitError::AmbiguousVariantConstruction { name, .. } => assert_eq!(name, "Dup"),
        other => panic!("expected AmbiguousVariantConstruction, got {other:?}"),
    }
}

#[test]
fn variant_also_a_function_is_ambiguous_error() {
    // `Add` is both an `Op` variant AND a top-level function — ambiguous.
    let file = File {
        items: vec![
            op_enum(),
            Item::Function(Function {
                name: "Add".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![Param {
                    name: "x".into(),
                    ty: Type::Primitive(Primitive::F64),
                    meta: Meta::new(),
                }],
                return_type: Type::Named("Op".into()),
                body: vec![Statement::Return(Some(Expr::Ref("x".into())))],
                meta: Meta::new(),
            }),
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Named("Op".into()),
                body: vec![Statement::Return(Some(Expr::Call {
                    callee: Box::new(Expr::Ref("Add".into())),
                    args: vec![Expr::FloatLiteral("1.0".into())],
                }))],
                meta: Meta::new(),
            }),
        ],
    };
    let err = emit(&file, &construction_fixture()).expect_err("variant+function must error");
    match err {
        EmitError::AmbiguousVariantConstruction { name, reason } => {
            assert_eq!(name, "Add");
            assert!(
                reason.contains("function"),
                "reason should mention the function collision: {reason}"
            );
        }
        other => panic!("expected AmbiguousVariantConstruction, got {other:?}"),
    }
}
