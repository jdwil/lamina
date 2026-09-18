//! Integration tests for construct **metadata** (Part 1), the closed
//! **resolution/render helpers** (Part 2), and **anonymous-form
//! reconstruction** (Part 3) — see `/tmp/lamina-metadata-helpers.md`.
//!
//! These build the ASTs a *layer* would produce (hoisted items + use-site
//! references / struct-literals, carrying the metadata a layer would attach)
//! and assert the language definition reconstructs the idiomatic anonymous
//! form using metadata + helpers, with the engine staying transparent to
//! metadata and dumb about targets.

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Primitive, Statement, Type,
    Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::lang_doc::parse_language_def;

// ---- helpers -----------------------------------------------------------

fn lang(src: &str) -> LanguageDef {
    parse_language_def(src).unwrap_or_else(|e| panic!("def should parse/load: {e}"))
}

fn emit_file(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_err(items: Vec<Item>, lang: &LanguageDef) -> lamina_core::EmitError {
    emit(&File { items }, lang).expect_err("expected emit error")
}

/// A minimal function `fn <name>() { return <expr>; }` with the given metadata.
fn func_meta(name: &str, ret: Type, body: Vec<Statement>, meta: Meta) -> Function {
    Function {
        name: name.to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: ret,
        body,
        meta,
    }
}

/// A caps matrix covering every kernel primitive by identity (i8..fnptr), so a
/// self-contained test def loads. Only the rows the tests exercise matter.
const CAPS: &str = concat!(
    "## Capabilities\n\n",
    "| Primitive | Action | Target |\n",
    "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
    "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
    "| void | alias | void |\n| never | alias | never |\n| byte | alias | u8 |\n",
    "| bytes | wrap | Vec |\n| char | identity | char |\n| str | wrap | String |\n",
    "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
);

// =======================================================================
// Part 1 — Construct metadata
// =======================================================================

/// A def whose `## Function` entry branches on function metadata: it prefixes a
/// `/* origin=<v> */` comment when `meta.origin` is set, proving both the
/// `meta.<key> is <value>` fact and the `{meta.<key>}` slot work — and that a
/// function WITHOUT metadata renders byte-identically to the plain form.
fn meta_fn_def() -> LanguageDef {
    lang(&[
        concat!(
            "# Lamina Language Definition: metafn\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: metafn\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\n{origin_tag}fn {name}() {{\n    {body}\n}}\n```\n\n",
            "### origin_tag\n",
            "| When                  | Template |\n",
            "|-----------------------|----------|\n",
            "| has_meta(origin)      | \"/* origin={meta.origin} */ \" |\n",
            "| else                  | \"\" |\n\n",
            "### statement\n```template\nreturn {value};\n```\n\n",
            "### expr\n",
            "| When        | Template |\n",
            "|-------------|----------|\n",
            "| expr is int | \"{value}\" |\n",
            "| else        | forbid |\n\n",
        ), CAPS].concat())
}

#[test]
fn metadata_fact_and_slot_render() {
    let def = meta_fn_def();
    let ret = Type::Primitive(Primitive::Void);
    let body = vec![Statement::Return(Some(Expr::IntLiteral("0".into())))];
    // Tagged function: the `has_meta(origin)` row fires and `{meta.origin}`
    // renders the value.
    let tagged = func_meta("f", ret.clone(), body.clone(), Meta::new().with("origin", "anon_fn"));
    let out = emit_file(vec![Item::Function(tagged)], &def);
    assert_eq!(out, "/* origin=anon_fn */ fn f() {\n    return 0;\n}", "got: {out}");
}

#[test]
fn empty_metadata_renders_byte_identically() {
    let def = meta_fn_def();
    let ret = Type::Primitive(Primitive::Void);
    let body = vec![Statement::Return(Some(Expr::IntLiteral("0".into())))];
    // No metadata -> the `else` row -> no prefix, identical to pre-metadata.
    let plain = func_meta("f", ret, body, Meta::new());
    let out = emit_file(vec![Item::Function(plain)], &def);
    assert_eq!(out, "fn f() {\n    return 0;\n}", "got: {out}");
}

#[test]
fn structural_equality_ignores_metadata() {
    // Two structurally identical expressions differing ONLY in metadata are
    // equal; metadata never participates in `PartialEq`.
    let bare = Expr::StructLit {
        type_name: "P".into(),
        fields: vec![FieldInit {
            name: "x".into(),
            value: Expr::IntLiteral("1".into()),
            meta: Meta::new(),
        }],
        meta: Meta::new(),
    };
    let tagged = Expr::StructLit {
        type_name: "P".into(),
        fields: vec![FieldInit {
            name: "x".into(),
            value: Expr::IntLiteral("1".into()),
            meta: Meta::new().with("capture", "true"),
        }],
        meta: Meta::new().with("origin", "anon_class"),
    };
    assert_eq!(bare, tagged, "metadata must not affect structural equality");

    // And a genuine structural difference is still unequal.
    let different = Expr::StructLit {
        type_name: "Q".into(),
        fields: vec![],
        meta: Meta::new().with("origin", "anon_class"),
    };
    assert_ne!(bare, different);
}

// =======================================================================
// Part 2 — Resolution / render helpers
// =======================================================================

/// A def exercising the `escape(<arg>, <style>)` template helper: string
/// literals are emitted with C-style escaping applied to their (unescaped)
/// stored contents.
fn escape_def() -> LanguageDef {
    lang(&[
        concat!(
            "# Lamina Language Definition: esc\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: esc\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            "### statement\n```template\nreturn {value};\n```\n\n",
            "### expr\n",
            "| When           | Template |\n",
            "|----------------|----------|\n",
            "| expr is string | \"\\\"{escape(value, c)}\\\"\" |\n",
            "| expr is int    | \"{value}\" |\n",
            "| else           | forbid |\n\n",
        ), CAPS].concat())
}

#[test]
fn escape_helper_escapes_string_contents() {
    let def = escape_def();
    // Stored contents are unescaped: a newline and a quote and a backslash.
    let body = vec![Statement::Return(Some(Expr::StringLiteral(
        "a\nb\"c\\d".into(),
    )))];
    let f = func_meta("f", Type::Primitive(Primitive::Void), body, Meta::new());
    let out = emit_file(vec![Item::Function(f)], &def);
    // The `escape(value, c)` helper applies C escaping; the template supplies
    // the surrounding quotes.
    assert_eq!(
        out,
        "fn f() {\n    return \"a\\nb\\\"c\\\\d\";\n}",
        "got: {out}"
    );
}

/// A def that inlines a function-pointer value via `resolve_fnptr` through a
/// `### anon_fn` slot (a Rust-like closure spelling). Used to prove Part 2's
/// `resolve_fnptr` and Part 3a's anonymous-function reconstruction.
fn anon_fn_def() -> LanguageDef {
    lang(&[
        concat!(
            "# Lamina Language Definition: anonfn\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: anonfn\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            // The anonymous-function spelling used when a fnptr value is
            // inlined via resolve_fnptr: a Rust-like closure `|| <body-expr>`.
            "### anon_fn\n```template\n|| {body}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n",
            "| When            | Template |\n",
            "|-----------------|----------|\n",
            "| expr is int     | \"{value}\" |\n",
            "| expr is ref     | \"{value}\" |\n",
            "| expr is binary  | \"{lhs} {op} {rhs}\" |\n",
            // A struct-literal field whose value is a single-use anon_fn fnptr
            // is inlined as a closure; otherwise the field renders normally.
            "| expr is struct_lit | @struct_lit |\n",
            "| else            | forbid |\n\n",
            "### struct_lit\n```template\n{type_name} {{ {fields} }}\n```\n\n",
            "### field_init\n",
            "| When                                              | Template |\n",
            "|---------------------------------------------------|----------|\n",
            "| resolve(value) is function && fnptr_ref_count(value) is 1 | \"{name}: {resolve_fnptr(value)}\" |\n",
            "| first                                             | \"{name}: {value}\" |\n",
            "| else                                              | \", {name}: {value}\" |\n\n",
            "### expr_arg\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{value}\" |\n",
            "| else  | \", {value}\" |\n\n",
        ), CAPS].concat())
}

/// Builds the layer-shaped AST for an anonymous function stored in a struct:
/// a hoisted `fn lam() { <body> }` tagged `origin=anon_fn`, and a struct literal
/// `Holder { cb: lam }` where `cb` holds the fnptr value.
fn anon_fn_unit(body_expr: Expr) -> Vec<Item> {
    let lam = func_meta(
        "lam",
        Type::Primitive(Primitive::I32),
        vec![Statement::Return(Some(body_expr))],
        Meta::new().with("origin", "anon_fn"),
    );
    let holder = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Holder".into()),
        body: vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Holder".into(),
            fields: vec![FieldInit {
                name: "cb".into(),
                value: Expr::Ref("lam".into()),
                meta: Meta::new(),
            }],
            meta: Meta::new(),
        }))],
        meta: Meta::new(),
    };
    vec![Item::Function(lam), Item::Function(holder)]
}

