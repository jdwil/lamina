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
            TypeAttribute::Displayable,
            TypeAttribute::Cloneable,
            TypeAttribute::Equatable,
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
        attributes: vec![TypeAttribute::Displayable],
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
            TypeAttribute::Displayable,
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
            TypeAttribute::Cloneable,
            TypeAttribute::Copyable,
            TypeAttribute::HasDefault,
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
fn ts_struct_equatable_generates_field_wise_eq_fn() {
    // With the validator argument-aware (Blocker #C), a TS `equatable` struct
    // generates a field-wise `function Point_eq(...)` above the interface.
    // Field-wise `===` is the honest structural comparison. Hand-verified valid
    // idiomatic TypeScript.
    let item = point_struct(vec![TypeAttribute::Equatable], Visibility::Public);
    let out = emit_one(item, &ts()).expect("emit");
    // The generated helper is emitted directly above the interface, separated by
    // a single newline (a template trailing blank line is trimmed by the block
    // extractor, matching the C def's own defs/body convention). Valid TS.
    assert_eq!(
        out,
        "function Point_eq(a: Point, b: Point): boolean {\n    \
         return a.x === b.x && a.y === b.y;\n}\n\
         export interface Point {\n    x: number;\n    y: number;\n}",
        "got: {out}"
    );
}

#[test]
fn ts_struct_displayable_generates_to_string_fn() {
    // A TS `displayable` struct generates a field-wise
    // `function Point_toString(a: Point): string` using a template string
    // (Blocker #D), emitted above the interface. A primitive field interpolates
    // honestly via `${…}`. Hand-verified valid, idiomatic TypeScript.
    let item = point_struct(vec![TypeAttribute::Displayable], Visibility::Public);
    let out = emit_one(item, &ts()).expect("emit");
    assert_eq!(
        out,
        "function Point_toString(a: Point): string {\n    \
         return `Point { x = ${a.x}, y = ${a.y} }`;\n}\n\
         export interface Point {\n    x: number;\n    y: number;\n}",
        "got: {out}"
    );
}

#[test]
fn ts_struct_displayable_named_field_is_forbidden() {
    // A `named` field would interpolate to the useless `[object Object]`, and a
    // cross-type `_toString` availability check is out of scope for #D, so a
    // named field forbids the printer (mirroring C's escalated decision).
    let item = Item::Struct {
        name: "Wrap".to_string(),
        visibility: Visibility::Public,
        fields: vec![Field {
            name: "inner".to_string(),
            ty: lamina_core::ast::Type::Named("Point".to_string()),
            visibility: Visibility::Public,
            meta: lamina_core::ast::Meta::new(),
        }],
        attributes: vec![TypeAttribute::Displayable],
        meta: lamina_core::ast::Meta::new(),
    };
    let err = emit_one(item, &ts()).expect_err("named field forbidden in TS toString");
    assert!(
        matches!(err, EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}

#[test]
fn ts_struct_non_equatable_attributes_are_forbidden() {
    // `equatable` and `displayable` have clean field-wise TS forms; every other
    // attribute is a clean ForbiddenConstruct (see typescript.mdl `## Struct`
    // prose).
    for attr in [
        TypeAttribute::Comparable,
        TypeAttribute::Hashable,
        TypeAttribute::Cloneable,
        TypeAttribute::Copyable,
        TypeAttribute::HasDefault,
        TypeAttribute::Iterable,
    ] {
        let item = point_struct(vec![attr], Visibility::Public);
        let err = emit_one(item, &ts()).expect_err("non-equatable attr forbidden on TS");
        assert!(
            matches!(err, EmitError::ForbiddenConstruct { .. }),
            "{attr:?} got: {err:?}"
        );
    }
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
        attributes: vec![TypeAttribute::Displayable],
        meta: lamina_core::ast::Meta::new(),
    };
    let err = emit_one(item, &ts()).expect_err("enum attributes forbidden on TS");
    assert!(
        matches!(err, EmitError::ForbiddenConstruct { .. }),
        "got: {err:?}"
    );
}
