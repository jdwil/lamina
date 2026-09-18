//! End-to-end item/declaration tests against the *shipped* language
//! definitions.
//!
//! Like the statement tests, there is no concrete source syntax for the full
//! item set yet (the parser intentionally lags), so these build [`Item`] ASTs
//! directly and transpile them with the real `rust.mdl` / `typescript.mdl`
//! documents from `lamina-defs`, proving both definitions dispatch each item
//! kind to its own `## <Item>` section and render a mixed file end-to-end.

use std::path::PathBuf;

use lamina_core::ast::{
    Expr, Field, File, Function, Item, Primitive, Statement, Type, Variant, Visibility,
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

fn emit_items(items: Vec<Item>, lang: &LanguageDef) -> String {
    let file = File { items };
    emit(&file, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

// ---- shipped defs still load with the new item sections ----------------

#[test]
fn shipped_defs_load_with_item_sections() {
    let _ = rust();
    let _ = ts();
}

// ---- struct ------------------------------------------------------------

#[test]
fn struct_with_fields_renders_rust() {
    let item = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![
            Field {
                name: "x".to_string(),
                ty: i32t(),
                visibility: Visibility::Public,
                meta: lamina_core::ast::Meta::new(),
            },
            Field {
                name: "y".to_string(),
                ty: i32t(),
                visibility: Visibility::Private,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        meta: lamina_core::ast::Meta::new(),
    };
    let out = emit_items(vec![item], &rust());
    assert_eq!(
        out, "pub struct Point {\n    pub x: i32,\n    y: i32,\n}",
        "got: {out}"
    );
}

#[test]
fn struct_with_fields_renders_ts_interface() {
    let item = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![
            Field {
                name: "x".to_string(),
                ty: i32t(),
                visibility: Visibility::Public,
                meta: lamina_core::ast::Meta::new(),
            },
            Field {
                name: "y".to_string(),
                ty: i32t(),
                visibility: Visibility::Private,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        meta: lamina_core::ast::Meta::new(),
    };
    let out = emit_items(vec![item], &ts());
    // i32 widens to number in TS; a public struct maps to an exported
    // interface. Fields carry their own trailing `;` and separator newline.
    assert_eq!(
        out, "export interface Point {\n    x: number;\n    y: number;\n}",
        "got: {out}"
    );
}

#[test]
fn empty_struct_renders() {
    let item = Item::Struct {
        name: "Empty".to_string(),
        visibility: Visibility::Private,
        fields: vec![],
        meta: lamina_core::ast::Meta::new(),
    };
    let out = emit_items(vec![item], &rust());
    assert_eq!(out, "struct Empty {\n    \n}", "got: {out}");
}

// ---- enum --------------------------------------------------------------

#[test]
fn enum_of_variants_renders() {
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: lamina_core::ast::VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: lamina_core::ast::VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
            Variant {
                name: "Blue".to_string(),
                payload: lamina_core::ast::VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        meta: lamina_core::ast::Meta::new(),
    };
    let rust_out = emit_items(vec![item.clone()], &rust());
    assert_eq!(
        rust_out, "pub enum Color {\n    Red,\n    Green,\n    Blue,\n}",
        "got: {rust_out}"
    );
    let ts_out = emit_items(vec![item], &ts());
    assert_eq!(
        ts_out, "export enum Color {\n    Red,\n    Green,\n    Blue,\n}",
        "got: {ts_out}"
    );
}

// ---- typedef -----------------------------------------------------------

#[test]
fn typedef_renders() {
    let item = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(emit_items(vec![item.clone()], &rust()), "type Id = i32;");
    // i32 widens to number in TS.
    assert_eq!(emit_items(vec![item], &ts()), "type Id = number;");
}

// ---- const -------------------------------------------------------------

#[test]
fn const_renders() {
    let item = Item::Const {
        name: "MAX".to_string(),
        ty: i32t(),
        value: Expr::IntLiteral("100".to_string()),
        visibility: Visibility::Public,
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(
        emit_items(vec![item.clone()], &rust()),
        "pub const MAX: i32 = 100;"
    );
    assert_eq!(
        emit_items(vec![item], &ts()),
        "export const MAX: number = 100;"
    );
}

// ---- use ---------------------------------------------------------------

#[test]
fn use_renders() {
    let item = Item::Use {
        path: "std::io".to_string(),
        items: vec![],
        alias: None,
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(emit_items(vec![item.clone()], &rust()), "use std::io;");
    let ts_item = Item::Use {
        path: "\"fs\"".to_string(),
        items: vec![],
        alias: None,
        meta: lamina_core::ast::Meta::new(),
    };
    assert_eq!(emit_items(vec![ts_item], &ts()), "import \"fs\";");
}

// ---- interleaved file (order preserved) --------------------------------

#[test]
fn interleaved_struct_and_functions_render_in_order() {
    let func = |name: &str, ret: &str| {
        Item::Function(Function {
            name: name.to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: i32t(),
            body: vec![Statement::Return(Some(Expr::IntLiteral(ret.to_string())))],
            meta: lamina_core::ast::Meta::new(),
        })
    };
    let items = vec![
        func("first", "1"),
        Item::Struct {
            name: "Mid".to_string(),
            visibility: Visibility::Private,
            fields: vec![Field {
                name: "n".to_string(),
                ty: i32t(),
                visibility: Visibility::Private,
                meta: lamina_core::ast::Meta::new(),
            }],
            meta: lamina_core::ast::Meta::new(),
        },
        func("second", "2"),
    ];
    let out = emit_items(items, &rust());
    // Items are separated by a blank line and rendered in source order.
    assert_eq!(
        out,
        "fn first() -> i32 {\n    return 1;\n}\n\n\
         struct Mid {\n    n: i32,\n}\n\n\
         fn second() -> i32 {\n    return 2;\n}",
        "got: {out}"
    );
}

// ---- missing item section is a clean error -----------------------------

#[test]
fn item_without_section_errors_cleanly() {
    use lamina_core::lang_doc::parse_language_def;
    // A minimal def with ONLY `## Function` + `## Capabilities` (no `## Use`).
    // Emitting a `Use` item must fail with a clear UnknownItem error, not a
    // panic.
    let def = concat!(
        "# Lamina Language Definition: tiny\n\n",
        "## Function\n\n",
        "```template\nfn {name}() {{ {body} }}\n```\n\n",
        "### statement\n```template\nreturn;\n```\n\n",
        "## Capabilities\n\n",
        "| Primitive | Action | Target |\n",
        "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
        "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
        "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
        "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
        "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
        "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
        "| void | alias | () |\n| never | alias | ! |\n| byte | alias | u8 |\n",
        "| bytes | wrap | Vec |\n| char | identity | char |\n| str | wrap | String |\n",
        "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
    );
    let lang = parse_language_def(def).expect("tiny def parses");
    let file = File {
        items: vec![Item::Use {
            path: "x".to_string(),
            items: vec![],
            alias: None,
            meta: lamina_core::ast::Meta::new(),
        }],
    };
    let err = emit(&file, &lang).expect_err("no Use section");
    assert!(
        matches!(err, lamina_core::EmitError::UnknownItem { ref item, .. } if item == "use"),
        "got: {err:?}"
    );
}
