//! End-to-end tests for the shipped `javascript.mdl` language definition.
//!
//! JavaScript is TypeScript-minus-types: the control-flow, expression,
//! operator, and lambda mappings are identical, but every type-annotation
//! position is dropped (no `: T` on params/fields/returns, no `interface`).
//! These tests pin the exact emitted string for the representative constructs:
//!
//! * an untyped `function` with an indented body,
//! * a `struct` → a `class` with a field-assigning constructor, plus the
//!   generated `equatable` (`<Name>_eq`) and `displayable` (`<Name>_toString`)
//!   helpers,
//! * `if`/`while`/`for`/`switch`/`foreach` control flow,
//! * a native arrow-function lambda,
//! * calls, operators (`**`, `>>>`, compound-assignment idiom), and
//! * a plain enum → a frozen tag object, a payload enum → tagged constructors.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `javascript.mdl` document shipped
//! in `lamina-defs`, asserting the EXACT emitted string. Every asserted output
//! was hand-verified to be valid, idiomatic JavaScript; the harness in
//! `compile_check.rs` additionally runs a representative program through
//! `node --check`.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Modifier, Param, Primitive,
    Statement, SwitchCase, Type, TypeAttribute, UseItem, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn javascript() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("javascript.mdl");
    load_language_def(&path)
        .unwrap_or_else(|e| panic!("shipped javascript.mdl should load: {e}"))
}

fn emit_ok(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_err(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang)
        .expect_err("expected a forbidden-construct error")
        .to_string()
}

fn i32t() -> Type {
    Type::Primitive(Primitive::I32)
}

fn named(n: &str) -> Type {
    Type::Named(n.to_string())
}

fn int(v: &str) -> Expr {
    Expr::IntLiteral(v.to_string())
}

fn r(name: &str) -> Expr {
    Expr::Ref(name.to_string())
}

fn param(name: &str, ty: Type) -> Param {
    Param {
        name: name.to_string(),
        ty,
        meta: Meta::new(),
    }
}

fn field(name: &str, ty: Type) -> Field {
    Field {
        name: name.to_string(),
        ty,
        visibility: Visibility::Public,
        meta: Meta::new(),
    }
}

fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn add(lhs: Expr, rhs: Expr) -> Expr {
    binary(BinaryOp::Add, lhs, rhs)
}

fn func(name: &str, params: Vec<Param>, ret: Type, body: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params,
        return_type: ret,
        body,
        meta: Meta::new(),
    })
}

// ---- the shipped def loads (validates every slot/fact/matrix) -------------

#[test]
fn shipped_javascript_def_loads() {
    let _ = javascript();
}

// ---- fn: an untyped function with an indented body ------------------------

#[test]
fn simple_fn_renders_untyped_with_indented_body() {
    // export function add(a, b) {
    //     return a + b;
    // }
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function add(a, b) {\n    return a + b;\n}"
    );
}

#[test]
fn void_fn_omits_return_and_annotation() {
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function noop() {\n    return;\n}"
    );
}

#[test]
fn async_fn_renders_async_keyword() {
    let function = Function {
        name: "fetchIt".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![Item::Function(function)], &javascript()),
        "export async function fetchIt() {\n    return 0;\n}"
    );
}

// ---- nested indentation ---------------------------------------------------

#[test]
fn nested_blocks_indent_correctly() {
    // function f(n) {
    //     while (n > 0) {
    //         if (n === 1) {
    //             return 1;
    //         }
    //         n -= 1;
    //     }
    //     return 0;
    // }
    let inner_if = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("1")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: None,
    };
    let dec = Statement::assign(r("n"), binary(BinaryOp::Sub, r("n"), int("1"))).unwrap();
    let while_stmt = Statement::While {
        cond: binary(BinaryOp::Gt, r("n"), int("0")),
        body: vec![inner_if, dec],
    };
    let f = func(
        "f",
        vec![param("n", i32t())],
        i32t(),
        vec![while_stmt, Statement::Return(Some(int("0")))],
    );
    let expected = "export function f(n) {\n    while (n > 0) {\n        if (n === 1) {\n            return 1;\n        }\n        n -= 1;\n    }\n    return 0;\n}";
    assert_eq!(emit_ok(vec![f], &javascript()), expected);
}

// ---- if / else ------------------------------------------------------------

#[test]
fn if_else_renders_braced_blocks() {
    let stmt = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("0")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![stmt]);
    let expected = "export function f(n) {\n    if (n === 0) {\n        return 1;\n    } else {\n        return 2;\n    }\n}";
    assert_eq!(emit_ok(vec![f], &javascript()), expected);
}

// ---- while ----------------------------------------------------------------

