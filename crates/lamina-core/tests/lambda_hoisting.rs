//! End-to-end proof of multi-statement `Expr::Lambda` HOISTING — kernel blocker
//! #2 — driven entirely by a language definition with the engine staying dumb.
//!
//! These tests build a hand-crafted `.mdl` fixture (a minimal brace-language
//! target, independent of any shipped def) and hand-built ASTs, then drive the
//! real engine to prove the four fixes that make hoisting expressible:
//!
//! 1. the closed **body-cardinality** fact `body is single|block` lets a def
//!    branch a single-expression lambda (rendered inline) apart from a
//!    multi-statement one (hoisted);
//! 2. a **projected statement-sequence** `{body:item_slot}` renders the hoisted
//!    body through a chosen item slot (its body is NON-EMPTY — the core-bug
//!    proof);
//! 3. a **single-pass, two-region** hoist routes the lifted definition into a
//!    top-level region while the lambda site emits an inline reference; and
//! 4. **`fresh_name` keyed per lambda** makes a lambda's definition and
//!    reference share one name while two DISTINCT lambdas get two distinct
//!    names (no collision).

use lamina_core::ast::{
    BinaryOp, Expr, File, Function, Item, Meta, Param, Primitive, Statement, Type, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

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

/// A fixture target that renders a single-expression lambda inline (`|x| expr`)
/// and hoists a multi-statement lambda to a named `fn` in a `defs` region,
/// leaving the generated name as the inline reference. One pass, two regions.
const HOIST_DEF: &str = r#"# Lamina Language Definition: lam-hoist-fixture

```lang-meta
lamina-format: 0.0.0
target: lam-hoist-fixture
target-version: 0
```

## Passes

```lang-passes
passes: only
regions: defs, body
layout: defs, body
```

## Function

```template
fn {name}() {{ {body} }}
```

### statement
| When  | Template |
|-------|----------|
| first | @stmt    |
| else  | " {stmt}" |

### stmt
| When                        | Template |
|-----------------------------|----------|
| stmt is let && has_value    | "let {name} = {value};" |
| stmt is return && has_value | "return {value};" |
| stmt is expr                | "{value};" |
| else                        | forbid |

### expr
| When                             | Template |
|----------------------------------|----------|
| expr is lambda && body is single | @lam_inline |
| expr is lambda && body is block  | @lam_hoist |
| expr is int                      | "{value}" |
| expr is ref                      | "{value}" |
| expr is binary                   | "{lhs} {op} {rhs}" |
| else                             | forbid |

### lam_inline
```template
|{params}| {body:lam_tail}
```

### lam_tail
| When                        | Template |
|-----------------------------|----------|
| stmt is return && has_value | "{value}" |
| stmt is expr                | "{value}" |
| else                        | forbid |

### lam_hoist
```template
{lam_def}{fresh_name(lam, def)}
```

### lam_def
region: defs
```template
fn {fresh_name(lam, def)}({params}) {{ {body:lam_stmt} }}
```

### lam_stmt
| When  | Template |
|-------|----------|
| first | @stmt    |
| else  | " {stmt}" |

### param
| When  | Template |
|-------|----------|
| first | "{name}: {type}" |
| else  | ", {name}: {type}" |
"#;

fn def() -> LanguageDef {
    parse_language_def(&format!("{HOIST_DEF}\n{CAPS}"))
        .unwrap_or_else(|e| panic!("fixture def should parse: {e}"))
}

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}
fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}
fn r(n: &str) -> Expr {
    Expr::Ref(n.to_string())
}
fn add(a: Expr, b: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        lhs: Box::new(a),
        rhs: Box::new(b),
    }
}
fn param(n: &str) -> Param {
    Param {
        name: n.to_string(),
        ty: i32t(),
        meta: Meta::new(),
    }
}
fn func(body: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body,
        meta: Meta::new(),
    })
}