#[test]
fn resolve_fnptr_inlines_single_use_anonymous_function() {
    let def = anon_fn_def();
    // `lam` returns `x * 2`; used exactly once as a value in `Holder { cb: lam }`.
    let body_expr = Expr::Binary {
        op: BinaryOp::Mul,
        lhs: Box::new(Expr::Ref("x".into())),
        rhs: Box::new(Expr::IntLiteral("2".into())),
    };
    let out = emit_file(anon_fn_unit(body_expr), &def);
    // The `make` function's returned struct literal inlines `lam` as a closure
    // `|| x * 2` rather than referencing the hoisted name.
    assert!(
        out.contains("Holder { cb: || x * 2 }"),
        "expected inlined closure, got: {out}"
    );
    // The hoisted `fn lam` is still emitted (this def does not elide it).
    assert!(out.contains("fn lam()"), "got: {out}");
}

#[test]
fn fnptr_ref_count_gt_one_does_not_inline() {
    let def = anon_fn_def();
    // `lam` is referenced as a value TWICE -> not single-use -> not inlined.
    let lam = func_meta(
        "lam",
        Type::Primitive(Primitive::I32),
        vec![Statement::Return(Some(Expr::IntLiteral("1".into())))],
        Meta::new().with("origin", "anon_fn"),
    );
    let holder = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Holder".into()),
        body: vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Holder".into(),
            fields: vec![
                FieldInit {
                    name: "a".into(),
                    value: Expr::Ref("lam".into()),
                    meta: Meta::new(),
                },
                FieldInit {
                    name: "b".into(),
                    value: Expr::Ref("lam".into()),
                    meta: Meta::new(),
                },
            ],
            meta: Meta::new(),
        }))],
        meta: Meta::new(),
    };
    let out = emit_file(vec![Item::Function(lam), Item::Function(holder)], &def);
    // Two references -> `fnptr_ref_count is 1` is false -> plain name is kept.
    assert!(out.contains("Holder { a: lam, b: lam }"), "got: {out}");
}

