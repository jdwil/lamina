//! Integration tests for the **TOML** language definition.
//!
//! These verify that the same generic tree shape that renders to HTML, JSON,
//! and CSS also renders to a **TOML** document — that
//! `Expr::Node { name, attrs, children }` / `Attr` expresses TOML's
//! `key = value` + `[table]` header shape (Lamina dogfooding its own config
//! format).
//!
//! The mapping under test:
//! - a TOML **table** is a `node` whose `name` is the (opaque, possibly dotted)
//!   header path and whose attributes are its `key = value` pairs;
//! - the **document root** is a `node` tagged `root` in metadata, which renders
//!   its bare top-level keys with no `[header]` line, then its child tables;
//! - a **key/value pair** is an `attr` (`name` = key, `value` = value), rendered
//!   as its own `key = value` line;
//! - an **array value** is an `Expr::ArrayLit`, rendered inline as `[a, b, c]`.
//!
//! Every assertion below is hand-verified valid TOML (and mechanically parsed by
//! the `compile_check` harness).

use std::path::PathBuf;

use lamina_core::ast::{Attr, Expr, File, Item, Meta};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout
/// (identical resolution to `tree_core.rs` / `css.rs`).
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

fn toml() -> LanguageDef {
    shipped_def("toml.mdl")
}

/// A key/value pair whose value is a double-quoted TOML basic string.
fn str_kv(key: &str, value: &str) -> Attr {
    Attr {
        name: key.to_string(),
        value: Expr::StringLiteral(value.to_string()),
        meta: Meta::new(),
    }
}

/// A key/value pair whose value is a bare integer.
fn int_kv(key: &str, value: &str) -> Attr {
    Attr {
        name: key.to_string(),
        value: Expr::IntLiteral(value.to_string()),
        meta: Meta::new(),
    }
}

/// A key/value pair whose value is a bare boolean.
fn bool_kv(key: &str, value: bool) -> Attr {
    Attr {
        name: key.to_string(),
        value: Expr::BoolLiteral(value),
        meta: Meta::new(),
    }
}

/// A named TOML table node (`[name]` header + its pairs + child tables).
fn table(name: &str, pairs: Vec<Attr>, children: Vec<Expr>) -> Expr {
    Expr::Node {
        name: name.to_string(),
        attrs: pairs,
        children,
        meta: Meta::new(),
    }
}

/// The document-root node (tagged `root`, so it renders no `[header]` line).
fn document(pairs: Vec<Attr>, children: Vec<Expr>) -> Expr {
    Expr::Node {
        name: String::new(),
        attrs: pairs,
        children,
        meta: Meta::new().with("root", "true"),
    }
}

/// Wraps a root tree value as a top-level `Item::Tree`.
fn doc_file(root: Expr) -> File {
    File {
        items: vec![Item::Tree(root)],
    }
}

#[test]
fn declarative_only_def_loads_without_function_section() {
    // Like html.mdl / json.mdl / css.mdl, toml.mdl omits `## Function` entirely
    // — the loader must accept a declarative-only definition hosting slots under
    // `## Tree`.
    let _ = toml();
}

#[test]
fn single_table_key_value_pairs() {
    // A standalone `[table]` with three key/value pairs of different scalar
    // kinds: a string (quoted), an integer (bare), and a boolean (bare).
    let file = doc_file(table(
        "server",
        vec![
            str_kv("host", "localhost"),
            int_kv("port", "8080"),
            bool_kv("enabled", true),
        ],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(
        out,
        "[server]\nhost = \"localhost\"\nport = 8080\nenabled = true"
    );
}

#[test]
fn document_root_bare_keys_then_table() {
    // A document root with a bare top-level key (no `[header]`), followed by a
    // `[table]` section separated by a blank line — the canonical TOML file
    // layout.
    let file = doc_file(document(
        vec![str_kv("title", "Lamina")],
        vec![table(
            "package",
            vec![str_kv("name", "lamina-core"), str_kv("version", "0.1.0")],
            vec![],
        )],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(
        out,
        "title = \"Lamina\"\n\n[package]\nname = \"lamina-core\"\nversion = \"0.1.0\""
    );
}

#[test]
fn nested_table_dotted_header() {
    // A nested table: a `[server]` table containing a `[server.db]` sub-table.
    // The dotted header path is an opaque node name (the layer's concern), so
    // nesting needs no special engine support — the same generalization CSS
    // relies on for compound selectors.
    let file = doc_file(document(
        vec![],
        vec![table(
            "server",
            vec![str_kv("host", "localhost")],
            vec![table(
                "server.db",
                vec![str_kv("engine", "sqlite"), int_kv("pool", "8")],
                vec![],
            )],
        )],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(
        out,
        "\n\n[server]\nhost = \"localhost\"\n\n[server.db]\nengine = \"sqlite\"\npool = 8"
    );
}

#[test]
fn array_value_inline() {
    // An array value renders inline as `[a, b, c]` with `, ` separators from the
    // `### array_elem` loop facts.
    let file = doc_file(table(
        "network",
        vec![Attr {
            name: "ports".to_string(),
            value: Expr::ArrayLit {
                elems: vec![
                    Expr::IntLiteral("80".to_string()),
                    Expr::IntLiteral("443".to_string()),
                    Expr::IntLiteral("8080".to_string()),
                ],
                meta: Meta::new(),
            },
            meta: Meta::new(),
        }],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(out, "[network]\nports = [80, 443, 8080]");
}

#[test]
fn array_of_strings() {
    // A string array: each element is a quoted TOML basic string.
    let file = doc_file(table(
        "owner",
        vec![Attr {
            name: "aliases".to_string(),
            value: Expr::ArrayLit {
                elems: vec![
                    Expr::StringLiteral("jd".to_string()),
                    Expr::StringLiteral("jane".to_string()),
                ],
                meta: Meta::new(),
            },
            meta: Meta::new(),
        }],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(out, "[owner]\naliases = [\"jd\", \"jane\"]");
}

#[test]
fn empty_array_value() {
    // An empty array renders `[]` (a valid empty TOML array).
    let file = doc_file(table(
        "empty",
        vec![Attr {
            name: "items".to_string(),
            value: Expr::ArrayLit {
                elems: vec![],
                meta: Meta::new(),
            },
            meta: Meta::new(),
        }],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(out, "[empty]\nitems = []");
}

#[test]
fn float_value_bare() {
    // A float renders its bare textual form (TOML shares the literal spelling).
    let file = doc_file(table(
        "metrics",
        vec![Attr {
            name: "ratio".to_string(),
            value: Expr::FloatLiteral("0.75".to_string()),
            meta: Meta::new(),
        }],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(out, "[metrics]\nratio = 0.75");
}

#[test]
fn reference_value_is_interpolation_hole() {
    // A value that is a reference renders its bare name — proving values
    // dispatch through `### expr` (an interpolation hole for a templating
    // layer). Note: a bare identifier is only valid TOML when substituted, so
    // this is asserted as a string only, not fed to the parser harness.
    let file = doc_file(table(
        "build",
        vec![Attr {
            name: "target".to_string(),
            value: Expr::Ref("HOST_TRIPLE".to_string()),
            meta: Meta::new(),
        }],
        vec![],
    ));
    let out = emit(&file, &toml()).expect("emit toml");
    assert_eq!(out, "[build]\ntarget = HOST_TRIPLE");
}
