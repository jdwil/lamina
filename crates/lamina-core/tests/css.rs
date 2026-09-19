//! Integration tests for the **CSS** language definition — the
//! *tree-core-vs-styles* validator.
//!
//! Where `tree_core.rs` proves the generic `node` / `attr` / `text` substrate
//! lowers to angle-bracket markup (HTML) and brace/colon data (JSON), these
//! tests prove it also lowers to a **stylesheet**: the SAME generic tree AST
//! shape (`Expr::Node { name, attrs, children }` / `Attr`) expresses CSS's
//! `selector { property: value; }` rule shape.
//!
//! The mapping under test:
//! - a CSS **rule** is a `node` whose `name` is the arbitrary selector string;
//! - a **declaration** is an `attr` (`name` = property, `value` = value), which
//!   CSS renders as its own `property: value;` line (contrast HTML's
//!   ` name="value"` and JSON's `"name": value`);
//! - a **stylesheet** is a sequence of top-level `Item::Tree` values, joined by
//!   the engine's blank-line item separator.
//!
//! Every assertion below is hand-verified valid CSS.

use std::path::PathBuf;

use lamina_core::ast::{Attr, Expr, File, Item, Meta};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout
/// (identical resolution to `tree_core.rs`).
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

fn css() -> LanguageDef {
    shipped_def("css.mdl")
}

/// Builds one CSS declaration (`property: value;`) as an `Attr` whose value is a
/// bare (unquoted) CSS token string.
fn decl(property: &str, value: &str) -> Attr {
    Attr {
        name: property.to_string(),
        value: Expr::StringLiteral(value.to_string()),
        meta: Meta::new(),
    }
}

/// Builds one CSS rule (`selector { ... }`) as a tree `node`.
fn rule(selector: &str, decls: Vec<Attr>) -> Expr {
    Expr::Node {
        name: selector.to_string(),
        attrs: decls,
        children: vec![],
        meta: Meta::new(),
    }
}

/// Wraps rules as a stylesheet: one top-level `Item::Tree` per rule.
fn stylesheet(rules: Vec<Expr>) -> File {
    File {
        items: rules.into_iter().map(Item::Tree).collect(),
    }
}

#[test]
fn declarative_only_def_loads_without_function_section() {
    // Like html.mdl / json.mdl, css.mdl omits `## Function` entirely — the
    // loader must accept a declarative-only definition hosting slots under
    // `## Tree`.
    let _ = css();
}

#[test]
fn single_rule_multiple_declarations() {
    // A class selector with three declarations, indented four spaces, each on
    // its own line, terminated with a semicolon.
    let file = stylesheet(vec![rule(
        ".box",
        vec![
            decl("color", "red"),
            decl("background", "blue"),
            decl("margin", "0"),
        ],
    )]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(
        out,
        ".box {\n    color: red;\n    background: blue;\n    margin: 0;\n}"
    );
}

#[test]
fn single_declaration_rule() {
    let file = stylesheet(vec![rule("p", vec![decl("color", "green")])]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, "p {\n    color: green;\n}");
}

#[test]
fn element_selector() {
    // A bare element selector (`h1`).
    let file = stylesheet(vec![rule(
        "h1",
        vec![decl("font-size", "2em"), decl("font-weight", "bold")],
    )]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, "h1 {\n    font-size: 2em;\n    font-weight: bold;\n}");
}

#[test]
fn class_selector() {
    let file = stylesheet(vec![rule(".btn", vec![decl("padding", "8px 12px")])]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, ".btn {\n    padding: 8px 12px;\n}");
}

#[test]
fn id_selector() {
    let file = stylesheet(vec![rule("#main", vec![decl("display", "flex")])]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, "#main {\n    display: flex;\n}");
}

#[test]
fn multiple_rules_blank_line_separated() {
    // A stylesheet of three rules of different selector kinds — element, class,
    // id — separated by a blank line (the engine's top-level item separator).
    let file = stylesheet(vec![
        rule("body", vec![decl("margin", "0")]),
        rule(".container", vec![decl("width", "100%")]),
        rule("#header", vec![decl("height", "60px")]),
    ]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(
        out,
        "body {\n    margin: 0;\n}\n\n\
         .container {\n    width: 100%;\n}\n\n\
         #header {\n    height: 60px;\n}"
    );
}

#[test]
fn compound_and_descendant_selectors_are_opaque_node_names() {
    // The node name is an arbitrary, opaque selector string: compound
    // (`a.btn:hover`) and descendant (`nav ul li`) selectors need no special
    // engine support — proving the tree-core node name generalizes to any
    // selector.
    let file = stylesheet(vec![
        rule("a.btn:hover", vec![decl("color", "white")]),
        rule("nav ul li", vec![decl("list-style", "none")]),
    ]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(
        out,
        "a.btn:hover {\n    color: white;\n}\n\n\
         nav ul li {\n    list-style: none;\n}"
    );
}

#[test]
fn value_with_hash_color_and_units() {
    // CSS values are rendered verbatim (bare tokens, not quoted) — a hex color
    // and a shorthand with multiple tokens.
    let file = stylesheet(vec![rule(
        ".card",
        vec![
            decl("color", "#ffffff"),
            decl("border", "1px solid #000"),
        ],
    )]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(
        out,
        ".card {\n    color: #ffffff;\n    border: 1px solid #000;\n}"
    );
}

#[test]
fn reference_value_is_interpolation_hole() {
    // A declaration value that is a reference (a variable / custom property)
    // renders its bare name — proving values dispatch through `### expr`.
    let file = stylesheet(vec![rule(
        ":root",
        vec![Attr {
            name: "--brand".to_string(),
            value: Expr::Ref("brandColor".to_string()),
            meta: Meta::new(),
        }],
    )]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, ":root {\n    --brand: brandColor;\n}");
}

#[test]
fn empty_rule_renders_empty_block() {
    // A rule with no declarations renders an empty (whitespace-only) block. This
    // is valid CSS (an empty rule is legal, if inert).
    let file = stylesheet(vec![rule(".empty", vec![])]);
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, ".empty {\n    \n}");
}

#[test]
fn text_node_child_renders_verbatim() {
    // A `text` node renders its inner value verbatim through `### expr` —
    // exercised as a standalone top-level tree value.
    let file = File {
        items: vec![Item::Tree(Expr::Text(Box::new(Expr::StringLiteral(
            "/* a comment */".to_string(),
        ))))],
    };
    let out = emit(&file, &css()).expect("emit css");
    assert_eq!(out, "/* a comment */");
}
