//! Integration test for the switch enum-payload-binding kernel extension
//! (spec 07): a `switch` case may bind the matched variant's payload into
//! arm-scoped locals — tuple payloads positionally (`case Circle(r)`), struct
//! payloads by field name (`case Rect { w, h }`).
//!
//! These use small, self-contained fixture definitions parsed with the real
//! `parse_language_def`, so they exercise the engine's binding exposure
//! directly (independent of any shipped def): the closed `case_has_bindings` /
//! `case_binds is <kind>` facts, the projected `{bindings:binding}` sequence
//! looping a `### binding` item slot in the new `CaseBinding` element scope
//! (exposing `{name}`, `{field}`, and the `{index}` ordinal with `first`/`last`
//! loop facts), and the resolution of bound names as plain value refs in the
//! arm body. The engine stays DUMB: it exposes only the binding DATA; a def
//! chooses native-pattern vs generated-extraction. Every property is checked
//! against a fixture def's EXACT output.
//!
//! Strictly additive: a `CaseBindings::None` case renders byte-identically to a
//! case built before binding existed (the `none_binding_*` test).

use lamina_core::ast::{
    CaseBindings, CaseFieldBind, Expr, File, Function, Item, Meta, Primitive, Statement,
    SwitchCase, Type, Visibility,
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

/// A fixture def covering BOTH binding strategies the spec names:
/// - a **native-pattern** target (`positional` → `Circle(r, g)` in the case
///   head, body references the name directly), and
/// - a **generated-extraction** target (`named` → the def emits `let`-style
///   extraction locals at arm entry via a dedicated `### extract` item slot,
///   then the body references the local).
///
/// This single def proves the engine exposes enough data (name, kind, ordinal,
/// source field) for both without the engine knowing which strategy a target
/// uses.
fn fixture() -> LanguageDef {
    let doc = format!(
        concat!(
            "# Lamina Language Definition: binder\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: binder\ntarget-version: t\n```\n\n",
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
            "| stmt is switch | \"switch {{scrutinee}} {{{{\\n    {{cases}}\\n}}}}\" |\n",
            "| stmt is return | \"return {{value}};\" |\n",
            "| stmt is break  | \"break;\" |\n",
            "| else           | forbid |\n\n",
            // A positional case renders a NATIVE pattern (binding in the head);
            // a named case renders GENERATED extraction locals at arm entry
            // (via `{bindings:extract}`), then the ordinary body. An unbound
            // case takes the `else` row — byte-identical to pre-binding output.
            "### switch_case\n",
            "| When                     | Template |\n",
            "|--------------------------|----------|\n",
            "| case_binds is positional | \"case {{value}}({{bindings:binding}}): {{{{\\n    {{body}}\\n}}}}\" |\n",
            "| case_has_bindings        | \"case {{value}}: {{{{\\n    {{bindings:extract}}\\n    {{body}}\\n}}}}\" |\n",
            "| else                     | \"case {{value}}: {{{{\\n    {{body}}\\n}}}}\" |\n\n",
            // Native-pattern item slot: name + ordinal, comma-separated.
            "### binding\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{{name}}\" |\n",
            "| else  | \", {{name}}\" |\n\n",
            // Generated-extraction item slot (named bindings): one local per
            // field, reading the source `{field}` into the local `{name}`.
            "### extract\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"let {{name}} = payload.{{field}};\" |\n",
            "| else  | \"\\n    let {{name}} = payload.{{field}};\" |\n\n",
            "### expr\n",
            "| When        | Template |\n",
            "|-------------|----------|\n",
            "| expr is int | \"{{value}}\" |\n",
            "| expr is ref | \"{{value}}\" |\n",
            "| else        | forbid |\n\n",
            "{caps}",
        ),
        caps = CAPS,
    );
    parse_language_def(&doc).unwrap_or_else(|e| panic!("fixture def should parse/load: {e}"))
}

fn emit_switch(cases: Vec<SwitchCase>) -> String {
    let file = File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::Void),
            body: vec![Statement::Switch {
                scrutinee: Expr::Ref("s".into()),
                cases,
                default: None,
            }],
            meta: Meta::new(),
        })],
    };
    emit(&file, &fixture()).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

#[test]
fn none_binding_is_byte_identical_to_pre_binding() {
    // A `CaseBindings::None` case renders through the `else` row — exactly the
    // output a case built before the `bindings` field existed would produce.
    let out = emit_switch(vec![SwitchCase::new(
        Expr::IntLiteral("1".into()),
        vec![Statement::Break],
    )]);
    let expected = "fn f() {\n    switch s {\n        case 1: {\n            break;\n        }\n    }\n}";
    assert_eq!(out, expected, "unbound case must be byte-identical");
}

