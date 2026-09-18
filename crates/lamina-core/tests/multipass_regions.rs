//! End-to-end proof of author-defined multi-pass AST rendering + named output
//! regions.
//!
//! These tests build hand-crafted `.mdl` fixture definitions (not full targets)
//! and hand-built ASTs, and drive the real engine to prove the two DUMB engine
//! primitives:
//!
//! 1. **Multi-pass** — the engine renders the whole unit once per declared
//!    pass, scoping `pass:`-annotated rules to their pass.
//! 2. **Named regions** — `region:`-annotated rule output routes into an
//!    author-named buffer; the buffers assemble in the declared `layout` order.
//!
//! The headline proof is a genuine HOISTING transform: a `while` statement
//! renders a recursive helper *definition* into a `helpers` region during one
//! pass and an inline *call* into the default `body` region during another,
//! with both sharing a single generated name via the memoized `fresh_name`
//! helper — exactly the imperative→functional lowering the mechanism exists to
//! enable, done entirely in the language definition with the engine staying
//! dumb.

use lamina_core::ast::{Expr, File, Function, Item, Meta, Primitive, Statement, Type, Visibility};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

/// The 26-primitive capability matrix every definition must carry (completeness
/// is enforced at load time). Reused verbatim by each fixture; the tests care
/// about passes/regions, not the matrix.
const CAPS: &str = "\
## Capabilities

| Primitive | Action   | Target |
|-----------|----------|--------|
| i8        | identity | i8     |
| i16       | identity | i16    |
| i32       | identity | i32    |
| i64       | identity | i64    |
| i128      | identity | i128   |
| u8        | identity | u8     |
| u16       | identity | u16    |
| u32       | identity | u32    |
| u64       | identity | u64    |
| u128      | identity | u128   |
| isize     | identity | isize  |
| usize     | identity | usize  |
| f16       | forbid   |        |
| bf16      | forbid   |        |
| f32       | identity | f32    |
| f64       | identity | f64    |
| f128      | forbid   |        |
| bool      | identity | bool   |
| void      | alias    | ()     |
| never     | alias    | !      |
| byte      | alias    | u8     |
| bytes     | wrap     | Bytes  |
| char      | identity | char   |
| str       | wrap     | String |
| ptr       | forbid   |        |
| fnptr     | forbid   |        |
";

fn parse(doc: &str) -> LanguageDef {
    parse_language_def(doc).unwrap_or_else(|e| panic!("fixture def should parse: {e}"))
}

fn i32_fn(name: &str, body: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::I32),
        body,
        meta: Meta::new(),
    })
}

// ---------------------------------------------------------------------------
// (a) HOISTING PROOF
// ---------------------------------------------------------------------------

/// A fixture that lowers a `while` loop to a hoisted recursive helper + an
/// inline call, coordinating the shared name via `fresh_name(loop, w)`.
///
/// Two passes: `collect` emits helper definitions into the `helpers` region;
/// `emit` emits the ordinary inline body. The `layout` places `helpers` first,
/// so the assembled output has every hoisted helper at the top and the calling
/// code below — the canonical hoisting shape.
const HOIST_DEF: &str = r#"# Lamina Language Definition: hoist-fixture

```lang-meta
lamina-format: 0.0.0
target: hoist-fixture
target-version: 0
```

## Passes

```lang-passes
passes: collect, emit
regions: helpers, body
layout: helpers, body
```

## Function

```template
{body}
```

### statement
| When          | Template        | pass/region |
|---------------|-----------------|-------------|
| stmt is while | @while_helper   | pass: collect | region: helpers |
| else          | ""              | pass: collect |
| stmt is while | @while_call     | pass: emit  |
| else          | "{stmt}"        | pass: emit  |

### while_helper
```template
fn {fresh_name(loop, w)}() {{ /* recursive loop */ }}
```

### while_call
```template
{fresh_name(loop, w)}();
```

### stmt
| When           | Template |
|----------------|----------|
| stmt is return | "return;" |
| else           | forbid |
"#;

#[test]
fn hoisting_definition_and_reference_share_a_fresh_name() {
    let doc = format!("{HOIST_DEF}\n{CAPS}");
    let def = parse(&doc);

    let file = File {
        items: vec![i32_fn(
            "f",
            vec![
                Statement::While {
                    cond: Expr::BoolLiteral(true),
                    body: vec![],
                },
                Statement::Return(None),
            ],
        )],
    };

    let out = emit(&file, &def).expect("emit");

    // The hoisted helper definition must appear (from the `helpers` region),
    // and the inline call must appear (from the `body` region).
    assert!(
        out.contains("fn loop_0() {"),
        "hoisted helper definition missing:\n{out}"
    );
    assert!(
        out.contains("loop_0();"),
        "inline call to the hoisted helper missing:\n{out}"
    );

    // They must SHARE the generated name (fresh_name memoized by (loop, w)):
    // both use `loop_0`.
    let helper_pos = out.find("fn loop_0()").expect("helper present");
    let call_pos = out.find("loop_0();").expect("call present");

    // Layout is `helpers, body`, so the hoisted definition must come BEFORE the
    // inline call in the assembled output.
    assert!(
        helper_pos < call_pos,
        "hoisted helper must precede its inline call (layout helpers, body):\n{out}"
    );

    // The inline `return;` from the non-while statement lands in body, after
    // the helper.
    assert!(out.contains("return;"), "inline body statement missing:\n{out}");
}

// ---------------------------------------------------------------------------
// (b) REGION ASSEMBLY IN DECLARED LAYOUT ORDER
// ---------------------------------------------------------------------------

