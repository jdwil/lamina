//! End-to-end tests for the shipped `go.mdl` language definition.
//!
//! These verify that the kernel imperative constructs map onto their native Go
//! forms: a function → a Go `func` (visibility spelled by identifier case, so
//! the `vis` slot emits no keyword), a `struct` → a Go value-type `struct`, a
//! payloadless `enum` → an idiomatic `iota` block, a payload-bearing `enum` →
//! the sealed-interface + variant-struct encoding, `if`/`for`/`switch` (Go's
//! single `for` hosts `while`, counted-`for`, and `foreach`), a `lambda` → a Go
//! function literal, a cast → a Go conversion `T(x)`, and type-level attributes
//! → generated methods (`equatable` → `Equal`, `displayable` → `String`
//! implementing `fmt.Stringer`). What Go genuinely lacks stays forbidden: the
//! `never` return type, an enum's type attributes, and the `**`/`//`/`>>>`
//! operators; the unary `~` remaps to Go's `^`.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `go.mdl` shipped in `lamina-defs`,
//! asserting the EXACT emitted string. The sibling `compile_check.rs` harness
//! feeds a representative emitted program to `gofmt -e` to mechanically confirm
//! the strings parse as valid Go.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Modifier, Param, Primitive,
    Statement, SwitchCase, Type, TypeAttribute, UnaryOp, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn go() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("go.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped go.mdl should load: {e}"))
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
fn shipped_go_def_loads() {
    let _ = go();
}

// ---- fn: a Go function definition -----------------------------------------

#[test]
fn simple_fn_renders_as_go_func() {
    let f = func(
        "Add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Add(a int32, b int32) int32 {\n    return a + b\n}");
}

#[test]
fn void_fn_omits_return_type() {
    let f = func("Noop", vec![], Type::Primitive(Primitive::Void), vec![]);
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Noop() {\n    \n}");
}

#[test]
fn visibility_is_a_no_op_keyword() {
    // Go spells visibility by identifier case, not a keyword. A `private`
    // function therefore emits NO keyword — the identifier the author chose is
    // preserved verbatim (a documented no-op). No visibility is forbidden.
    let f = Item::Function(Function {
        name: "helper".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![],
        meta: Meta::new(),
    });
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func helper() {\n    \n}");
}

#[test]
fn protected_visibility_is_not_forbidden() {
    // Unlike keyword-based targets, Go expresses every visibility by naming
    // convention, so `protected` is a clean no-op rather than a forbidden
    // construct.
    let f = Item::Function(Function {
        name: "Mid".to_string(),
        visibility: Visibility::Protected,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![],
        meta: Meta::new(),
    });
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Mid() {\n    \n}");
}

#[test]
fn never_return_is_forbidden() {
    let f = func("Loop", vec![], Type::Primitive(Primitive::Never), vec![]);
    let err = emit_err(vec![f], &go());
    assert!(
        err.to_lowercase().contains("forbid") || err.to_lowercase().contains("never"),
        "{err}"
    );
}

// ---- struct: a Go value type ----------------------------------------------

#[test]
fn simple_struct_renders_fields() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("X", i32t()), field("Y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &go());
    assert_eq!(out, "type Point struct {\n    X int32\n    Y int32\n}");
}

#[test]
fn struct_equatable_and_displayable_generate_methods() {
    // equatable -> a field-wise Equal method; displayable -> a fmt.Stringer
    // String method; copyable is inherent (value type) and emits nothing.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("X", i32t()), field("Y", i32t())],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Displayable,
            TypeAttribute::Copyable,
        ],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &go());
    assert_eq!(
        out,
        "type Point struct {\n    X int32\n    Y int32\n}\n\n\
         func (a Point) Equal(b Point) bool {\n    return a.X == b.X && a.Y == b.Y\n}\n\n\
         func (t Point) String() string {\n    return fmt.Sprintf(\"Point{X=%v Y=%v}\", t.X, t.Y)\n}"
    );
}

#[test]
fn struct_comparable_attribute_is_forbidden() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("X", i32t())],
        attributes: vec![TypeAttribute::Comparable],
        meta: Meta::new(),
    };
    let err = emit_err(vec![s], &go());
    assert!(
        err.contains("forbid") || err.to_lowercase().contains("forbidden"),
        "{err}"
    );
}

// ---- enum: iota block (payloadless) ---------------------------------------

