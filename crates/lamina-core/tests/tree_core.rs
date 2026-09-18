//! Integration tests for the **declarative tree core** (`node` / `attr` /
//! `text`).
//!
//! These prove the substrate is format-neutral: ONE generic tree AST
//! (`div > p > "hi"`) lowers to two structurally-different targets — HTML
//! (angle-bracket markup with attributes + children) and JSON (brace/colon
//! key-value data) — from the SAME node structure, via the real declarative
//! `html.mdl` / `json.mdl` documents in the sibling `lamina-defs` checkout.
//!
//! They also prove the two cores interoperate: a child that is an arbitrary
//! expression (interpolation), and an imperative `fn` that returns a tree node.

use std::path::PathBuf;

use lamina_core::ast::{Attr, Expr, File, Function, Item, Meta, Type, Visibility};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

/// Loads a shipped language definition from the sibling `lamina-defs` checkout.
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

fn html() -> LanguageDef {
    shipped_def("html.mdl")
}

fn json() -> LanguageDef {
    shipped_def("json.mdl")
}

/// The canonical generic tree used across the format-neutrality tests:
/// `div class="box"` containing a `p` whose only child is the text `hi`.
///
/// This SAME AST is emitted to both HTML and JSON below.
fn sample_tree() -> Expr {
    Expr::Node {
        name: "div".to_string(),
        attrs: vec![Attr {
            name: "class".to_string(),
            value: Expr::StringLiteral("box".to_string()),
            meta: Meta::new(),
        }],
        children: vec![Expr::Node {
            name: "p".to_string(),
            attrs: vec![],
            children: vec![Expr::Text(Box::new(Expr::StringLiteral("hi".to_string())))],
            meta: Meta::new(),
        }],
        meta: Meta::new(),
    }
}

/// A file whose sole item is a top-level tree value (a pure markup/config file).
fn tree_file(root: Expr) -> File {
    File {
        items: vec![Item::Tree(root)],
    }
}

#[test]
fn declarative_only_defs_load_without_function_section() {
    // Both defs omit `## Function` entirely — the loader must accept a
    // declarative-only definition that hosts its slots under `## Tree`.
    let _ = html();
    let _ = json();
}

#[test]
fn same_tree_renders_to_html() {
    let out = emit(&tree_file(sample_tree()), &html()).expect("emit html");
    assert_eq!(out, "<div class=\"box\"><p>hi</p></div>");
}

#[test]
fn same_tree_renders_to_json() {
    let out = emit(&tree_file(sample_tree()), &json()).expect("emit json");
    assert_eq!(
        out,
        "{\"tag\": \"div\", \"attributes\": {\"class\": \"box\"}, \"children\": \
         [{\"tag\": \"p\", \"attributes\": {}, \"children\": [\"hi\"]}]}"
    );
}

#[test]
fn same_tree_ast_two_targets_differ() {
    // The exact same AST value produces two structurally-different outputs.
    let tree = sample_tree();
    let as_html = emit(&tree_file(tree.clone()), &html()).expect("html");
    let as_json = emit(&tree_file(tree), &json()).expect("json");
    assert_ne!(as_html, as_json);
    assert!(as_html.starts_with('<'));
    assert!(as_json.starts_with('{'));
}

#[test]
fn interpolated_child_expression_renders() {
    // A child that is an arbitrary expression (a `{name}` reference) proves the
    // imperative and tree cores interoperate through expressions.
    let tree = Expr::Node {
        name: "span".to_string(),
        attrs: vec![],
        children: vec![Expr::Ref("name".to_string())],
        meta: Meta::new(),
    };
    let out = emit(&tree_file(tree), &html()).expect("emit html");
    assert_eq!(out, "<span>name</span>");
}

#[test]
fn interpolated_attribute_value_renders() {
    // An attribute value that is an arbitrary expression (an interpolation).
    let tree = Expr::Node {
        name: "a".to_string(),
        attrs: vec![Attr {
            name: "href".to_string(),
            value: Expr::Ref("url".to_string()),
            meta: Meta::new(),
        }],
        children: vec![],
        meta: Meta::new(),
    };
    let out = emit(&tree_file(tree), &html()).expect("emit html");
    assert_eq!(out, "<a href=\"url\"></a>");
}

#[test]
fn function_returning_a_node_renders_the_tree() {
    // Imperative/tree interop: a `fn` whose body returns a tree node. We reuse
    // the declarative `html` def, which now needs a minimal `## Function`
    // section to host the callable; the returned node still renders through the
    // shared `### expr` dispatch. Build a self-contained def inline.
    let def = lamina_core::lang_doc::parse_language_def(FN_RETURNS_NODE_DEF)
        .expect("fn+tree def parses");
    let func = Function {
        name: "view".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Node".to_string()),
        body: vec![lamina_core::ast::Statement::Return(Some(Expr::Node {
            name: "b".to_string(),
            attrs: vec![],
            children: vec![Expr::Text(Box::new(Expr::StringLiteral("x".to_string())))],
            meta: Meta::new(),
        }))],
        meta: Meta::new(),
    };
    let file = File {
        items: vec![Item::Function(func)],
    };
    let out = emit(&file, &def).expect("emit fn returning node");
    assert_eq!(out, "fn view() -> Node {\n    return <b>x</b>;\n}");
}

/// A hybrid def: an imperative `## Function` section (so a `fn` can be rendered)
/// whose shared `### expr` dispatch ALSO knows the tree kinds — a node's TYPE is
/// expressed as a `Named` type (`Node`), reusing the existing type machinery.
const FN_RETURNS_NODE_DEF: &str = concat!(
    "# Lamina Language Definition: htmlfn\n\n",
    "```lang-meta\nlamina-format: 0.0.0\ntarget: htmlfn\ntarget-version: test\n```\n\n",
    "## Function\n\n",
    "```template\n",
    "fn {name}({params}){ret} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n\n",
    "### ret\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| ret is void | \"\" |\n",
    "| else        | \" -> {ret_type}\" |\n\n",
    "### param\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n\n",
    "### statement\n",
    "```template\n",
    "return {value};\n",
    "```\n\n",
    "### expr\n",
    "| When         | Template |\n",
    "|--------------|----------|\n",
    "| expr is node | @element |\n",
    "| expr is text | \"{value}\" |\n",
    "| expr is string | \"{value}\" |\n",
    "| expr is ref  | \"{value}\" |\n",
    "| else         | forbid |\n\n",
    "### element\n",
    "```template\n",
    "<{node_name}{attrs}>{children}</{node_name}>\n",
    "```\n\n",
    "### attr\n",
    "| When | Template |\n",
    "|------|----------|\n",
    "| else | \" {name}=\\\"{value}\\\"\" |\n\n",
    "### child\n",
    "| When | Template |\n",
    "|------|----------|\n",
    "| else | \"{value}\" |\n\n",
    "## Capabilities\n\n",
    "| Primitive | Action | Target |\n",
    "|-----------|--------|--------|\n",
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
