//! Full-pipeline tests driven from raw Lamina **source text**.
//!
//! Where `tests/transpile.rs` proves the core equation on a one-line program
//! and `tests/parse_roundtrip.rs` proves the parser's AST equals the hand-built
//! oracle, THIS suite closes the loop: it drives representative `.mdl` SOURCE
//! STRINGS through the WHOLE pipeline — `parse` → `lower` → `emit`, via the
//! convenience `transpile` wrapper — against ONE self-contained synthetic
//! fixture definition, asserting the exact emitted output.
//!
//! This proves the engine can take raw source text (not a hand-built AST) and
//! transpile it end-to-end with no external definition. The fixture is modelled
//! on the one in `tests/transpile.rs` but widened to cover a cross-section of
//! the kernel: functions, a `struct`, a payload-bearing `enum`, the imperative
//! statements, operators (including the Dart-style floor-division `~/`), a
//! `lambda`, and a `raw` block (exercising the `lower` pass's target-aware
//! raw-arm resolution). The fixture stays deliberately simple — it is a test
//! oracle, not an idiomatic target — so the asserted outputs are easy to read.

use lamina_core::lang_doc::parse_language_def;
use lamina_core::transpile;

/// A single self-contained synthetic fixture definition, rich enough to emit a
/// cross-section of the kernel. Its `target` is `fixture`, so a `raw` block's
/// `fixture` arm is the one the `lower` pass selects.
///
/// The spellings are intentionally plain (a C/Rust-ish pidgin) so the asserted
/// strings read clearly; the point is that the OUTPUT is produced from parsed
/// SOURCE, not that it is idiomatic any particular language.
const FIXTURE_DEF: &str = concat!(
    "# Lamina Language Definition: fixture\n",
    "\n",
    "```lang-meta\nlamina-format: 0.0.0\ntarget: fixture\ntarget-version: 1\n```\n",
    "\n",
    "## Function\n",
    "\n",
    "```template\n",
    "{vis}fn {name}({params}){ret} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### vis\n",
    "| When          | Template |\n",
    "|---------------|----------|\n",
    "| vis is public | \"pub \" |\n",
    "| else          | \"\" |\n",
    "\n",
    "### ret\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| ret is void | \"\" |\n",
    "| else        | \" -> {ret_type}\" |\n",
    "\n",
    "### param\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n",
    "\n",
    "### statement\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | @stmt |\n",
    "| else  | \"\\n{stmt}\" |\n",
    "\n",
    "### stmt\n",
    "| When                        | Template |\n",
    "|-----------------------------|----------|\n",
    "| stmt is let && has_value    | \"let {name} = {value};\" |\n",
    "| stmt is let                 | \"let {name};\" |\n",
    "| stmt is return && has_value | \"return {value};\" |\n",
    "| stmt is return              | \"return;\" |\n",
    "| stmt is if                  | @if_stmt |\n",
    "| stmt is while               | @while_stmt |\n",
    "| stmt is assign              | \"{target} = {value};\" |\n",
    "| stmt is expr                | \"{value};\" |\n",
    "| stmt is raw                 | \"{value}\" |\n",
    "| else                        | forbid |\n",
    "\n",
    "### if_stmt\n",
    "```template\n",
    "if {cond} {{\n",
    "    {then}\n",
    "}}\n",
    "```\n",
    "\n",
    "### while_stmt\n",
    "```template\n",
    "while {cond} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### expr\n",
    "| When            | Template |\n",
    "|-----------------|----------|\n",
    "| expr is int     | \"{value}\" |\n",
    "| expr is float   | \"{value}\" |\n",
    "| expr is bool    | \"{value}\" |\n",
    "| expr is string  | \"\\\"{value}\\\"\" |\n",
    "| expr is ref     | \"{value}\" |\n",
    "| expr is field   | \"{obj}.{field}\" |\n",
    "| expr is call    | \"{callee}({args})\" |\n",
    "| expr is unary   | \"{op}{operand}\" |\n",
    "| expr is binary  | \"{lhs} {op} {rhs}\" |\n",
    "| expr is struct_lit | @struct_lit |\n",
    "| expr is lambda  | @lambda |\n",
    "| expr is raw     | \"{value}\" |\n",
    "| else            | forbid |\n",
    "\n",
    "### struct_lit\n",
    "```template\n",
    "{type_name} {{ {fields} }}\n",
    "```\n",
    "\n",
    "### field_init\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {value}\" |\n",
    "| else  | \", {name}: {value}\" |\n",
    "\n",
    "### expr_arg\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{value}\" |\n",
    "| else  | \", {value}\" |\n",
    "\n",
    "### lambda\n",
    "```template\n",
    "|{params}| {{ {body} }}\n",
    "```\n",
    "\n",
    "## Struct\n",
    "\n",
    "```template\n",
    "struct {name} {{\n",
    "    {fields}\n",
    "}}\n",
    "```\n",
    "\n",
    "### field\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type},\" |\n",
    "| else  | \"\\n{name}: {type},\" |\n",
    "\n",
    "## Enum\n",
    "\n",
    "```template\n",
    "enum {name} {{\n",
    "    {variants}\n",
    "}}\n",
    "```\n",
    "\n",
    "### variant\n",
    "| When                       | Template |\n",
    "|----------------------------|----------|\n",
    "| variant is tuple && first  | \"{name}({payload_types})\" |\n",
    "| variant is tuple           | \"\\n{name}({payload_types})\" |\n",
    "| variant is struct && first | \"{name} {{ {payload_fields} }}\" |\n",
    "| variant is struct          | \"\\n{name} {{ {payload_fields} }}\" |\n",
    "| first                      | \"{name}\" |\n",
    "| else                       | \"\\n{name}\" |\n",
    "\n",
    "### payload_type\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{type}\" |\n",
    "| else  | \", {type}\" |\n",
    "\n",
    "### payload_field\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n",
    "\n",
    "## Const\n",
    "\n",
    "```template\n",
    "const {name}: {type} = {value};\n",
    "```\n",
    "\n",
    "## Capabilities\n",
    "| Primitive | Action | Target |\n",
    "|-----------|--------|--------|\n",
    "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
    "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
    "| void | identity | void |\n| never | identity | never |\n| byte | identity | byte |\n",
    "| bytes | identity | bytes |\n| char | identity | char |\n| str | identity | str |\n",
    "| ptr | forbid | |\n| fnptr | forbid | |\n",
);