#[test]
fn resolve_and_type_of_facts_via_helpers() {
    // A def that branches on `resolve(value) is function` (a fnptr) vs a plain
    // reference, and on `type_of(self) is <TypeName>` for a struct literal.
    let def = lang(&[
        concat!(
            "# Lamina Language Definition: resolvefacts\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: resolvefacts\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n",
            "| When                       | Template |\n",
            "|----------------------------|----------|\n",
            "| expr is struct_lit && type_of(self) is Widget | \"WIDGET({type_name})\" |\n",
            "| expr is struct_lit         | \"OTHER({type_name})\" |\n",
            "| expr is ref && resolve(self) is function | \"FN:{value}\" |\n",
            "| expr is ref                | \"{value}\" |\n",
            "| expr is int                | \"{value}\" |\n",
            "| else                       | forbid |\n\n",
        ), CAPS].concat());
    // A function `g`, referenced as a value -> `resolve(self) is function`.
    let g = func_meta("g", Type::Primitive(Primitive::Void), vec![], Meta::new());
    let user = Function {
        name: "u".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![
            // A struct literal whose type resolves to `Widget`.
            Statement::Expr(Expr::StructLit {
                type_name: "Widget".into(),
                fields: vec![],
                meta: Meta::new(),
            }),
            // A bare fnptr reference to `g`.
            Statement::Expr(Expr::Ref("g".into())),
        ],
        meta: Meta::new(),
    };
    let out = emit_file(vec![Item::Function(g), Item::Function(user)], &def);
    assert!(out.contains("WIDGET(Widget)"), "type_of fact: {out}");
    assert!(out.contains("FN:g"), "resolve fact: {out}");
}

// =======================================================================
// Part 3 — Anonymous class reconstruction
// =======================================================================

