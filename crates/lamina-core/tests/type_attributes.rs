//! End-to-end tests for Lamina **type attributes** against the *shipped*
//! language definitions.
//!
//! Type attributes are the type-level analog of callable modifiers: the kernel
//! reserves the superset (`debug`/`eq`/`ord`/`hash`/`clone`/`copy`/`default`/
//! `iterable`) and each language definition realizes each one per target. Rust
//! folds requested attributes into a single `#[derive(...)]` line before the
//! declaration; TypeScript has no derive mechanism, so a type carrying any
//! attribute is a clean `ForbiddenConstruct`.
//!
//! There is no concrete source syntax for attributes yet (the parser lags), so
//! these build [`Item`] ASTs directly and transpile them with the real
//! `rust.mdl` / `typescript.mdl` documents from `lamina-defs`.

use std::path::PathBuf;

use lamina_core::ast::{
    Field, File, Item, Primitive, Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::error::EmitError;
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

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

fn point_field(name: &str) -> Field {
    Field {
        name: name.to_string(),
        ty: i32t(),
        visibility: Visibility::Public,
        meta: lamina_core::ast::Meta::new(),
    }
}

/// A `Point { x, y }` struct with the given attributes and visibility.
fn point_struct(attributes: Vec<TypeAttribute>, visibility: Visibility) -> Item {
    Item::Struct {
        name: "Point".to_string(),
        visibility,
        fields: vec![point_field("x"), point_field("y")],
        attributes,
        meta: lamina_core::ast::Meta::new(),
    }
}

fn emit_one(item: Item, lang: &LanguageDef) -> Result<String, EmitError> {
    emit(&File { items: vec![item] }, lang)
}

// ---- shipped defs still load with the attribute slots ------------------

#[test]
fn shipped_defs_load_with_attribute_slots() {
    let _ = rust();
    let _ = ts();
}

// ---- (a) struct with [Debug, Clone, Eq] -> combined derive line --------

#[test]
fn rust_struct_with_debug_clone_eq_emits_exact_derive_line() {
    let item = point_struct(
        vec![
            TypeAttribute::Debug,
            TypeAttribute::Clone,
            TypeAttribute::Eq,
        ],
        Visibility::Public,
    );
    let out = emit_one(item, &rust()).expect("emit");
    // The derive line precedes the struct, byte-for-byte, with the kernel `eq`
    // mapped to Rust `PartialEq`.
    assert_eq!(
        out,
        "#[derive(Debug, Clone, PartialEq)]\npub struct Point {\n    pub x: i32,\n    pub y: i32,\n}",
        "got: {out}"
    );
    // Explicitly assert the exact derive prefix (the headline proof).
    assert!(out.starts_with("#[derive(Debug, Clone, PartialEq)]\n"), "got: {out}");
}

// ---- (b) enum with [Debug] -> `#[derive(Debug)]` -----------------------

#[test]
fn rust_enum_with_debug_emits_derive_debug() {
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: VariantPayload::None,
                meta: lamina_core::ast::Meta::new(),
            },
        ],
        attributes: vec![TypeAttribute::Debug],
        meta: lamina_core::ast::Meta::new(),
    };
    let out = emit_one(item, &rust()).expect("emit");
    assert!(out.starts_with("#[derive(Debug)]\n"), "got: {out}");
    assert_eq!(
        out,
        "#[derive(Debug)]\npub enum Color {\n    Red,\n    Green,\n}",
        "got: {out}"
    );
}

// ---- (c) empty attributes -> byte-identical to the old form ------------

#[test]
fn empty_attributes_are_byte_identical_to_no_derive() {
    // A struct built with NO attributes must render exactly as it did before
    // attributes existed — no derive line, no stray whitespace.
    let with_empty = point_struct(Vec::new(), Visibility::Public);
    let out = emit_one(with_empty, &rust()).expect("emit");
    assert_eq!(
        out,
        "pub struct Point {\n    pub x: i32,\n    pub y: i32,\n}",
        "got: {out}"
    );
    // And it must NOT contain a derive line.
    assert!(!out.contains("#[derive"), "got: {out}");
}

#[test]
fn empty_attributes_enum_is_byte_identical() {
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Red".to_string(),
            payload: VariantPayload::None,
            meta: lamina_core::ast::Meta::new(),
        }],
        attributes: Vec::new(),
        meta: lamina_core::ast::Meta::new(),
    };
    let out = emit_one(item, &rust()).expect("emit");
    assert_eq!(out, "pub enum Color {\n    Red,\n}", "got: {out}");
    assert!(!out.contains("#[derive"), "got: {out}");
}

#[test]
fn rust_all_attribute_spellings_render() {
    // Exercise the full mapping (except iterable, which is forbid): the derive
    // list is comma-joined in canonical order.
    let item = point_struct(
        vec![
            TypeAttribute::Debug,
            TypeAttribute::Eq,
            TypeAttribute::Ord,
            TypeAttribute::Hash,
            TypeAttribute::Clone,
            TypeAttribute::Copy,
            TypeAttribute::Default,
        ],
        Visibility::Public,
    );
    let out = emit_one(item, &rust()).expect("emit");
    assert!(
        out.starts_with(
            "#[derive(Debug, PartialEq, PartialOrd, Hash, Clone, Copy, Default)]\n"
        ),
        "got: {out}"
    );
}

#[test]
fn rust_iterable_attribute_is_forbidden() {
    // Rust has no single idiomatic derive for iteration, so `iterable` is
    // forbid-by-omission-of-idiom: it surfaces a ForbiddenConstruct.
    let item = point_struct(vec![TypeAttribute::Iterable], Visibility::Public);
    let err = emit_one(item, &rust()).expect_err("iterable forbidden on Rust");
    assert!(
        matches!(err, EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}

// ---- (d) TypeScript: attributes are forbidden; empty is byte-identical -

#[test]
fn ts_struct_with_attributes_is_forbidden() {
    // A TS interface cannot realize a type-level attribute, so a struct that
    // requests any attribute is a clean ForbiddenConstruct.
    let item = point_struct(vec![TypeAttribute::Debug], Visibility::Public);
    let err = emit_one(item, &ts()).expect_err("attributes forbidden on TS");
    assert!(
        matches!(err, EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}

#[test]
fn ts_empty_attribute_struct_is_byte_identical() {
    // An attribute-free TS struct renders exactly as before attributes existed.
    let item = point_struct(Vec::new(), Visibility::Public);
    let out = emit_one(item, &ts()).expect("emit");
    assert_eq!(
        out,
        "export interface Point {\n    x: number;\n    y: number;\n}",
        "got: {out}"
    );
}

#[test]
fn ts_enum_with_attributes_is_forbidden() {
    let item = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Red".to_string(),
            payload: VariantPayload::None,
            meta: lamina_core::ast::Meta::new(),
        }],
        attributes: vec![TypeAttribute::Debug],
        meta: lamina_core::ast::Meta::new(),
    };
    let err = emit_one(item, &ts()).expect_err("enum attributes forbidden on TS");
    assert!(
        matches!(err, EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}