#[test]
fn while_renders_braced_block() {
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func("loopf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function loopf() {\n    while (i < 10) {\n        i += 1;\n    }\n}"
    );
}

// ---- counted for ----------------------------------------------------------

#[test]
fn counted_for_renders_c_style_header() {
    // for (let i = 0; i < 10; i = i + 1) { total += i; }
    let init = Statement::Let {
        name: "i".to_string(),
        ty: None,
        value: Some(int("0")),
    };
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let body_step = Statement::assign(r("total"), add(r("total"), r("i"))).unwrap();
    let stmt = Statement::For {
        init: Some(Box::new(init)),
        cond: Some(binary(BinaryOp::Lt, r("i"), int("10"))),
        step: Some(Box::new(step)),
        body: vec![body_step],
    };
    let f = func("f", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function f() {\n    for (let i = 0; i < 10; i += 1) {\n        total += i;\n    }\n}"
    );
}

// ---- foreach --------------------------------------------------------------

#[test]
fn foreach_renders_for_of() {
    let step = Statement::assign(r("total"), add(r("total"), r("x"))).unwrap();
    let stmt = Statement::ForEach {
        binding: "x".to_string(),
        iterable: r("xs"),
        body: vec![step],
    };
    let f = func("sumf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function sumf() {\n    for (const x of xs) {\n        total += x;\n    }\n}"
    );
}

// ---- switch ---------------------------------------------------------------

#[test]
fn switch_renders_native_switch() {
    let sw = Statement::Switch {
        scrutinee: r("n"),
        cases: vec![
            SwitchCase {
                value: int("0"),
                body: vec![Statement::Return(Some(int("1")))],
                meta: Meta::new(),
            },
            SwitchCase {
                value: int("1"),
                body: vec![Statement::Return(Some(int("2")))],
                meta: Meta::new(),
            },
        ],
        default: Some(vec![Statement::Return(Some(int("0")))]),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![sw]);
    let expected = "export function f(n) {\n    switch (n) {\n        case 0: {\n            return 1;\n        }\n        case 1: {\n            return 2;\n        }\n        default: {\n            return 0;\n        }\n    }\n}";
    assert_eq!(emit_ok(vec![f], &javascript()), expected);
}

// ---- let ------------------------------------------------------------------

#[test]
fn let_binding_is_untyped() {
    let untyped = Statement::Let {
        name: "x".to_string(),
        ty: None,
        value: Some(int("5")),
    };
    let f = func("g", vec![], Type::Primitive(Primitive::Void), vec![untyped]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function g() {\n    let x = 5;\n}"
    );

    // Even a TYPED kernel let drops its annotation in JavaScript.
    let typed = Statement::Let {
        name: "y".to_string(),
        ty: Some(i32t()),
        value: Some(int("7")),
    };
    let f = func("h", vec![], Type::Primitive(Primitive::Void), vec![typed]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function h() {\n    let y = 7;\n}"
    );
}

// ---- struct -> class ------------------------------------------------------

#[test]
fn struct_renders_as_class_with_constructor() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &javascript()),
        "export class Point {\n    constructor(x, y) {\n        this.x = x;\n        this.y = y;\n    }\n}"
    );
}

// ---- struct attributes: equatable / displayable ---------------------------

#[test]
fn equatable_struct_generates_eq_helper() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &javascript());
    assert_eq!(
        out,
        "function Point_eq(a, b) {\n    return a.x === b.x && a.y === b.y;\n}\n\nexport class Point {\n    constructor(x, y) {\n        this.x = x;\n        this.y = y;\n    }\n}"
    );
}

#[test]
fn displayable_struct_generates_tostring_helper() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &javascript());
    assert_eq!(
        out,
        "function Point_toString(a) {\n    return `Point { x = ${a.x}, y = ${a.y} }`;\n}\n\nexport class Point {\n    constructor(x, y) {\n        this.x = x;\n        this.y = y;\n    }\n}"
    );
}

#[test]
fn cloneable_attribute_is_forbidden() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Cloneable],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![s], &javascript()).contains("forbid"),
        "cloneable should be a forbidden construct in JavaScript"
    );
}

// ---- enum -----------------------------------------------------------------

#[test]
fn plain_enum_renders_as_frozen_object() {
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "RED".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "GREEN".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "BLUE".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![e], &javascript()),
        "export const Color = Object.freeze({\n    RED: \"RED\",\n    GREEN: \"GREEN\",\n    BLUE: \"BLUE\",\n});"
    );
}

