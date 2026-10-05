//! Integration test for the switch-scrutinee tagged-enum signal (spec 08):
//! the closed `scrutinee is payload_enum` fact, which tells a language
//! definition whether a `switch`'s scrutinee resolves to a **payload-bearing
//! enum** value (so a tagged-union / sealed / discriminated target dispatches
//! on the discriminant in the header) versus a plain integer / unresolvable
//! scrutinee (which keeps the bare-dispatch form, byte-identical to before).
//!
//! These use small, self-contained fixture definitions parsed with the real
//! `parse_language_def`, so they exercise the engine's new signal directly
//! (independent of any shipped def). The engine stays DUMB: it computes ONE
//! boolean from existing type resolution (the function's parameter / typed-let
//! value-type scope and top-level consts); it never scripts and never guesses
//! — an unresolvable scrutinee answers `false`.
//!
//! Strictly additive: a definition that never references the fact, or a switch
//! whose scrutinee is not a payload-bearing enum, renders byte-identically to
//! before the fact existed (the `*_byte_identical` tests).

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

/// A fixture def whose `### stmt` switch row branches on the new fact: a
/// payload-enum scrutinee dispatches on `.tag` in the header; every other
/// scrutinee keeps the bare `switch (scrut)` form.
fn tagged_fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: tagd\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: tagd\ntarget-version: t\n```\n\n",
            "## Function\n\n",
            "```template\nfn {{name}}() {{{{\n    {{body}}\n}}}}\n```\n\n",
            "### statement\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{{stmt}}\" |\n",
            "| else  | \"\\n{{stmt}}\" |\n\n",
            "### stmt\n",
            "| When                        | Template |\n",
            "|-----------------------------|----------|\n",
            "| stmt is switch && scrutinee is payload_enum | \"switch ({{scrutinee}}.tag) {{{{ {{cases}} }}}}\" |\n",
            "| stmt is switch              | \"switch ({{scrutinee}}) {{{{ {{cases}} }}}}\" |\n",
            "| stmt is break               | \"break;\" |\n",
            "| else                        | forbid |\n\n",
            "### switch_case\n",
            "| When | Template |\n",
            "|------|----------|\n",
            "| else | \"case {{value}}: {{body}}\" |\n\n",
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

/// A fixture def that NEVER references the new fact — only the plain bare
/// `switch (scrut)` form. Used to prove the fact is strictly additive: the
/// tagged fixture's FALSE branch renders byte-identically to this def.
fn plain_fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: plain\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: plain\ntarget-version: t\n```\n\n",
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
            "### switch_case\n",
            "| When | Template |\n",
            "|------|----------|\n",
            "| else | \"case {{value}}: {{body}}\" |\n\n",
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
/// body is a single `switch` over the bare name `s`.
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
fn payload_enum_scrutinee_selects_tagged_header() {
    // A switch over a `Shape`-typed parameter (a payload-bearing enum) answers
    // the fact TRUE, selecting the `.tag` discriminant-dispatch header.
    let file = switch_over_param(Type::Named("Shape".into()));
    let out = emit(&file, &tagged_fixture()).expect("emit");
    assert!(
        out.contains("switch (s.tag) {"),
        "payload-enum scrutinee must dispatch on `.tag`:\n{out}"
    );
    assert!(
        !out.contains("switch (s) {"),
        "the bare form must NOT be selected:\n{out}"
    );
}

#[test]
fn integer_scrutinee_selects_bare_header() {
    // A switch over an `i32` parameter answers the fact FALSE, keeping the bare
    // `switch (s)` form.
    let file = switch_over_param(Type::Primitive(Primitive::I32));
    let out = emit(&file, &tagged_fixture()).expect("emit");
    assert!(
        out.contains("switch (s) {"),
        "integer scrutinee must keep the bare dispatch:\n{out}"
    );
    assert!(
        !out.contains(".tag"),
        "integer scrutinee must NOT dispatch on `.tag`:\n{out}"
    );
}

#[test]
fn false_branch_is_byte_identical_to_a_fact_free_def() {
    // The KEY additivity check: the tagged fixture's FALSE branch (integer
    // scrutinee) renders the EXACT SAME bytes as a definition that never
    // references the fact at all.
    let file = switch_over_param(Type::Primitive(Primitive::I32));
    let via_tagged = emit(&file, &tagged_fixture()).expect("emit tagged");
    let via_plain = emit(&file, &plain_fixture()).expect("emit plain");
    assert_eq!(
        via_tagged, via_plain,
        "a non-payload-enum switch must be byte-identical with or without the fact"
    );
}

#[test]
fn payloadless_enum_scrutinee_is_not_tagged() {
    // An enum whose every variant is a unit is NOT a payload-bearing enum, so
    // its switch keeps the bare form (it needs no discriminant dispatch).
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
    let out = emit(&file, &tagged_fixture()).expect("emit");
    assert!(
        out.contains("switch (c) {") && !out.contains(".tag"),
        "payloadless enum keeps the bare switch:\n{out}"
    );
}