/// A single-expression lambda body (`body is single`) renders INLINE — no hoist,
/// no `defs` region content.
#[test]
fn single_expression_lambda_renders_inline() {
    // let g = |x| x + 1;   (body is one expression-statement)
    let lambda = Expr::Lambda {
        params: vec![param("x")],
        return_type: None,
        body: vec![Statement::Expr(add(r("x"), int("1")))],
        meta: Meta::new(),
    };
    let file = File {
        items: vec![func(vec![Statement::Let {
            name: "g".to_string(),
            ty: None,
            value: Some(lambda),
        }])],
    };
    let out = emit(&file, &def()).expect("emit");
    // No hoisted def; the lambda is spelled inline. (defs region is empty.)
    assert_eq!(out, "fn f() { let g = |x: i32| x + 1; }");
    assert!(!out.contains("fn lam_"), "single-expr lambda must NOT hoist:\n{out}");
}

/// A multi-statement lambda body (`body is block`) HOISTS to a named `fn` with a
/// NON-EMPTY body, and the lambda site references the SAME generated name.
#[test]
fn multi_statement_lambda_hoists_to_named_fn() {
    // let g = |x| { let y = x + 1; return y; };   (body is a block)
    let lambda = Expr::Lambda {
        params: vec![param("x")],
        return_type: None,
        body: vec![
            Statement::Let {
                name: "y".to_string(),
                ty: None,
                value: Some(add(r("x"), int("1"))),
            },
            Statement::Return(Some(r("y"))),
        ],
        meta: Meta::new(),
    };
    let file = File {
        items: vec![func(vec![Statement::Let {
            name: "g".to_string(),
            ty: None,
            value: Some(lambda),
        }])],
    };
    let out = emit(&file, &def()).expect("emit");
    // The hoisted `fn` (with a NON-EMPTY body — the core bug) appears at the top
    // (the `defs` region, assembled first), and the lambda site references it.
    assert_eq!(
        out,
        "fn lam_0(x: i32) { let y = x + 1; return y; }fn f() { let g = lam_0; }"
    );
    // The hoisted definition body is genuinely non-empty.
    assert!(
        out.contains("fn lam_0(x: i32) { let y = x + 1; return y; }"),
        "hoisted body must be non-empty:\n{out}"
    );
    // The hoisted definition precedes the calling code (layout `defs, body`).
    let def_pos = out.find("fn lam_0(").expect("hoisted def present");
    let ref_pos = out.find("let g = lam_0").expect("reference present");
    assert!(def_pos < ref_pos, "hoisted def must precede its reference:\n{out}");
    // Definition and reference SHARE one generated name.
    assert_eq!(out.matches("lam_0").count(), 2, "def + ref share one name:\n{out}");
}

/// TWO distinct multi-statement lambdas get TWO distinct hoisted names — no
/// collision — each definition/reference pair internally consistent.
#[test]
fn two_distinct_lambdas_get_distinct_hoisted_names() {
    let mk = |var: &str| Expr::Lambda {
        params: vec![param("x")],
        return_type: None,
        body: vec![
            Statement::Let {
                name: var.to_string(),
                ty: None,
                value: Some(add(r("x"), int("1"))),
            },
            Statement::Return(Some(r(var))),
        ],
        meta: Meta::new(),
    };
    let file = File {
        items: vec![func(vec![
            Statement::Let {
                name: "g".to_string(),
                ty: None,
                value: Some(mk("y")),
            },
            Statement::Let {
                name: "h".to_string(),
                ty: None,
                value: Some(mk("z")),
            },
        ])],
    };
    let out = emit(&file, &def()).expect("emit");
    // Two distinct hoisted names, each referenced by its own lambda site.
    assert!(out.contains("fn lam_0("), "first hoisted def missing:\n{out}");
    assert!(out.contains("fn lam_1("), "second hoisted def missing:\n{out}");
    assert!(out.contains("let g = lam_0;"), "g must reference lam_0:\n{out}");
    assert!(out.contains("let h = lam_1;"), "h must reference lam_1:\n{out}");
    // No collision: each name appears exactly twice (its def + its reference).
    assert_eq!(out.matches("lam_0").count(), 2, "lam_0 = def + ref:\n{out}");
    assert_eq!(out.matches("lam_1").count(), 2, "lam_1 = def + ref:\n{out}");
    // The two hoisted bodies are the two distinct lambda bodies.
    assert!(out.contains("let y = x + 1; return y;"), "lam_0 body:\n{out}");
    assert!(out.contains("let z = x + 1; return z;"), "lam_1 body:\n{out}");
}
