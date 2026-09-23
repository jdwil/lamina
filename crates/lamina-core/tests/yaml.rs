//! Integration tests for the **YAML** language definition.
//!
//! These verify that the same generic tree core (`node` / `attr` / `text`) that
//! renders to HTML, JSON, and CSS also renders to a **YAML** document — that
//! `Expr::Node { name, attrs, children }` / `Attr` expresses YAML's
//! indentation-based block mapping (`key: value`), block sequence (`- item`),
//! and scalar shapes.
//!
//! The mapping under test:
//! - a YAML **block mapping** is a `node` (default role); each declaration is an
//!   `Attr` rendered as its own `key: value` line;
//! - a YAML **block sequence** is a `node` carrying `yaml=seq`; each child is a
//!   `- item` line;
//! - **nesting** is expressed by a `block`-tagged entry, whose value is placed
//!   on the following lines indented two spaces by the renderer's
//!   continuation-line rule;
//! - a **scalar** (string / number / bool / null / reference) renders as a bare
//!   YAML token.
//!
//! Every assertion below is hand-verified valid YAML and is additionally parsed
//! by a real YAML loader in `compile_check.rs`.

use std::path::PathBuf;

use lamina_core::ast::{Attr, Expr, File, Item, Meta};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout
/// (identical resolution to `css.rs` / `tree_core.rs`).
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

fn yaml() -> LanguageDef {
    shipped_def("yaml.mdl")
}

/// A scalar mapping entry (`key: value`) whose value is a bare string token.
fn s(key: &str, value: &str) -> Attr {
    Attr {
        name: key.to_string(),
        value: Expr::StringLiteral(value.to_string()),
        meta: Meta::new(),
    }
}

/// An integer mapping entry (`key: 42`).
fn i(key: &str, value: &str) -> Attr {
    Attr {
        name: key.to_string(),
        value: Expr::IntLiteral(value.to_string()),
        meta: Meta::new(),
    }
}

/// A `block`-tagged mapping entry whose value is a nested collection node,
/// placed on the following indented lines.
fn block(key: &str, nested: Expr) -> Attr {
    Attr {
        name: key.to_string(),
        value: nested,
        meta: Meta::new().with("block", "true"),
    }
}

/// A block mapping node (default YAML role).
fn map(attrs: Vec<Attr>) -> Expr {
    Expr::Node {
        name: String::new(),
        attrs,
        children: vec![],
        meta: Meta::new(),
    }
}

/// A block sequence node (`yaml=seq`), children rendered as `- item`.
fn seq(children: Vec<Expr>) -> Expr {
    Expr::Node {
        name: String::new(),
        attrs: vec![],
        children,
        meta: Meta::new().with("yaml", "seq"),
    }
}

/// Wraps a root value as a one-item YAML document.
fn doc(root: Expr) -> File {
    File {
        items: vec![Item::Tree(root)],
    }
}

#[test]
fn declarative_only_def_loads_without_function_section() {
    // Like html.mdl / json.mdl / css.mdl, yaml.mdl omits `## Function` entirely —
    // the loader must accept a declarative-only definition hosting slots under
    // `## Tree`.
    let _ = yaml();
}

#[test]
fn flat_mapping_scalar_values() {
    // A flat block mapping: three `key: value` pairs, each on its own line.
    let file = doc(map(vec![
        s("name", "lamina"),
        i("port", "8080"),
        s("env", "prod"),
    ]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "name: lamina\nport: 8080\nenv: prod");
}

#[test]
fn single_entry_mapping() {
    let file = doc(map(vec![s("key", "value")]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "key: value");
}

#[test]
fn scalar_kinds_render_as_bare_tokens() {
    // string, int, float, bool, null, and a reference all render unquoted.
    let file = doc(map(vec![
        s("s", "text"),
        i("n", "42"),
        Attr {
            name: "f".to_string(),
            value: Expr::FloatLiteral("3.14".to_string()),
            meta: Meta::new(),
        },
        Attr {
            name: "b".to_string(),
            value: Expr::BoolLiteral(true),
            meta: Meta::new(),
        },
        Attr {
            name: "nothing".to_string(),
            value: Expr::NullLiteral,
            meta: Meta::new(),
        },
        Attr {
            name: "ref".to_string(),
            value: Expr::Ref("anchorName".to_string()),
            meta: Meta::new(),
        },
    ]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(
        out,
        "s: text\nn: 42\nf: 3.14\nb: true\nnothing: null\nref: anchorName"
    );
}

#[test]
fn flat_sequence() {
    // A block sequence of three scalar items, each on a `- ` line.
    let file = doc(seq(vec![
        Expr::StringLiteral("alpha".to_string()),
        Expr::StringLiteral("beta".to_string()),
        Expr::StringLiteral("gamma".to_string()),
    ]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "- alpha\n- beta\n- gamma");
}

#[test]
fn nested_mapping_indented_two_spaces() {
    // A mapping whose `server` entry is a nested mapping: the nested keys sit on
    // the lines after `server:`, indented two spaces.
    let file = doc(map(vec![
        s("name", "app"),
        block("server", map(vec![s("host", "localhost"), i("port", "5432")])),
    ]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(
        out,
        "name: app\nserver:\n  host: localhost\n  port: 5432"
    );
}

#[test]
fn deeply_nested_mapping() {
    // Two levels of nesting: each level indents its children two more spaces.
    let file = doc(map(vec![block(
        "a",
        map(vec![block("b", map(vec![s("c", "deep")]))]),
    )]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "a:\n  b:\n    c: deep");
}

#[test]
fn mapping_with_nested_sequence() {
    // A mapping entry whose value is a block sequence.
    let file = doc(map(vec![
        s("service", "web"),
        block(
            "ports",
            seq(vec![
                Expr::IntLiteral("80".to_string()),
                Expr::IntLiteral("443".to_string()),
            ]),
        ),
    ]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "service: web\nports:\n  - 80\n  - 443");
}

#[test]
fn empty_mapping_renders_empty() {
    let file = doc(map(vec![]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "");
}

#[test]
fn text_node_renders_verbatim_scalar() {
    // A top-level `text` node renders its inner value verbatim through `### expr`.
    let file = doc(Expr::Text(Box::new(Expr::StringLiteral(
        "just a scalar".to_string(),
    ))));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "just a scalar");
}

#[test]
fn quoted_scalar_carries_its_own_quotes() {
    // A value needing YAML quoting (e.g. a string that looks like a number, or
    // contains special characters) carries its own quotes in the stored string.
    let file = doc(map(vec![s("version", "\"1.0\"")]));
    let out = emit(&file, &yaml()).expect("emit yaml");
    assert_eq!(out, "version: \"1.0\"");
}