/// Parse → lower → emit `src` through the fixture def, panicking with context
/// on any pipeline error.
fn t(src: &str) -> String {
    let lang = parse_language_def(FIXTURE_DEF).expect("fixture def parses");
    transpile(src, &lang).unwrap_or_else(|e| panic!("transpile failed for {src:?}: {e}"))
}

// ---- functions ------------------------------------------------------------

#[test]
fn function_with_params_and_return() {
    let out = t("public fn add(x: i32, y: i32): i32 { return x + y; }");
    assert_eq!(out, "pub fn add(x: i32, y: i32) -> i32 {\n    return x + y;\n}");
}

#[test]
fn void_function_no_params() {
    let out = t("fn noop(): void { return; }");
    assert_eq!(out, "fn noop() {\n    return;\n}");
}

// ---- struct ---------------------------------------------------------------

#[test]
fn struct_declaration() {
    let out = t("struct Point { x: i32, y: i32 }");
    assert_eq!(out, "struct Point {\n    x: i32,\n    y: i32,\n}");
}

// ---- enum with payloads ---------------------------------------------------

#[test]
fn enum_with_unit_tuple_and_struct_variants() {
    let out = t("enum Shape { Empty, Circle(f64), Rect { w: f64, h: f64 } }");
    // The `{variants}` slot sits at column 4, so each non-first variant's
    // leading `\n` is followed by the slot's column-4 continuation indent.
    assert_eq!(
        out,
        "enum Shape {\n    Empty\n    Circle(f64)\n    Rect { w: f64, h: f64 }\n}"
    );
}

// ---- statements -----------------------------------------------------------

#[test]
fn let_and_assign_and_if_and_while() {
    let src = "fn f(): void { \
        let n = 1; \
        n = n + 2; \
        if (n) { return; } \
        while (n) { n = n; } \
    }";
    let out = t(src);
    assert_eq!(
        out,
        "fn f() {\n    \
             let n = 1;\n    \
             n = n + 2;\n    \
             if n {\n        return;\n    }\n    \
             while n {\n        n = n;\n    }\n}"
    );
}

// ---- operators (including the `~/` floor-division spelling) ---------------

#[test]
fn floor_division_tilde_slash_emits_canonical_spelling() {
    // The source `~/` parses to `BinaryOp::FloorDiv`, whose canonical operator
    // spelling (the fixture renders `{op}` with no exception) is `//`.
    let out = t("fn f(): i32 { return a ~/ b; }");
    assert_eq!(out, "fn f() -> i32 {\n    return a // b;\n}");
}

#[test]
fn mixed_operators_render() {
    let out = t("fn f(): bool { return !a == b * c; }");
    // Precedence: `!a == (b * c)`. Compound operands are parenthesized by the
    // engine to preserve grouping.
    assert_eq!(out, "fn f() -> bool {\n    return (!a) == (b * c);\n}");
}

// ---- lambda ---------------------------------------------------------------

#[test]
fn lambda_renders_as_closure() {
    let out = t("fn f(): void { let g = (x: i32): i32 => { return x; }; }");
    assert_eq!(out, "fn f() {\n    let g = |x: i32| { return x; };\n}");
}

// ---- raw block (exercises the lower pass's target-aware arm resolution) ---

#[test]
fn grouped_raw_block_selects_the_fixture_arm() {
    // A grouped `raw { … }` with a `fixture` arm plus others must lower to the
    // `fixture` arm and emit it verbatim.
    let src = "fn f(): void { \
        raw { \
            rust    { let r = x.sqrt(); } \
            fixture { FIXTURE_LINE; } \
            else    { fallback(); } \
        } \
    }";
    let out = t(src);
    assert_eq!(out, "fn f() {\n    FIXTURE_LINE;\n}");
}

#[test]
fn raw_falls_back_to_else_when_no_arm_matches() {
    let src = "fn f(): void { \
        raw { \
            rust { let r = x.sqrt(); } \
            else { FALLBACK_LINE; } \
        } \
    }";
    let out = t(src);
    assert_eq!(out, "fn f() {\n    FALLBACK_LINE;\n}");
}

// ---- const (an item-level value) -----------------------------------------

#[test]
fn const_item() {
    let out = t("const MAX: i32 = 100;");
    assert_eq!(out, "const MAX: i32 = 100;");
}

// ---- call expression ------------------------------------------------------

#[test]
fn call_with_args() {
    let out = t("fn f(): void { g(x, y); }");
    assert_eq!(out, "fn f() {\n    g(x, y);\n}");
}

// ---- struct literal -------------------------------------------------------

#[test]
fn struct_literal_construction() {
    let out = t("fn f(): void { let p = Point { x: 1, y: 2 }; }");
    assert_eq!(out, "fn f() {\n    let p = Point { x: 1, y: 2 };\n}");
}
