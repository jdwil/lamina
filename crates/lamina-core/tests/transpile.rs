//! End-to-end tests for the core equation:
//! `Lamina source + language definition = target source`.
//!
//! Language definitions are parsed from rigid template-model `.mdl` documents:
//! a single entry `template` block plus `### slot` subsections. The same
//! minimal Lamina program transpiles to two targets — `i32` maps by *identity*
//! in Rust but *widens* to `number` in TypeScript — proving output is driven
//! entirely by the (external) language definition.

use lamina_core::lang_doc::parse_language_def;
use lamina_core::transpile;

const SOURCE: &str = "fn answer() -> i32 { return 42; }";

const RUST_DEF: &str = concat!(
    "# Lamina Language Definition: rust\n",
    "\n",
    "## Function\n",
    "\n",
    "```template\n",
    "{vis}{async}fn {name}({params}){ret} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### ret\n",
    "| When | Template |\n",
    "|------|----------|\n",
    "| else | \" -> {ret_type}\" |\n",
    "\n",
    "### vis\n",
    "| When           | Template |\n",
    "|----------------|----------|\n",
    "| vis is public  | \"pub \" |\n",
    "| else           | \"\" |\n",
    "\n",
    "### async\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| async | \"async \" |\n",
    "| else  | \"\" |\n",
    "\n",
    "### param\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n",
    "\n",
    "### statement\n",
    "```template\n",
    "return {value};\n",
    "```\n",
    "\n",
    "### expr\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| expr is int | \"{value}\" |\n",
    "| else        | forbid |\n",
    "\n",
    "## Capabilities\n",
    "| Primitive | Action   | Target |\n",
    "|-----------|----------|--------|\n",
    "| i32       | identity | i32    |\n",
    "| i8 | identity | i8 |\n",
    "| i16 | identity | i16 |\n",
    "| i64 | identity | i64 |\n",
    "| i128 | identity | i128 |\n",
    "| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n",
    "| u32 | identity | u32 |\n",
    "| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n",
    "| isize | identity | isize |\n",
    "| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n",
    "| bf16 | identity | bf16 |\n",
    "| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n",
    "| f128 | identity | f128 |\n",
    "| bool | identity | bool |\n",
    "| void | alias | () |\n",
    "| never | alias | ! |\n",
    "| byte | alias | u8 |\n",
    "| bytes | wrap | Vec |\n",
    "| char | identity | char |\n",
    "| str | wrap | String |\n",
    "| ptr | wrap | Ptr |\n",
    "| fnptr | wrap | Fn |\n",
);

const TS_DEF: &str = concat!(
    "# Lamina Language Definition: typescript\n",
    "\n",
    "## Function\n",
    "\n",
    "```template\n",
    "{async}function {name}({params}){ret} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### ret\n",
    "| When | Template |\n",
    "|------|----------|\n",
    "| else | \": {ret_type}\" |\n",
    "\n",
    "### async\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| async | \"async \" |\n",
    "| else  | \"\" |\n",
    "\n",
    "### param\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n",
    "\n",
    "### statement\n",
    "```template\n",
    "return {value};\n",
    "```\n",
    "\n",
    "### expr\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| expr is int | \"{value}\" |\n",
    "| else        | forbid |\n",
    "\n",
    "## Capabilities\n",
    "| Primitive | Action | Target  |\n",
    "|-----------|--------|---------|\n",
    "| i32       | widen  | number  |\n",
    "| i8 | widen | number |\n",
    "| i16 | widen | number |\n",
    "| i64 | widen | bigint |\n",
    "| i128 | widen | bigint |\n",
    "| u8 | widen | number |\n",
    "| u16 | widen | number |\n",
    "| u32 | widen | number |\n",
    "| u64 | widen | bigint |\n",
    "| u128 | widen | bigint |\n",
    "| isize | widen | number |\n",
    "| usize | widen | number |\n",
    "| f16 | widen | number |\n",
    "| bf16 | widen | number |\n",
    "| f32 | widen | number |\n",
    "| f64 | widen | number |\n",
    "| f128 | forbid | |\n",
    "| bool | identity | boolean |\n",
    "| void | alias | void |\n",
    "| never | alias | never |\n",
    "| byte | widen | number |\n",
    "| bytes | wrap | Uint8Array |\n",
    "| char | alias | string |\n",
    "| str | alias | string |\n",
    "| ptr | forbid | |\n",
    "| fnptr | forbid | |\n",
);

#[test]
fn transpiles_to_rust() {
    let lang = parse_language_def(RUST_DEF).expect("rust def parses");
    let out = transpile(SOURCE, &lang).expect("should transpile to rust");
    assert_eq!(out, "fn answer() -> i32 {\n    return 42;\n}");
}

#[test]
fn transpiles_to_typescript() {
    let lang = parse_language_def(TS_DEF).expect("ts def parses");
    let out = transpile(SOURCE, &lang).expect("should transpile to typescript");
    assert_eq!(out, "function answer(): number {\n    return 42;\n}");
}

#[test]
fn same_source_two_targets_differ() {
    let rust = transpile(SOURCE, &parse_language_def(RUST_DEF).expect("rust")).expect("rust emit");
    let ts = transpile(SOURCE, &parse_language_def(TS_DEF).expect("ts")).expect("ts emit");
    assert_ne!(rust, ts);
}

#[test]
fn modifiers_and_visibility_via_templates() {
    let src = "public async fn a() -> i32 { return 1; }";
    let lang = parse_language_def(RUST_DEF).expect("rust");
    let out = transpile(src, &lang).expect("emit");
    assert_eq!(out, "pub async fn a() -> i32 {\n    return 1;\n}");
}