/// A TypeScript-like def that reconstructs an anonymous class from a struct
/// literal tagged `origin=anon_class`: it emits an object literal whose method
/// fields (tagged `member=method`) inline the resolved method function bodies
/// via `resolve_fnptr`, and whose data fields render normally.
fn anon_class_def() -> LanguageDef {
    lang(&[
        concat!(
            "# Lamina Language Definition: anonclass\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: anonclass\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            // Object-literal method spelling for an inlined method fnptr.
            "### anon_fn\n```template\n() => {body}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n",
            "| When            | Template |\n",
            "|-----------------|----------|\n",
            "| expr is int     | \"{value}\" |\n",
            "| expr is ref     | \"{value}\" |\n",
            "| expr is struct_lit && meta.origin is anon_class | @anon_object |\n",
            "| expr is struct_lit | @struct_lit |\n",
            "| else            | forbid |\n\n",
            // A tagged anonymous class -> a TS object literal `{ field: … }`.
            "### anon_object\n```template\n{{ {fields} }}\n```\n\n",
            "### struct_lit\n```template\n{type_name} {{ {fields} }}\n```\n\n",
            // A method field inlines the resolved fn as an arrow method; a data
            // field renders its value.
            "### field_init\n",
            "| When                        | Template |\n",
            "|-----------------------------|----------|\n",
            "| has_meta(method) && first   | \"{name}: {resolve_fnptr(value)}\" |\n",
            "| has_meta(method)            | \", {name}: {resolve_fnptr(value)}\" |\n",
            "| first                       | \"{name}: {value}\" |\n",
            "| else                        | \", {name}: {value}\" |\n\n",
        ), CAPS].concat())
}

#[test]
fn anonymous_class_reconstructs_object_with_inlined_methods() {
    let def = anon_class_def();
    // Hoisted method fn `greet` returning 42, tagged origin=anon_fn.
    let greet = func_meta(
        "greet",
        Type::Primitive(Primitive::I32),
        vec![Statement::Return(Some(Expr::IntLiteral("42".into())))],
        Meta::new().with("origin", "anon_fn"),
    );
    // A struct literal tagged origin=anon_class: a data field `count: 1` and a
    // method field `greet` (tagged member=method) holding the fnptr `greet`.
    let obj = Expr::StructLit {
        type_name: "Anon".into(),
        fields: vec![
            FieldInit {
                name: "count".into(),
                value: Expr::IntLiteral("1".into()),
                meta: Meta::new(),
            },
            FieldInit {
                name: "greet".into(),
                value: Expr::Ref("greet".into()),
                meta: Meta::new().with("method", "true"),
            },
        ],
        meta: Meta::new().with("origin", "anon_class"),
    };
    let user = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Anon".into()),
        body: vec![Statement::Return(Some(obj))],
        meta: Meta::new(),
    };
    let out = emit_file(vec![Item::Function(greet), Item::Function(user)], &def);
    // The anonymous class becomes an object literal (no type name), the data
    // field renders plainly, and the method field inlines the resolved body as
    // an arrow function.
    assert!(
        out.contains("{ count: 1, greet: () => 42 }"),
        "expected reconstructed object literal, got: {out}"
    );
}

// =======================================================================
// Part 3b — Closure capture-environment reconstruction
// =======================================================================

#[test]
fn closure_captures_are_known_from_metadata_not_structure() {
    // A capture-environment struct: its fields are tagged `capture=true`, and a
    // language def can emit a native closure capturing exactly those. Here we
    // assert the metadata drives which fields are captures (a plain data field
    // is NOT tagged), independent of structural shape.
    let env = Item::Struct {
        name: "Env".into(),
        visibility: Visibility::Private,
        attributes: Vec::new(),
        fields: vec![
            Field {
                name: "captured_x".into(),
                ty: Type::Primitive(Primitive::I32),
                visibility: Visibility::Private,
                meta: Meta::new().with("capture", "true"),
            },
            Field {
                name: "not_captured".into(),
                ty: Type::Primitive(Primitive::I32),
                visibility: Visibility::Private,
                meta: Meta::new(),
            },
        ],
        meta: Meta::new().with("role", "closure_env"),
    };
    // A def that renders only capture-tagged fields into a Rust-like capture
    // list `[captured_x]`, proving captures come from METADATA.
    let def = lang(&[
        concat!(
            "# Lamina Language Definition: closureenv\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: closureenv\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{}}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n| When | Template |\n|------|----------|\n| else | forbid |\n\n",
            "## Struct\n\n",
            "```template\n[{fields}]\n```\n\n",
            "### field\n",
            "| When              | Template |\n",
            "|-------------------|----------|\n",
            "| has_meta(capture) && first | \"{name}\" |\n",
            "| has_meta(capture) | \", {name}\" |\n",
            "| else              | \"\" |\n\n",
        ), CAPS].concat());
    let out = emit_file(vec![env], &def);
    // Only the capture-tagged field appears in the capture list.
    assert_eq!(out, "[captured_x]", "got: {out}");
}