#[test]
fn payloadless_enum_renders_as_iota_block() {
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "Red".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Green".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "Blue".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![e], &go());
    assert_eq!(
        out,
        "type Color int\n\nconst (\n    Red Color = iota\n    Green\n    Blue\n)"
    );
}

// ---- enum: sealed interface + variant structs (payload-bearing) -----------

#[test]
fn payload_enum_renders_as_sealed_interface() {
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
                payload: VariantPayload::Struct(vec![
                    field("W", i32t()),
                    field("H", i32t()),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![e], &go());
    assert_eq!(
        out,
        "type Shape interface {\n    isShape()\n}\n\n\
         type Empty struct{}\n\n\
         func (Empty) isShape() {}\n\n\
         type Circle struct {\n    F0 int32\n}\n\n\
         func (Circle) isShape() {}\n\n\
         type Rect struct {\n    W int32\n    H int32\n}\n\n\
         func (Rect) isShape() {}"
    );
}

#[test]
fn enum_attribute_is_forbidden() {
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Red".to_string(),
            payload: VariantPayload::None,
            meta: Meta::new(),
        }],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    let err = emit_err(vec![e], &go());
    assert!(
        err.contains("forbid") || err.to_lowercase().contains("forbidden"),
        "{err}"
    );
}

// ---- control flow ---------------------------------------------------------

#[test]
fn if_else_renders() {
    let f = func(
        "Sign",
        vec![param("n", i32t())],
        i32t(),
        vec![Statement::If {
            cond: binary(BinaryOp::Lt, r("n"), int("0")),
            then_block: vec![Statement::Return(Some(int("-1")))],
            else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
                int("1"),
            ))]))),
        }],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Sign(n int32) int32 {\n    if n < 0 {\n        return -1\n    } else {\n        return 1\n    }\n}"
    );
}

#[test]
fn while_renders_as_for_cond() {
    let f = func(
        "Spin",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::While {
            cond: Expr::BoolLiteral(true),
            body: vec![Statement::Break],
        }],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Spin() {\n    for true {\n        break\n    }\n}");
}

#[test]
fn counted_for_renders_as_go_for_header() {
    let f = func(
        "Count",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::For {
            init: Some(Box::new(Statement::Let {
                name: "i".to_string(),
                ty: None,
                value: Some(int("0")),
            })),
            cond: Some(binary(BinaryOp::Lt, r("i"), int("10"))),
            step: Some(Box::new(
                Statement::assign(r("i"), add(r("i"), int("1"))).expect("lvalue"),
            )),
            body: vec![Statement::Continue],
        }],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Count() {\n    for i := 0; i < 10; i += 1 {\n        continue\n    }\n}"
    );
}

#[test]
fn foreach_renders_for_range() {
    let f = func(
        "Each",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::ForEach {
            binding: "x".to_string(),
            iterable: r("xs"),
            body: vec![Statement::Expr(Expr::Call {
                callee: Box::new(r("use")),
                args: vec![r("x")],
            })],
        }],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Each() {\n    for x := range xs {\n        use(x)\n    }\n}"
    );
}

#[test]
fn switch_renders_with_default_no_fallthrough() {
    let f = func(
        "Classify",
        vec![param("n", i32t())],
        Type::Primitive(Primitive::Void),
        vec![Statement::Switch {
            scrutinee: r("n"),
            cases: vec![
                SwitchCase {
                    value: int("0"),
                    body: vec![Statement::Break],
                    meta: Meta::new(),
                },
                SwitchCase {
                    value: int("1"),
                    body: vec![Statement::Break],
                    meta: Meta::new(),
                },
            ],
            default: Some(vec![Statement::Break]),
        }],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Classify(n int32) {\n    switch n {\n        case 0:\n            break\n        case 1:\n            break\n        default:\n            break\n    }\n}"
    );
}

// ---- lambda: a Go function literal ----------------------------------------

#[test]
fn lambda_renders_as_func_literal() {
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let c = Item::Const {
        name: "Inc".to_string(),
        ty: named("Fn"),
        value: lam,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &go());
    assert_eq!(
        out,
        "const Inc Fn = func(x int32) {\n    return x + 1\n}"
    );
}

#[test]
fn lambda_with_return_type_annotates() {
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(r("x")))],
        meta: Meta::new(),
    };
    let c = Item::Const {
        name: "Id".to_string(),
        ty: named("Fn"),
        value: lam,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &go());
    assert_eq!(out, "const Id Fn = func(x int32) int32 {\n    return x\n}");
}

