//! Integration tests for the **SQL** language definition — a scoped
//! **fit-assessment probe**, not a shipping general-purpose def.
//!
//! SQL is a *relational* language. Only its **DDL** (`CREATE TABLE`) maps
//! cleanly onto Lamina's declarative **tree core** (`node` / `attr` / `child`),
//! reusing the exact substrate that expressed HTML, JSON, CSS, and TOML. SQL's
//! *query* surface (`SELECT`/`FROM`/`WHERE`/`JOIN`/`GROUP BY`) and the rest of
//! its DML (`INSERT`/`UPDATE`/`DELETE`) are set-oriented relational algebra with
//! **no** kernel representation and are deliberately out of scope — see the
//! fit-assessment report.
//!
//! The mapping under test:
//! - a **table** is a `node` tagged `sql=table` whose `node_name` is the table
//!   name and whose children are column / table-constraint definitions;
//! - a **column** is a child `node` tagged `sql=column` whose `node_name` is the
//!   column name, whose first attribute is its SQL type token, and whose further
//!   attributes are verbatim constraint clauses;
//! - a **table constraint** is a child `node` tagged `sql=constraint` whose
//!   `node_name` is the whole verbatim clause.
//!
//! Every emitted string below is hand-verified valid SQLite DDL (and, when
//! `sqlite3` is present, mechanically parsed by the `compile_check` harness).

use std::path::PathBuf;

use lamina_core::ast::{Attr, Expr, File, Item, Meta};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout
/// (identical resolution to `tree_core.rs` / `toml.rs`).
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

fn sql() -> LanguageDef {
    shipped_def("sql.mdl")
}

/// A column definition node: `name TYPE constraint…`. The first attribute is the
/// SQL type token (attribute name `type`); each further `constraint` is a
/// verbatim clause stored in the attribute *name* with an empty value.
fn column(name: &str, ty: &str, constraints: &[&str]) -> Expr {
    let mut attrs = vec![Attr {
        name: "type".to_string(),
        value: Expr::StringLiteral(ty.to_string()),
        meta: Meta::new(),
    }];
    for c in constraints {
        attrs.push(Attr {
            name: (*c).to_string(),
            value: Expr::StringLiteral(String::new()),
            meta: Meta::new(),
        });
    }
    Expr::Node {
        name: name.to_string(),
        attrs,
        children: vec![],
        meta: Meta::new().with("sql", "column"),
    }
}

/// A verbatim table-level constraint clause (e.g. a FOREIGN KEY line).
fn table_constraint(clause: &str) -> Expr {
    Expr::Node {
        name: clause.to_string(),
        attrs: vec![],
        children: vec![],
        meta: Meta::new().with("sql", "constraint"),
    }
}

/// A `CREATE TABLE` node whose children are the column / constraint definitions.
fn table(name: &str, defs: Vec<Expr>) -> Expr {
    Expr::Node {
        name: name.to_string(),
        attrs: vec![],
        children: defs,
        meta: Meta::new().with("sql", "table"),
    }
}

/// Wraps a root tree value as a top-level `Item::Tree` (a declarative-only file).
fn ddl_file(root: Expr) -> File {
    File {
        items: vec![Item::Tree(root)],
    }
}

#[test]
fn declarative_only_def_loads_without_function_section() {
    // Like html.mdl / json.mdl / css.mdl / toml.mdl, sql.mdl omits `## Function`
    // entirely — the loader must accept a declarative-only definition hosting
    // its slots under `## Tree`.
    let _ = sql();
}

#[test]
fn single_column_table() {
    // The minimal table: one column with a type and no constraints.
    let file = ddl_file(table("logs", vec![column("message", "TEXT", &[])]));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(out, "CREATE TABLE logs (\n    message TEXT\n);");
}

#[test]
fn column_with_single_constraint() {
    // A column carrying one constraint clause after its type.
    let file = ddl_file(table(
        "users",
        vec![column("id", "INTEGER", &["PRIMARY KEY"])],
    ));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(out, "CREATE TABLE users (\n    id INTEGER PRIMARY KEY\n);");
}

#[test]
fn multi_column_table_with_constraints() {
    // The canonical `CREATE TABLE`: several columns of different types, each with
    // its own constraint set, comma-separated one per indented line.
    let file = ddl_file(table(
        "users",
        vec![
            column("id", "INTEGER", &["PRIMARY KEY"]),
            column("name", "TEXT", &["NOT NULL"]),
            column("email", "TEXT", &["NOT NULL", "UNIQUE"]),
            column("age", "INTEGER", &[]),
        ],
    ));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(
        out,
        "CREATE TABLE users (\n    \
         id INTEGER PRIMARY KEY,\n    \
         name TEXT NOT NULL,\n    \
         email TEXT NOT NULL UNIQUE,\n    \
         age INTEGER\n);"
    );
}

#[test]
fn column_with_default_clause() {
    // A `DEFAULT <value>` clause is just another verbatim constraint clause.
    let file = ddl_file(table(
        "settings",
        vec![
            column("key", "TEXT", &["PRIMARY KEY"]),
            column("enabled", "INTEGER", &["NOT NULL", "DEFAULT 0"]),
        ],
    ));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(
        out,
        "CREATE TABLE settings (\n    \
         key TEXT PRIMARY KEY,\n    \
         enabled INTEGER NOT NULL DEFAULT 0\n);"
    );
}

#[test]
fn table_level_foreign_key_constraint() {
    // A table-level constraint (a FOREIGN KEY clause) renders as its own line,
    // exactly like a column — proving the `sql=constraint` role works.
    let file = ddl_file(table(
        "orders",
        vec![
            column("id", "INTEGER", &["PRIMARY KEY"]),
            column("user_id", "INTEGER", &["NOT NULL"]),
            table_constraint("FOREIGN KEY (user_id) REFERENCES users(id)"),
        ],
    ));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(
        out,
        "CREATE TABLE orders (\n    \
         id INTEGER PRIMARY KEY,\n    \
         user_id INTEGER NOT NULL,\n    \
         FOREIGN KEY (user_id) REFERENCES users(id)\n);"
    );
}

#[test]
fn reference_type_is_interpolation_hole() {
    // A column type that is a reference renders its bare name — proving values
    // dispatch through `### expr` (an interpolation hole for a templating
    // layer). Asserted as a string only (a bare identifier is valid SQL only
    // once substituted), so this case is not fed to the sqlite3 parser harness.
    let file = ddl_file(table(
        "generated",
        vec![Expr::Node {
            name: "col".to_string(),
            attrs: vec![Attr {
                name: "type".to_string(),
                value: Expr::Ref("COLTYPE".to_string()),
                meta: Meta::new(),
            }],
            children: vec![],
            meta: Meta::new().with("sql", "column"),
        }],
    ));
    let out = emit(&file, &sql()).expect("emit sql");
    assert_eq!(out, "CREATE TABLE generated (\n    col COLTYPE\n);");
}