#[test]
fn payload_enum_renders_as_tagged_constructors() {
    let e = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Circle".to_string(),
                payload: VariantPayload::Tuple(vec![i32t()]),
                meta: Meta::new(),
            },
            Variant {
                name: "Rect".to_string(),
                payload: VariantPayload::Tuple(vec![i32t(), i32t()]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![e], &javascript());
    assert_eq!(
        out,
        "export const Shape = Object.freeze({\n    Empty: { tag: \"Empty\" },\n    Circle: (a0) => ({ tag: \"Circle\", values: [a0] }),\n    Rect: (a0, a1) => ({ tag: \"Rect\", values: [a0, a1] }),\n});"
    );
}

// ---- function call --------------------------------------------------------

#[test]
fn call_renders_as_js_call() {
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function g() {\n    return f(x, (y + 1));\n}"
    );
}

// ---- operators ------------------------------------------------------------

#[test]
fn exponent_and_unsigned_shift_map_canonically() {
    let pow = binary(BinaryOp::Pow, r("a"), r("b"));
    let f = func("p", vec![], i32t(), vec![Statement::Return(Some(pow))]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function p() {\n    return a ** b;\n}"
    );

    let ushr = binary(BinaryOp::UShr, r("a"), r("b"));
    let f = func("s", vec![], i32t(), vec![Statement::Return(Some(ushr))]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function s() {\n    return a >>> b;\n}"
    );
}

#[test]
fn floor_division_is_forbidden() {
    let expr = binary(BinaryOp::FloorDiv, r("a"), r("b"));
    let f = func("d", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert!(
        emit_err(vec![f], &javascript()).contains("forbid"),
        "floordiv should be forbidden in JavaScript (no `//` operator)"
    );
}

// ---- lambda -> arrow function ---------------------------------------------

#[test]
fn lambda_renders_as_arrow_function() {
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function mk() {\n    let g = (x) => {\n        return x + 1;\n    };\n}"
    );
}

// ---- struct literal -> object literal -------------------------------------

#[test]
fn struct_literal_renders_as_object_literal() {
    let lit = Expr::StructLit {
        type_name: "Point".to_string(),
        fields: vec![
            FieldInit {
                name: "x".to_string(),
                value: int("1"),
                meta: Meta::new(),
            },
            FieldInit {
                name: "y".to_string(),
                value: int("2"),
                meta: Meta::new(),
            },
        ],
        meta: Meta::new(),
    };
    let f = func("mk", vec![], named("Point"), vec![Statement::Return(Some(lit))]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function mk() {\n    return { x: 1, y: 2 };\n}"
    );
}

// ---- array literal + index ------------------------------------------------

#[test]
fn array_literal_and_index_render() {
    let arr = Expr::ArrayLit {
        elems: vec![int("10"), int("20"), int("30")],
        meta: Meta::new(),
    };
    let idx = Expr::Index {
        obj: Box::new(r("arr")),
        index: Box::new(int("1")),
    };
    let f = func(
        "arrf",
        vec![],
        i32t(),
        vec![Statement::Expr(arr), Statement::Return(Some(idx))],
    );
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function arrf() {\n    [10, 20, 30];\n    return arr[1];\n}"
    );
}

// ---- cast passes value through --------------------------------------------

#[test]
fn cast_passes_value_through() {
    // JavaScript has no static cast; the value is emitted unchanged.
    let cast = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::I64),
    };
    let f = func("c", vec![], i32t(), vec![Statement::Return(Some(cast))]);
    assert_eq!(
        emit_ok(vec![f], &javascript()),
        "export function c() {\n    return x;\n}"
    );
}

// ---- typedef forbidden ----------------------------------------------------

#[test]
fn typedef_is_forbidden() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![td], &javascript()).contains("forbid"),
        "a type alias has no runtime form in JavaScript and must be forbidden"
    );
}

// ---- const + use ----------------------------------------------------------

#[test]
fn const_and_use_render() {
    let konst = Item::Const {
        name: "ANSWER".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![konst], &javascript()),
        "export const ANSWER = 42;"
    );

    let bare = Item::Use {
        path: "fs".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![bare], &javascript()), "import fs;");

    let aliased = Item::Use {
        path: "path".to_string(),
        items: vec![],
        alias: Some("p".to_string()),
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![aliased], &javascript()),
        "import * as p from path;"
    );

    let selective = Item::Use {
        path: "util".to_string(),
        items: vec![
            UseItem {
                name: "format".to_string(),
                alias: None,
                meta: Meta::new(),
            },
            UseItem {
                name: "inspect".to_string(),
                alias: Some("show".to_string()),
                meta: Meta::new(),
            },
        ],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![selective], &javascript()),
        "import { format, inspect as show } from util;"
    );
}

// ---- raw item passes through ----------------------------------------------

#[test]
fn raw_item_passes_through() {
    let raw = Item::Raw {
        code: "// hand-written\nconsole.log('hi');".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![raw], &javascript()),
        "// hand-written\nconsole.log('hi');"
    );
}