/// A fixture with three regions assembled in a NON-source order (`c, a, b`) to
/// prove the engine concatenates strictly by the declared `layout`, not by the
/// order emission happened or the order regions were declared. The entry
/// template emits one token per region in the order `a, b, c`.
const LAYOUT_DEF: &str = r#"# Lamina Language Definition: layout3

```lang-meta
lamina-format: 0.0.0
target: layout3
target-version: 0
```

## Passes

```lang-passes
passes: only
regions: a, b, c
layout: c, a, b
```

## Function

```template
{into_a}{into_b}{into_c}
```

### into_a
region: a
```template
A
```

### into_b
region: b
```template
B
```

### into_c
region: c
```template
C
```
"#;

#[test]
fn regions_assemble_in_declared_layout_order() {
    let doc = format!("{LAYOUT_DEF}\n{CAPS}");
    let def = parse(&doc);
    let file = File {
        items: vec![i32_fn("f", vec![])],
    };
    let out = emit(&file, &def).expect("emit");
    // Emission order was a, b, c (template order), but layout is c, a, b.
    assert_eq!(out, "CAB", "regions must assemble in layout order c,a,b: {out:?}");
}

/// A layout may include the conventional `body` region among others, positioning
/// the inline/unrouted output relative to routed regions.
#[test]
fn body_region_positions_inline_output_in_layout() {
    let doc = format!(
        "{}\n{CAPS}",
        r#"# Lamina Language Definition: body-pos

```lang-meta
lamina-format: 0.0.0
target: body-pos
target-version: 0
```

## Passes

```lang-passes
passes: only
regions: prelude, body
layout: prelude, body
```

## Function

```template
{pre}main
```

### pre
region: prelude
```template
PRELUDE
```
"#
    );
    let def = parse(&doc);
    let file = File {
        items: vec![i32_fn("f", vec![])],
    };
    let out = emit(&file, &def).expect("emit");
    // `PRELUDE` routed to the prelude region; `main` (unrouted) lands in body.
    // Layout `prelude, body` => PRELUDE first, then main.
    assert_eq!(out, "PRELUDEmain", "body region should follow prelude: {out:?}");
}

// ---------------------------------------------------------------------------
// (c) NO-PASS BYTE-IDENTITY
// ---------------------------------------------------------------------------

/// A definition WITHOUT a `## Passes` section must render byte-identically to
/// the legacy single-pass path. We prove it by rendering the SAME AST with a
/// pass-free fixture and asserting the exact expected output — the multi-pass
/// machinery must add nothing to the no-pass path.
const NOPASS_DEF: &str = r#"# Lamina Language Definition: nopass-fixture

```lang-meta
lamina-format: 0.0.0
target: nopass-fixture
target-version: 0
```

## Function

```template
fn {name}() {{ {body} }}
```

### statement
| When           | Template |
|----------------|----------|
| first          | "{stmt}" |
| else           | " {stmt}" |

### stmt
| When           | Template |
|----------------|----------|
| stmt is return | "return;" |
| else           | forbid |
"#;

#[test]
fn no_pass_definition_is_byte_identical_to_single_pass() {
    let doc = format!("{NOPASS_DEF}\n{CAPS}");
    let def = parse(&doc);
    assert!(
        !def.passes.is_multipass(),
        "fixture must declare no passes"
    );

    let file = File {
        items: vec![
            i32_fn("a", vec![Statement::Return(None)]),
            i32_fn("b", vec![Statement::Return(None)]),
        ],
    };
    let out = emit(&file, &def).expect("emit");
    // Two items, joined by the legacy blank-line separator; each renders through
    // the ordinary inline path with NO region assembly.
    assert_eq!(out, "fn a() { return; }\n\nfn b() { return; }");
}

// ---------------------------------------------------------------------------
// (d) MINIMAL while -> functional-style render (strong signal)
// ---------------------------------------------------------------------------

/// A slightly richer hoisting fixture: the helper carries a real (if trivial)
/// recursive body referencing its own generated name, proving the shared
/// `fresh_name` id threads into the helper body too — the shape a functional
/// `while` lowering (Haskell `let go = ... in go`) needs.
#[test]
fn while_lowers_to_named_recursive_helper() {
    let doc = format!(
        "{}\n{CAPS}",
        r#"# Lamina Language Definition: functional-while

```lang-meta
lamina-format: 0.0.0
target: functional-while
target-version: 0
```

## Passes

```lang-passes
passes: collect, emit
regions: helpers, body
layout: helpers, body
```

## Function

```template
{body}
```

### statement
| When          | Template      | pass/region |
|---------------|---------------|-------------|
| stmt is while | @go_def       | pass: collect | region: helpers |
| else          | ""            | pass: collect |
| stmt is while | @go_call      | pass: emit  |
| else          | ""            | pass: emit  |

### go_def
```template
fn {fresh_name(go, loop)}() {{ {fresh_name(go, loop)}(); }}
```

### go_call
```template
{fresh_name(go, loop)}();
```
"#
    );
    let def = parse(&doc);
    let file = File {
        items: vec![i32_fn(
            "f",
            vec![Statement::While {
                cond: Expr::BoolLiteral(true),
                body: vec![],
            }],
        )],
    };
    let out = emit(&file, &def).expect("emit");
    // The helper name (go_0) appears THREE times: the definition's name, the
    // recursive self-call inside it, and the inline call — all the SAME name,
    // proving unit-wide memoization by (prefix, key) across passes and regions.
    assert_eq!(out.matches("go_0").count(), 3, "all three uses share go_0: {out}");
    assert_eq!(out, "fn go_0() { go_0(); }go_0();");
}