// ---- calls, operators, casts, literals, construction ----------------------

#[test]
fn call_and_operators_render() {
    let f = func(
        "Compute",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(binary(
            BinaryOp::Mul,
            add(r("a"), r("b")),
            r("b"),
        )))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Compute(a int32, b int32) int32 {\n    return (a + b) * b\n}"
    );
}

#[test]
fn cast_renders_as_conversion() {
    let f = func(
        "Widen",
        vec![param("x", i32t())],
        Type::Primitive(Primitive::I64),
        vec![Statement::Return(Some(Expr::Cast {
            value: Box::new(r("x")),
            ty: Type::Primitive(Primitive::I64),
        }))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Widen(x int32) int64 {\n    return int64(x)\n}");
}

#[test]
fn null_renders_as_nil() {
    let c = Item::Const {
        name: "Nothing".to_string(),
        ty: Type::Pointer(Box::new(i32t())),
        value: Expr::NullLiteral,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &go());
    assert_eq!(out, "const Nothing *int32 = nil");
}

#[test]
fn struct_lit_renders_keyed_composite() {
    let f = func(
        "Make",
        vec![],
        named("Point"),
        vec![Statement::Return(Some(Expr::StructLit {
            type_name: "Point".to_string(),
            fields: vec![
                FieldInit {
                    name: "X".to_string(),
                    value: int("1"),
                    meta: Meta::new(),
                },
                FieldInit {
                    name: "Y".to_string(),
                    value: int("2"),
                    meta: Meta::new(),
                },
            ],
            meta: Meta::new(),
        }))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(
        out,
        "func Make() Point {\n    return Point{X: 1, Y: 2}\n}"
    );
}

#[test]
fn array_type_renders_sized_and_unsized() {
    let f = func(
        "Nums",
        vec![],
        Type::Array {
            elem: Box::new(i32t()),
            len: Some("3".to_string()),
        },
        vec![Statement::Return(Some(Expr::ArrayLit {
            elems: vec![int("1"), int("2"), int("3")],
            meta: Meta::new(),
        }))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Nums() [3]int32 {\n    return {1, 2, 3}\n}");
}

// ---- typedef, const, use --------------------------------------------------

#[test]
fn typedef_and_use_render() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    let u = Item::Use {
        path: "fmt".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![td, u], &go());
    assert_eq!(out, "type Id int32\n\nimport \"fmt\"");
}

#[test]
fn aliased_use_renders() {
    let u = Item::Use {
        path: "math/rand".to_string(),
        items: vec![],
        alias: Some("rnd".to_string()),
        meta: Meta::new(),
    };
    let out = emit_ok(vec![u], &go());
    assert_eq!(out, "import rnd \"math/rand\"");
}

#[test]
fn const_renders() {
    let c = Item::Const {
        name: "Max".to_string(),
        ty: i32t(),
        value: int("100"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &go());
    assert_eq!(out, "const Max int32 = 100");
}

// ---- operators: forbids and the bitnot remap ------------------------------

#[test]
fn pow_operator_is_forbidden() {
    let f = func(
        "Sq",
        vec![param("x", i32t())],
        i32t(),
        vec![Statement::Return(Some(binary(BinaryOp::Pow, r("x"), int("2"))))],
    );
    let err = emit_err(vec![f], &go());
    assert!(
        err.to_lowercase().contains("forbid") || err.to_lowercase().contains("pow"),
        "{err}"
    );
}

#[test]
fn bitnot_remaps_to_caret() {
    // The kernel's unary bitwise-not spells `~`, which is not valid Go; Go uses
    // unary `^`. The operator table remaps it.
    let f = func(
        "Comp",
        vec![param("x", i32t())],
        i32t(),
        vec![Statement::Return(Some(Expr::Unary {
            op: UnaryOp::BitNot,
            operand: Box::new(r("x")),
        }))],
    );
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Comp(x int32) int32 {\n    return ^x\n}");
}

// ---- modifiers are dropped (Go has no matching keyword) -------------------

#[test]
fn async_modifier_is_dropped() {
    // Go has no `async`/`throws` keyword (goroutines are a call-site concern,
    // errors are values), so a modifier the kernel carries emits no text.
    let f = Item::Function(Function {
        name: "Fetch".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async, Modifier::Throws],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
        meta: Meta::new(),
    });
    let out = emit_ok(vec![f], &go());
    assert_eq!(out, "func Fetch() int32 {\n    return 0\n}");
}
