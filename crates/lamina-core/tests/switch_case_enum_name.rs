//! Integration test for the switch-case `enum_name` scalar slot (spec 12,
//! Part 1): the scrutinee's enum **type name**, exposed in switch-case scope so
//! a language definition can render a QUALIFIED native match arm
//! (`Shape::Circle(r) =>`) from an UNqualified source pattern.
//!
//! The slot is a plain scalar (NOT a fact): it exposes the type name the index
//! already resolves for the `scrutinee is payload_enum` fact. When the
//! scrutinee is a payload-bearing-enum ref it resolves to the enum's name; for
//! every other scrutinee (a plain integer, a payloadless enum, an unresolvable
//! expression) it resolves to the EMPTY string, so a case that uses it, and
//! every non-enum switch, stays byte-identical to before the slot existed.
//!
//! These use small, self-contained fixture definitions parsed with the real
//! `parse_language_def`, exercising the engine's slot directly (independent of
//! any shipped def). The engine stays DUMB: it only exposes a name the index
//! already knows; it never scripts and never guesses.

use lamina_core::ast::{
    Expr, File, Function, Item, Meta, Param, Primitive, Statement, SwitchCase, Type, Variant,
    VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
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

/// A fixture def whose `### switch_case` row renders `{enum_name}::{value}` —
/// so a payload-enum scrutinee produces a qualified arm (`Shape::1: …`) while a
/// non-enum scrutinee's empty `enum_name` leaves the bare `::1` prefix absent
/// of a qualifier (just `::`-less `1`, see the row below).
fn qualifying_fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: qual\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: qual\ntarget-version: t\n```\n\n",
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
            "| stmt is switch | \"switch ({{scrutinee}}) {{{{ {{cases}} }}}}\" |\n",
            "| stmt is break  | \"break;\" |\n",
            "| else           | forbid |\n\n",
            // A payload-enum case renders `Shape::1`; a non-enum case has empty
            // `enum_name` so the row renders just `1` (no qualifier).
            "### switch_case\n",
            "| When | Template |\n",
            "|------|----------|\n",
            "| else | \"case {{enum_name}}{{value}}: {{body}}\" |\n\n",
            "### expr\n",
            "| When        | Template |\n",
            "|-------------|----------|\n",
            "| expr is int | \"{{value}}\" |\n",
            "| expr is ref | \"{{value}}\" |\n",
            "| else        | forbid |\n\n",
            "## Enum\n\n",
            "```template\nenum {{name}} {{{{}}}}\n```\n\n",
            "{caps}",
        ),
        caps = CAPS,
    );
    parse_language_def(&doc).unwrap_or_else(|e| panic!("fixture def should parse/load: {e}"))
}

/// A payload-bearing enum `Shape`.
fn shape_enum() -> Item {
    Item::Enum {
        name: "Shape".into(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".into(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".into(),
                payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::F64)]),
                meta: Meta::new(),
            },
        ],
        attributes: Vec::new(),
        meta: Meta::new(),
    }
}

/// Builds a unit with the `Shape` enum and a function `f(s: scrut_ty)` whose
/// body is a single `switch` over the bare name `s` with one case.
fn switch_over_param(scrut_ty: Type) -> File {
    File {
        items: vec![
            shape_enum(),
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![Param {
                    name: "s".into(),
                    ty: scrut_ty,
                    meta: Meta::new(),
                }],
                return_type: Type::Primitive(Primitive::Void),
                body: vec![Statement::Switch {
                    scrutinee: Expr::Ref("s".into()),
                    cases: vec![SwitchCase::new(
                        Expr::IntLiteral("1".into()),
                        vec![Statement::Break],
                    )],
                    default: None,
                }],
                meta: Meta::new(),
            }),
        ],
    }
}

#[test]
fn payload_enum_scrutinee_exposes_enum_name() {
    // A switch over a `Shape`-typed parameter (a payload-bearing enum) resolves
    // `{enum_name}` to `Shape`, so the case arm is QUALIFIED.
    let file = switch_over_param(Type::Named("Shape".into()));
    let out = emit(&file, &qualifying_fixture()).expect("emit");
    assert!(
        out.contains("case Shape1:"),
        "payload-enum scrutinee must expose the enum name in the case arm:\n{out}"
    );
}

#[test]
fn integer_scrutinee_has_empty_enum_name() {
    // A switch over an `i32` parameter resolves `{enum_name}` to the EMPTY
    // string, so the case arm carries no qualifier (byte-identical to before
    // the slot existed).
    let file = switch_over_param(Type::Primitive(Primitive::I32));
    let out = emit(&file, &qualifying_fixture()).expect("emit");
    assert!(
        out.contains("case 1:"),
        "integer scrutinee must leave the enum name empty:\n{out}"
    );
    assert!(
        !out.contains("case Shape1:"),
        "integer scrutinee must NOT acquire an enum qualifier:\n{out}"
    );
}

#[test]
fn payloadless_enum_scrutinee_has_empty_enum_name() {
    // A payloadless enum is NOT a payload-bearing enum, so `{enum_name}` is
    // empty (the slot shares the `scrutinee is payload_enum` resolution path).
    let file = File {
        items: vec![
            Item::Enum {
                name: "Color".into(),
                visibility: Visibility::Public,
                variants: vec![
                    Variant {
                        name: "Red".into(),
                        payload: VariantPayload::None,
                        meta: Meta::new(),
                    },
                    Variant {
                        name: "Green".into(),
                        payload: VariantPayload::None,
                        meta: Meta::new(),
                    },
                ],
                attributes: Vec::new(),
                meta: Meta::new(),
            },
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![Param {
                    name: "c".into(),
                    ty: Type::Named("Color".into()),
                    meta: Meta::new(),
                }],
                return_type: Type::Primitive(Primitive::Void),
                body: vec![Statement::Switch {
                    scrutinee: Expr::Ref("c".into()),
                    cases: vec![SwitchCase::new(
                        Expr::IntLiteral("1".into()),
                        vec![Statement::Break],
                    )],
                    default: None,
                }],
                meta: Meta::new(),
            }),
        ],
    };
    let out = emit(&file, &qualifying_fixture()).expect("emit");
    assert!(
        out.contains("case 1:") && !out.contains("case Color1:"),
        "payloadless enum must leave the enum name empty:\n{out}"
    );
}
