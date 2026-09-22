//! Engine-level tests for Blocker #A (projected item slots on
//! field/variant/attribute/payload sequences) and Blocker #B (the per-element
//! 0-based `{index}` scalar slot).
//!
//! These use small, self-contained fixture definitions parsed with the real
//! `parse_language_def`, so they exercise the engine's projection machinery and
//! the `{index}` ordinal directly (independent of any shipped def). #A lets one
//! collection render two ways in a single declaration; #B exposes the element
//! ordinal so a template can spell `_{index}` → `_0`, `_1`, …. Both are
//! strictly additive: the default (non-projected) rendering is byte-identical,
//! covered by the shipped-def suites.

use lamina_core::ast::{
    Field, File, Item, Meta, Primitive, Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

/// The 26-row capability matrix every def needs, all `identity` for brevity.
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

fn parse(doc: &str) -> LanguageDef {
    parse_language_def(doc).unwrap_or_else(|e| panic!("fixture def should parse/load: {e}"))
}

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

fn field(name: &str) -> Field {
    Field {
        name: name.to_string(),
        ty: i32t(),
        visibility: Visibility::Public,
        meta: Meta::new(),
    }
}

fn emit_one(item: Item, lang: &LanguageDef) -> String {
    emit(&File { items: vec![item] }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

// ---- #A: a struct renders its fields TWO ways in one declaration ----------

#[test]
fn projected_field_slot_renders_fields_a_second_way() {
    // `{fields}` renders declarations; `{fields:field_eq}` re-loops the SAME
    // field list through a second item slot (field-wise equality). This is the
    // motivating C-attribute-helper case, exercised here in isolation.
    let def = format!(
        concat!(
            "# Lamina Language Definition: proj\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: proj\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Struct\n\n",
            "```template\nstruct {{name}} {{{{ {{fields}} }}}} eq: {{fields:field_eq}}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n",
            "| first | \"{{name}}\" |\n| else | \", {{name}}\" |\n\n",
            "### field_eq\n| When | Template |\n|------|----------|\n",
            "| first | \"a.{{name}} == b.{{name}}\" |\n| else | \" && a.{{name}} == b.{{name}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let lang = parse(&def);
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x"), field("y")],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_one(s, &lang),
        "struct Point { x, y } eq: a.x == b.x && a.y == b.y"
    );
}

// ---- #A: a projected item slot missing its subsection is a load error -----

#[test]
fn projected_field_slot_missing_subsection_is_load_error() {
    let def = format!(
        concat!(
            "# Lamina Language Definition: proj2\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: proj2\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Struct\n\n",
            "```template\nstruct {{name}} {{{{ {{fields:missing}} }}}}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n| else | \"{{name}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let err = parse_language_def(&def).expect_err("a projection to a missing item slot must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("missing") || msg.contains("projection") || msg.contains("item"),
        "expected a missing-projection-item-slot error, got: {msg}"
    );
}

// ---- #A: projected VARIANT slot ------------------------------------------

#[test]
fn projected_variant_slot_renders_variants_a_second_way() {
    let def = format!(
        concat!(
            "# Lamina Language Definition: pv\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: pv\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Enum\n\n",
            "```template\nenum {{name}} {{{{ {{variants}} }}}} tags: {{variants:tag}}\n```\n\n",
            "### variant\n| When | Template |\n|------|----------|\n",
            "| first | \"{{name}}\" |\n| else | \", {{name}}\" |\n\n",
            "### tag\n| When | Template |\n|------|----------|\n",
            "| first | \"{{name}}Tag\" |\n| else | \"/{{name}}Tag\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let lang = parse(&def);
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant { name: "Red".into(), payload: VariantPayload::None, meta: Meta::new() },
            Variant { name: "Green".into(), payload: VariantPayload::None, meta: Meta::new() },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_one(e, &lang),
        "enum Color { Red, Green } tags: RedTag/GreenTag"
    );
}

// ---- #A: projected ATTRIBUTE slot ----------------------------------------

#[test]
fn projected_attribute_slot_renders_attributes_a_second_way() {
    let def = format!(
        concat!(
            "# Lamina Language Definition: pa\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: pa\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Struct\n\n",
            "```template\nstruct {{name}} {{{{ {{fields}} }}}}{{attributes:tag}}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n| else | \"{{name}};\" |\n\n",
            "### attribute\n| When | Template |\n|------|----------|\n| else | \" {{name}}\" |\n\n",
            "### tag\n| When | Template |\n|------|----------|\n| first | \" /* {{name}} */\" |\n| else | \", {{name}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let lang = parse(&def);
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x")],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    // The projected `tag` slot renders each attribute's canonical spelling.
    assert_eq!(
        emit_one(s, &lang),
        "struct P { x; } /* equatable */, displayable"
    );
}

// ---- #B: the per-element {index} scalar (variant payload / fields) --------

#[test]
fn index_slot_numbers_tuple_payload_members() {
    // The 0-based `{index}` ordinal lets a tuple payload's members be numbered
    // `_0`, `_1`, … — the C tagged-union case, isolated here.
    let def = format!(
        concat!(
            "# Lamina Language Definition: idx\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: idx\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Enum\n\n",
            "```template\nenum {{name}} {{{{ {{variants}} }}}}\n```\n\n",
            "### variant\n| When | Template |\n|------|----------|\n",
            "| variant is tuple | \"{{name}}({{payload_types:numbered}})\" |\n| else | \"{{name}}\" |\n\n",
            "### numbered\n| When | Template |\n|------|----------|\n",
            "| first | \"{{type}} _{{index}}\" |\n| else | \", {{type}} _{{index}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let lang = parse(&def);
    let e = Item::Enum {
        name: "E".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "T".into(),
            payload: VariantPayload::Tuple(vec![i32t(), i32t(), i32t()]),
            meta: Meta::new(),
        }],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(emit_one(e, &lang), "enum E { T(i32 _0, i32 _1, i32 _2) }");
}

#[test]
fn index_slot_numbers_struct_fields() {
    // `{index}` is also exposed in the field element scope.
    let def = format!(
        concat!(
            "# Lamina Language Definition: idxf\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: idxf\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Struct\n\n```template\nstruct {{name}} {{{{ {{fields}} }}}}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n",
            "| first | \"{{name}}@{{index}}\" |\n| else | \", {{name}}@{{index}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let lang = parse(&def);
    let s = Item::Struct {
        name: "S".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("a"), field("b"), field("c")],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(emit_one(s, &lang), "struct S { a@0, b@1, c@2 }");
}

// ---- #B: {index} is not bound outside a looped scope (load-time reject) ---

#[test]
fn index_slot_is_rejected_in_non_looped_scope() {
    // `{index}` is bound only in looped element scopes; referencing it on the
    // struct node itself (not an element) is an unknown-slot load error, keeping
    // binding⇔resolution consistent.
    let def = format!(
        concat!(
            "# Lamina Language Definition: idxbad\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: idxbad\ntarget-version: t\n```\n\n",
            "## Function\n\n```template\nfn {{name}}() {{{{ {{body}} }}}}\n```\n\n",
            "### statement\n```template\n;\n```\n\n",
            "## Struct\n\n```template\nstruct {{name}}{{index}} {{{{ {{fields}} }}}}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n| else | \"{{name}}\" |\n\n",
            "{caps}"
        ),
        caps = CAPS
    );
    let err = parse_language_def(&def)
        .expect_err("{index} in a non-looped scope must be an unknown-slot load error");
    let msg = err.to_string();
    assert!(
        msg.contains("index") || msg.contains("unknown"),
        "expected an unknown-slot error for stray {{index}}, got: {msg}"
    );
}