// =======================================================================
// C-style target leaves the fnptr + hoisted fn (no reconstruction)
// =======================================================================

#[test]
fn c_style_target_keeps_fnptr_and_hoisted_fn() {
    // A def WITHOUT any anon reconstruction rows leaves the struct literal's
    // fnptr field as a plain name and keeps the hoisted function — the correct
    // degradation for a target with no inline function form.
    let def = lang(&[
        concat!(
            "# Lamina Language Definition: cish\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: cish\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n",
            "| When            | Template |\n",
            "|-----------------|----------|\n",
            "| expr is int     | \"{value}\" |\n",
            "| expr is ref     | \"{value}\" |\n",
            "| expr is struct_lit | \"{type_name} {{ {fields} }}\" |\n",
            "| else            | forbid |\n\n",
            "### field_init\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{name}: {value}\" |\n",
            "| else  | \", {name}: {value}\" |\n\n",
        ), CAPS].concat());
    let out = emit_file(anon_fn_unit(Expr::IntLiteral("1".into())), &def);
    assert!(out.contains("Holder { cb: lam }"), "fnptr kept: {out}");
    assert!(out.contains("fn lam()"), "hoisted fn kept: {out}");
}

// ---- a forbidden/absent metadata slot renders empty (forgiving) -------

#[test]
fn absent_metadata_slot_renders_empty() {
    let def = meta_fn_def();
    // Reference `{meta.origin}` on a node with no such key -> empty string.
    // (The `origin_tag` else row never references it, so exercise the slot
    // directly via a tagged-then-untagged pair is covered above; here confirm a
    // different key is simply empty.)
    let f = func_meta(
        "f",
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(Some(Expr::IntLiteral("0".into())))],
        Meta::new().with("unrelated", "x"),
    );
    let out = emit_file(vec![Item::Function(f)], &def);
    // `has_meta(origin)` is false (only `unrelated` is set) -> no prefix.
    assert_eq!(out, "fn f() {\n    return 0;\n}", "got: {out}");
}

#[test]
fn field_type_helper_renders_declared_field_type() {
    // A def that renders a struct-literal field with an explicit type annotation
    // resolved via `{field_type(Type, field)}` — proving the closed
    // symbol/type-resolution helper crosses to the struct declaration.
    let def = lang(&[
        concat!(
            "# Lamina Language Definition: ftype\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: ftype\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            "### statement\n```template\n{value}\n```\n\n",
            "### expr\n",
            "| When            | Template |\n",
            "|-----------------|----------|\n",
            "| expr is int     | \"{value}\" |\n",
            "| expr is struct_lit | \"{type_name} {{ {fields} }}\" |\n",
            "| else            | forbid |\n\n",
            // Each field annotates its declared type via field_type(Type, name).
            "### field_init\n",
            "| When  | Template |\n",
            "|-------|----------|\n",
            "| first | \"{name}: {field_type(Point, x)} = {value}\" |\n",
            "| else  | \", {name}: {field_type(Point, x)} = {value}\" |\n\n",
            "## Struct\n\n```template\nstruct {name} {{ {fields} }}\n```\n\n",
            "### field\n| When | Template |\n|------|----------|\n| else | \"{name}: {type}\" |\n\n",
        ),
        CAPS,
    ]
    .concat());
    let point = Item::Struct {
        name: "Point".into(),
        visibility: Visibility::Private,
        attributes: Vec::new(),
        fields: vec![Field {
            name: "x".into(),
            ty: Type::Primitive(Primitive::I32),
            visibility: Visibility::Private,
            meta: Meta::new(),
        }],
        meta: Meta::new(),
    };
    let make = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Point".into()),
        body: vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Point".into(),
            fields: vec![FieldInit {
                name: "x".into(),
                value: Expr::IntLiteral("1".into()),
                meta: Meta::new(),
            }],
            meta: Meta::new(),
        }))],
        meta: Meta::new(),
    };
    let out = emit_file(vec![point, Item::Function(make)], &def);
    // The field-init annotates `x` with its declared type resolved from `Point`.
    assert!(out.contains("Point { x: i32 = 1 }"), "got: {out}");
}


use std::path::PathBuf;

/// Loads a shipped language definition from `../../lamina-defs/languages/`.
fn shipped(file: &str) -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // lamina (repo)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push(file);
    lamina_core::load_language_def(&path)
        .unwrap_or_else(|e| panic!("shipped def {file} should load: {e}"))
}

