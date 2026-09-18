//! End-to-end tests for the deferred imperative constructs: arrays (type +
//! literal + indexing), enum payloads (tuple + struct, capability-gated), and
//! structured `use` (bare / selective / aliased imports).
//!
//! There is no concrete source syntax for these yet (the parser intentionally
//! lags), so each test builds the AST directly and transpiles it with the real
//! `rust.mdl` / `typescript.mdl` documents from `lamina-defs`, proving both
//! shipped definitions render each construct idiomatically. A separate
//! C-style-forbid test uses an inline definition that forbids enum payloads.

use std::path::PathBuf;

use lamina_core::ast::{
    Expr, Field, File, Function, Item, Meta, Primitive, Statement, Type, UseItem, Variant,
    VariantPayload, Visibility,
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

fn emit_one(item: Item, lang: &LanguageDef) -> String {
    emit(&File { items: vec![item] }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}

// A `const NAME: TYPE = VALUE;` item — a convenient way to exercise an array
// TYPE and an array-literal VALUE end-to-end through a real item.
fn konst(name: &str, ty: Type, value: Expr) -> Item {
    Item::Const {
        name: name.to_string(),
        ty,
        value,
        visibility: Visibility::Private,
        meta: Meta::new(),
    }
}

// =======================================================================
// Part 1 — Arrays
// =======================================================================

#[test]
fn array_literal_renders_both_targets() {
    // `const XS: [i32; 3] = [1, 2, 3];`
    let ty = Type::Array {
        elem: Box::new(i32t()),
        len: Some("3".to_string()),
    };
    let value = Expr::ArrayLit {
        elems: vec![int("1"), int("2"), int("3")],
        meta: Meta::new(),
    };
    let item = konst("XS", ty, value);
    assert_eq!(
        emit_one(item.clone(), &rust()),
        "const XS: [i32; 3] = [1, 2, 3];"
    );
    assert_eq!(
        emit_one(item, &ts()),
        "const XS: number[] = [1, 2, 3];"
    );
}

#[test]
fn unsized_array_type_renders_both_targets() {
    // A slice element type `[i32]` (Rust) / `number[]` (TS).
    let ty = Type::Array {
        elem: Box::new(i32t()),
        len: None,
    };
    let item = konst("XS", ty, Expr::ArrayLit { elems: vec![], meta: Meta::new() });
    assert_eq!(emit_one(item.clone(), &rust()), "const XS: [i32] = [];");
    assert_eq!(emit_one(item, &ts()), "const XS: number[] = [];");
}

#[test]
fn array_indexing_renders() {
    // `const X: i32 = xs[0];` — indexing already exists; confirm it renders.
    let value = Expr::Index {
        obj: Box::new(Expr::Ref("xs".to_string())),
        index: Box::new(int("0")),
    };
    let item = konst("X", i32t(), value);
    assert_eq!(emit_one(item.clone(), &rust()), "const X: i32 = xs[0];");
    assert_eq!(emit_one(item, &ts()), "const X: number = xs[0];");
}

#[test]
fn nested_array_type_composes() {
    // `[[i32; 2]; 2]` in Rust, `number[][]` in TS.
    let inner = Type::Array {
        elem: Box::new(i32t()),
        len: Some("2".to_string()),
    };
    let ty = Type::Array {
        elem: Box::new(inner),
        len: Some("2".to_string()),
    };
    let item = konst("M", ty, Expr::ArrayLit { elems: vec![], meta: Meta::new() });
    assert_eq!(emit_one(item.clone(), &rust()), "const M: [[i32; 2]; 2] = [];");
    assert_eq!(emit_one(item, &ts()), "const M: number[][] = [];");
}

// =======================================================================
// Part 2 — Enum payloads
// =======================================================================

fn variant(name: &str, payload: VariantPayload) -> Variant {
    Variant {
        name: name.to_string(),
        payload,
        meta: Meta::new(),
    }
}

fn payload_enum() -> Item {
    // enum Shape { Empty, Circle(i32), Rect { w: i32, h: i32 } }
    Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        variants: vec![
            variant("Empty", VariantPayload::None),
            variant("Circle", VariantPayload::Tuple(vec![i32t()])),
            variant(
                "Rect",
                VariantPayload::Struct(vec![
                    Field {
                        name: "w".to_string(),
                        ty: i32t(),
                        visibility: Visibility::Private,
                        meta: Meta::new(),
                    },
                    Field {
                        name: "h".to_string(),
                        ty: i32t(),
                        visibility: Visibility::Private,
                        meta: Meta::new(),
                    },
                ]),
            ),
        ],
        meta: Meta::new(),
    }
}

#[test]
fn payload_enum_renders_rust() {
    let out = emit_one(payload_enum(), &rust());
    assert_eq!(
        out,
        "pub enum Shape {\n    Empty,\n    Circle(i32),\n    Rect { w: i32, h: i32 },\n}",
        "got: {out}"
    );
}

#[test]
fn payload_enum_renders_typescript_as_union() {
    let out = emit_one(payload_enum(), &ts());
    assert_eq!(
        out,
        "export type Shape =\n    { tag: \"Empty\" } | { tag: \"Circle\", values: [number] } | { tag: \"Rect\", w: number, h: number };",
        "got: {out}"
    );
}

#[test]
fn payloadless_enum_still_renders_plain_both_targets() {
    // A payloadless enum must render exactly as before this feature.
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        attributes: Vec::new(),
        variants: vec![
            variant("Red", VariantPayload::None),
            variant("Green", VariantPayload::None),
        ],
        meta: Meta::new(),
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
fn tuple_variant_construction_reuses_call() {
    // A payload-bearing variant is *constructed* via existing expressions: a
    // tuple variant `Circle(5)` is an `Expr::Call`.
    let value = Expr::Call {
        callee: Box::new(Expr::Ref("Circle".to_string())),
        args: vec![int("5")],
    };
    let item = konst("C", Type::Named("Shape".to_string()), value);
    assert_eq!(emit_one(item.clone(), &rust()), "const C: Shape = Circle(5);");
    assert_eq!(emit_one(item, &ts()), "const C: Shape = Circle(5);");
}

#[test]
fn c_style_target_forbids_payload_variant() {
    use lamina_core::lang_doc::parse_language_def;
    // A minimal C-like def whose `## Enum` renders unit variants but FORBIDS
    // tuple/struct payloads (C's plain integer enumerators carry no data).
    let def = concat!(
        "# Lamina Language Definition: clike\n\n",
        "## Function\n\n",
        "```template\nfn {name}() {{ {body} }}\n```\n\n",
        "### statement\n```template\nreturn;\n```\n\n",
        "## Enum\n\n",
        "```template\nenum {name} {{ {variants} }}\n```\n\n",
        "### variant\n\n",
        "| When            | Template |\n",
        "|-----------------|----------|\n",
        "| variant is unit && first | \"{name}\" |\n",
        "| variant is unit | \", {name}\" |\n",
        "| else            | forbid |\n\n",
        "## Capabilities\n\n",
        "| Primitive | Action | Target |\n",
        "| i8 | identity | int |\n| i16 | identity | int |\n| i32 | identity | int |\n",
        "| i64 | identity | int |\n| i128 | identity | int |\n| u8 | identity | int |\n",
        "| u16 | identity | int |\n| u32 | identity | int |\n| u64 | identity | int |\n",
        "| u128 | identity | int |\n| isize | identity | int |\n| usize | identity | int |\n",
        "| f16 | identity | float |\n| bf16 | identity | float |\n| f32 | identity | float |\n",
        "| f64 | identity | double |\n| f128 | identity | double |\n| bool | identity | bool |\n",
        "| void | alias | void |\n| never | alias | void |\n| byte | alias | char |\n",
        "| bytes | wrap | Bytes |\n| char | identity | char |\n| str | wrap | String |\n",
        "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
    );
    let lang = parse_language_def(def).expect("clike def parses");

    // A payloadless enum still renders on the C-like target.
    let plain = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Private,
        attributes: Vec::new(),
        variants: vec![
            variant("Red", VariantPayload::None),
            variant("Green", VariantPayload::None),
        ],
        meta: Meta::new(),
    };
    assert_eq!(emit_one(plain, &lang), "enum Color { Red, Green }");

    // A payload-bearing variant is a forbidden construct on the C-like target.
    let with_payload = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Private,
        attributes: Vec::new(),
        variants: vec![variant("Circle", VariantPayload::Tuple(vec![i32t()]))],
        meta: Meta::new(),
    };
    let err = emit(&File { items: vec![with_payload] }, &lang)
        .expect_err("payload variant must be forbidden");
    assert!(
        matches!(err, lamina_core::EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}

// =======================================================================
// Part 3 — Structured use
// =======================================================================

#[test]
fn bare_use_is_byte_identical() {
    // The bare-path form must render exactly as before structured use existed.
    let item = Item::Use {
        path: "std::io".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_one(item.clone(), &rust()), "use std::io;");

    let ts_item = Item::Use {
        path: "\"fs\"".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_one(ts_item, &ts()), "import \"fs\";");
}

#[test]
fn aliased_use_renders_both_targets() {
    let item = Item::Use {
        path: "std::collections".to_string(),
        items: vec![],
        alias: Some("cols".to_string()),
        meta: Meta::new(),
    };
    assert_eq!(emit_one(item, &rust()), "use std::collections as cols;");

    let ts_item = Item::Use {
        path: "\"react\"".to_string(),
        items: vec![],
        alias: Some("React".to_string()),
        meta: Meta::new(),
    };
    assert_eq!(
        emit_one(ts_item, &ts()),
        "import * as React from \"react\";"
    );
}

fn use_item(name: &str, alias: Option<&str>) -> UseItem {
    UseItem {
        name: name.to_string(),
        alias: alias.map(str::to_string),
        meta: Meta::new(),
    }
}

#[test]
fn selective_use_renders_both_targets() {
    // `use path::{a, b as c}` / `import { a, b as c } from path;`
    let items = vec![use_item("a", None), use_item("b", Some("c"))];

    let rust_item = Item::Use {
        path: "std::io".to_string(),
        items: items.clone(),
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_one(rust_item, &rust()),
        "use std::io::{a, b as c};"
    );

    let ts_item = Item::Use {
        path: "\"./mod\"".to_string(),
        items,
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_one(ts_item, &ts()),
        "import { a, b as c } from \"./mod\";"
    );
}

// =======================================================================
// A mixed file exercising all three constructs together, order preserved
// =======================================================================

#[test]
fn mixed_file_all_constructs_rust() {
    let items = vec![
        Item::Use {
            path: "std::io".to_string(),
            items: vec![use_item("Read", None)],
            alias: None,
            meta: Meta::new(),
        },
        payload_enum(),
        konst(
            "XS",
            Type::Array {
                elem: Box::new(i32t()),
                len: Some("2".to_string()),
            },
            Expr::ArrayLit {
                elems: vec![int("1"), int("2")],
                meta: Meta::new(),
            },
        ),
        Item::Function(Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(Primitive::Void),
            body: vec![Statement::Return(None)],
            meta: Meta::new(),
        }),
    ];
    let out = emit(&File { items }, &rust()).expect("emit");
    assert_eq!(
        out,
        concat!(
            "use std::io::{Read};\n\n",
            "pub enum Shape {\n    Empty,\n    Circle(i32),\n    Rect { w: i32, h: i32 },\n}\n\n",
            "const XS: [i32; 2] = [1, 2];\n\n",
            "fn f() {\n    return;\n}"
        ),
        "got: {out}"
    );
}