#[test]
fn positional_binding_native_pattern_with_names_and_ordinals() {
    // A positional (tuple) binding exposes its ordered names; the def spells a
    // native pattern in the case head and the body references the bound name
    // directly (it resolves as a plain value ref).
    let out = emit_switch(vec![SwitchCase::with_bindings(
        Expr::Ref("Circle".into()),
        vec![Statement::Return(Some(Expr::Ref("r".into())))],
        CaseBindings::Positional(vec!["r".into(), "g".into()]),
    )]);
    assert!(out.contains("case Circle(r, g): {"), "got:\n{out}");
    assert!(out.contains("return r;"), "bound name resolves in body:\n{out}");
}

#[test]
fn positional_binding_index_ordinal_is_rendered() {
    // The `{index}` ordinal is bound in the per-binding element scope, so a def
    // can number members (`_0`, `_1`) for a tuple-member extraction.
    let doc_with_index = fixture_doc_with_binding_row("| first | \"{name}_{index}\" |\n| else  | \", {name}_{index}\" |\n");
    let lang = parse_language_def(&doc_with_index).expect("def parses");
    let file = switch_file(vec![SwitchCase::with_bindings(
        Expr::Ref("Pair".into()),
        vec![Statement::Break],
        CaseBindings::Positional(vec!["a".into(), "b".into()]),
    )]);
    let out = emit(&file, &lang).expect("emit");
    assert!(out.contains("case Pair(a_0, b_1): {"), "got:\n{out}");
}

#[test]
fn named_binding_generated_extraction_exposes_field_and_bind() {
    // A named (struct) binding exposes BOTH the source variant field and the
    // local bind name. The def reads `payload.<field>` into `<name>` at arm
    // entry (generated extraction); the body then references the local. Covers
    // the `{ w, h }` shorthand (field==bind) AND an explicit rename.
    let out = emit_switch(vec![SwitchCase::with_bindings(
        Expr::Ref("Rect".into()),
        vec![Statement::Return(Some(Expr::Ref("w".into())))],
        CaseBindings::Named(vec![
            CaseFieldBind::shorthand("w"),
            CaseFieldBind::new("height", "h"),
        ]),
    )]);
    assert!(out.contains("let w = payload.w;"), "shorthand extraction:\n{out}");
    assert!(out.contains("let h = payload.height;"), "renamed extraction:\n{out}");
    assert!(out.contains("return w;"), "bound name resolves in body:\n{out}");
}

#[test]
fn case_binds_dispatch_selects_strategy_per_kind() {
    // One switch with three cases proves the closed facts route each binding
    // kind to its own row: positional → native pattern, named → extraction,
    // none → plain case.
    let out = emit_switch(vec![
        SwitchCase::with_bindings(
            Expr::Ref("Circle".into()),
            vec![Statement::Break],
            CaseBindings::Positional(vec!["r".into()]),
        ),
        SwitchCase::with_bindings(
            Expr::Ref("Rect".into()),
            vec![Statement::Break],
            CaseBindings::Named(vec![CaseFieldBind::shorthand("w")]),
        ),
        SwitchCase::new(Expr::Ref("Empty".into()), vec![Statement::Break]),
    ]);
    assert!(out.contains("case Circle(r): {"), "positional row:\n{out}");
    assert!(out.contains("let w = payload.w;"), "named extraction row:\n{out}");
    assert!(out.contains("case Empty: {"), "unbound else row:\n{out}");
}

// ---- fixture helpers ------------------------------------------------------

fn switch_file(cases: Vec<SwitchCase>) -> File {
    File {
        items: vec![Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::Void),
            body: vec![Statement::Switch {
                scrutinee: Expr::Ref("s".into()),
                cases,
                default: None,
            }],
            meta: Meta::new(),
        })],
    }
}

/// Rebuilds the fixture doc with a custom `### binding` table body, so a test
/// can exercise `{index}` in the positional-binding item slot.
fn fixture_doc_with_binding_row(binding_rows: &str) -> String {
    let base = format!(
        concat!(
            "# Lamina Language Definition: binder2\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: binder2\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{\n    {{body}}\n}}}}\n```\n\n",
            "### statement\n| When | Template |\n|------|----------|\n| first | \"{{stmt}}\" |\n| else | \"\\n{{stmt}}\" |\n\n",
            "### stmt\n| When | Template |\n|------|----------|\n",
            "| stmt is switch | \"switch {{scrutinee}} {{{{\\n    {{cases}}\\n}}}}\" |\n",
            "| stmt is break | \"break;\" |\n| else | forbid |\n\n",
            "### switch_case\n| When | Template |\n|------|----------|\n",
            "| case_binds is positional | \"case {{value}}({{bindings:binding}}): {{{{\\n    {{body}}\\n}}}}\" |\n",
            "| else | \"case {{value}}: {{{{\\n    {{body}}\\n}}}}\" |\n\n",
            "### binding\n| When | Template |\n|------|----------|\n{binding_rows}\n",
            "### expr\n| When | Template |\n|------|----------|\n| expr is ref | \"{{value}}\" |\n| else | forbid |\n\n",
            "{caps}",
        ),
        binding_rows = binding_rows,
        caps = CAPS,
    );
    base
}
