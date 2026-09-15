//! End-to-end tests for the core equation:
//! `Lamina source + language definition = target source`.
//!
//! Language definitions are parsed from rigid `.mdl` documents — the engine
//! ships none built in. The same minimal Lamina program transpiles to two
//! targets: `i32` maps by *identity* in Rust (`i32`) but *widens* to `number`
//! in TypeScript, and the function keyword and separator differ — proving the
//! output is driven entirely by the (external) language definition.

use lamina_core::lang_doc::parse_language_def;
use lamina_core::transpile;

const SOURCE: &str = "fn answer() -> i32 { return 42; }";

const RUST_DEF: &str = "# Lamina Language Definition: rust\n\
    \n\
    ## Function\n\
    ```lang-function\n\
    keyword = \"fn\"\n\
    return_type_sep = \" -> \"\n\
    emit_return_type = true\n\
    ```\n\
    \n\
    ## Capabilities\n\
    ```lang-capabilities\n\
    i32 identity i32\n\
    ```\n";

const TS_DEF: &str = "# Lamina Language Definition: typescript\n\
    \n\
    ## Function\n\
    ```lang-function\n\
    keyword = \"function\"\n\
    return_type_sep = \": \"\n\
    emit_return_type = true\n\
    ```\n\
    \n\
    ## Capabilities\n\
    ```lang-capabilities\n\
    i32 widen number\n\
    ```\n";

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
    assert_ne!(
        rust, ts,
        "the language definition must drive divergent output from identical source"
    );
}

#[test]
fn transpiles_multiple_functions() {
    let src = "fn a() -> i32 { return 1; } fn b() -> i32 { return 2; }";
    let lang = parse_language_def(RUST_DEF).expect("rust def");
    let out = transpile(src, &lang).expect("rust emit");
    assert_eq!(
        out,
        "fn a() -> i32 {\n    return 1;\n}\n\nfn b() -> i32 {\n    return 2;\n}"
    );
}