/// The layer-shaped AST for a single-use anonymous function stored in a struct
/// field: a hoisted `fn cb() { return x + 1; }` tagged `origin=anon_fn`, and a
/// `make` fn returning `Holder { cb: cb }`.
fn shipped_anon_fn_unit() -> Vec<Item> {
    let cb = func_meta(
        "cb",
        Type::Primitive(Primitive::I32),
        vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("x".into())),
            rhs: Box::new(Expr::IntLiteral("1".into())),
        }))],
        Meta::new().with("origin", "anon_fn"),
    );
    let make = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Holder".into()),
        body: vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Holder".into(),
            fields: vec![FieldInit {
                name: "cb".into(),
                value: Expr::Ref("cb".into()),
                meta: Meta::new(),
            }],
            meta: Meta::new(),
        }))],
        meta: Meta::new(),
    };
    vec![Item::Function(cb), Item::Function(make)]
}

#[test]
fn shipped_rust_reconstructs_anonymous_function_as_closure() {
    let out = emit_file(shipped_anon_fn_unit(), &shipped("rust.mdl"));
    // The single-use `cb` fnptr value inlines as a Rust closure `|| { … }`
    // inside the `Holder` initializer.
    assert!(
        out.contains("Holder { cb: || {"),
        "expected inlined Rust closure, got:\n{out}"
    );
    assert!(out.contains("x + 1"), "closure body inlined, got:\n{out}");
}

#[test]
fn shipped_typescript_reconstructs_anonymous_function_as_arrow() {
    let out = emit_file(shipped_anon_fn_unit(), &shipped("typescript.mdl"));
    // The single-use `cb` fnptr value inlines as a TS arrow function.
    assert!(
        out.contains("{ cb: () => {"),
        "expected inlined TS arrow function, got:\n{out}"
    );
    assert!(out.contains("x + 1"), "arrow body inlined, got:\n{out}");
}

#[test]
fn shipped_typescript_reconstructs_anonymous_class_object() {
    // A struct literal tagged origin=anon_class with a data field and a method
    // field (member=method) referencing a hoisted method fn.
    let m = func_meta(
        "m",
        Type::Primitive(Primitive::I32),
        vec![Statement::Return(Some(Expr::IntLiteral("7".into())))],
        Meta::new().with("origin", "anon_fn"),
    );
    let make = Function {
        name: "make".into(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Named("Obj".into()),
        body: vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Obj".into(),
            fields: vec![
                FieldInit {
                    name: "n".into(),
                    value: Expr::IntLiteral("1".into()),
                    meta: Meta::new(),
                },
                FieldInit {
                    name: "m".into(),
                    value: Expr::Ref("m".into()),
                    meta: Meta::new().with("member", "method"),
                },
            ],
            meta: Meta::new().with("origin", "anon_class"),
        }))],
        meta: Meta::new(),
    };
    let out = emit_file(vec![Item::Function(m), Item::Function(make)], &shipped("typescript.mdl"));
    // Reconstructed as an object literal (no type name), with the method field
    // inlined as an arrow function.
    assert!(out.contains("return { n: 1, m: () => {"), "got:\n{out}");
    assert!(out.contains("return 7;"), "method body inlined, got:\n{out}");
}


#[test]
fn escape_unknown_style_errors() {
    // A def that requests an unknown escape style fails at load OR emit; here
    // the style is unknown, so emitting a string errors (not silently wrong).
    let def = lang(&[
        concat!(
            "# Lamina Language Definition: badesc\n\n",
            "```lang-meta\nlamina-format: 0.0.0\ntarget: badesc\ntarget-version: test\n```\n\n",
            "## Function\n\n",
            "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
            "### statement\n```template\nreturn {value};\n```\n\n",
            "### expr\n",
            "| When           | Template |\n",
            "|----------------|----------|\n",
            "| expr is string | \"{escape(value, klingon)}\" |\n",
            "| else           | forbid |\n\n",
        ), CAPS].concat());
    let body = vec![Statement::Return(Some(Expr::StringLiteral("hi".into())))];
    let f = func_meta("f", Type::Primitive(Primitive::Void), body, Meta::new());
    let err = emit_err(vec![Item::Function(f)], &def);
    assert!(
        matches!(err, lamina_core::EmitError::UnknownSlot { .. }),
        "unknown escape style should error, got: {err:?}"
    );
}
