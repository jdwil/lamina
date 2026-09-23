//! End-to-end tests for the shipped `swift.mdl` language definition.
//!
//! These verify that the kernel imperative constructs map onto their native
//! Swift forms: a function (with Swift's *postfix* `async`/`throws` effect
//! specifiers) → a Swift `func`, a `struct` → a Swift value-type `struct`, an
//! `enum` with associated values → Swift's native tagged union, `if`/`while`/
//! `for`/`switch`/assignment → their Swift forms, a `lambda` → a Swift closure
//! (`{ (x) in … }`), a call → `f(…)`, operators → their Swift spellings, and
//! type-level attributes → protocol conformances (`Equatable`, `Comparable`,
//! `Hashable`, `CustomStringConvertible`). What Swift genuinely lacks stays
//! forbidden: the `protected` visibility tier, the `hasdefault` attribute, and
//! the `**`/`//`/`>>>`/unary-`+` operators.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `swift.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. The sibling
//! `compile_check.rs` harness feeds a representative emitted program to the real
//! `swiftc -parse` to mechanically confirm the strings are valid Swift.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, FieldInit, Function, Item, Meta, Modifier, Param, Primitive,
    Statement, SwitchCase, Type, TypeAttribute, UnaryOp, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn swift() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("swift.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped swift.mdl should load: {e}"))
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
fn shipped_swift_def_loads() {
    let _ = swift();
}

// ---- fn: a Swift function definition --------------------------------------

#[test]
fn simple_fn_renders_as_swift_func() {
    // public func add(a: Int32, b: Int32) -> Int32 {
    //     return a + b
    // }
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func add(a: Int32, b: Int32) -> Int32 {\n    return a + b\n}"
    );
}

#[test]
fn void_fn_omits_return_arrow() {
    let f = func("noop", vec![], Type::Primitive(Primitive::Void), vec![]);
    let out = emit_ok(vec![f], &swift());
    assert_eq!(out, "public func noop() {\n    \n}");
}

#[test]
fn async_throws_are_postfix() {
    // Swift places effect specifiers AFTER the parameter list:
    //   func fetch() async throws -> Int32
    let f = Item::Function(Function {
        name: "fetch".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async, Modifier::Throws],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
    meta: Meta::new(),
    });
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func fetch() async throws -> Int32 {\n    return 0\n}"
    );
}

// ---- struct: a Swift value type -------------------------------------------

#[test]
fn simple_struct_renders_stored_properties() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &swift());
    assert_eq!(
        out,
        "public struct Point {\n    public var x: Int32\n    public var y: Int32\n}"
    );
}

#[test]
fn struct_attributes_render_as_generated_extensions() {
    // equatable/hashable -> empty synthesized extensions; comparable/displayable
    // -> generated field-wise extensions; cloneable/copyable are inherent (value
    // type) and emit nothing. Extensions follow the declaration in attribute
    // order.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![
            TypeAttribute::Displayable,
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
            TypeAttribute::Cloneable,
            TypeAttribute::Copyable,
        ],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &swift());
    assert_eq!(
        out,
        "public struct Point {\n    public var x: Int32\n}\n\n\
         extension Point: CustomStringConvertible {\n    public var description: String {\n        return \"Point(x: \\(x))\"\n    }\n}\n\n\
         extension Point: Equatable {}\n\n\
         extension Point: Comparable {\n    public static func < (a: Point, b: Point) -> Bool {\n        if a.x != b.x { return a.x < b.x }\n        return false\n    }\n}\n\n\
         extension Point: Hashable {}"
    );
}

#[test]
fn struct_hasdefault_attribute_is_forbidden() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::HasDefault],
        meta: Meta::new(),
    };
    let err = emit_err(vec![s], &swift());
    assert!(err.contains("forbid") || err.to_lowercase().contains("forbidden"), "{err}");
}

// ---- enum: native tagged union with associated values ---------------------

#[test]
fn enum_with_associated_values_renders_natively() {
    // enum Shape {
    //     case empty
    //     case circle(Int32)
    //     case rect(width: Int32, height: Int32)
    // }
    let e = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "circle".to_string(),
                payload: VariantPayload::Tuple(vec![i32t()]),
                meta: Meta::new(),
            },
            Variant {
                name: "rect".to_string(),
                payload: VariantPayload::Struct(vec![
                    field("width", i32t()),
                    field("height", i32t()),
                ]),
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![e], &swift());
    assert_eq!(
        out,
        "public enum Shape {\n    case empty\n    case circle(Int32)\n    case rect(width: Int32, height: Int32)\n}"
    );
}

#[test]
fn enum_with_conformances() {
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![
            Variant {
                name: "red".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
            Variant {
                name: "green".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Hashable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![e], &swift());
    assert_eq!(
        out,
        "public enum Color {\n    case red\n    case green\n}\n\n\
         extension Color: Equatable {}\n\n\
         extension Color: Hashable {}"
    );
}

// ---- control flow ---------------------------------------------------------

#[test]
fn if_else_renders() {
    let f = func(
        "sign",
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
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func sign(n: Int32) -> Int32 {\n    if n < 0 {\n        return -1\n    } else {\n        return 1\n    }\n}"
    );
}

#[test]
fn while_renders() {
    let f = func(
        "spin",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::While {
            cond: Expr::BoolLiteral(true),
            body: vec![Statement::Break],
        }],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func spin() {\n    while true {\n        break\n    }\n}"
    );
}

#[test]
fn counted_for_desugars_to_while() {
    // A C-style counted `for (let i = 0; i < 10; i = i + 1)` lowers to a
    // brace-scoped `while` (Swift has no C-style for).
    let f = func(
        "count",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::For {
            init: Some(Box::new(Statement::Let {
                name: "i".to_string(),
                ty: Some(i32t()),
                value: Some(int("0")),
            })),
            cond: Some(binary(BinaryOp::Lt, r("i"), int("10"))),
            step: Some(Box::new(
                Statement::assign(r("i"), add(r("i"), int("1"))).expect("lvalue"),
            )),
            body: vec![Statement::Continue],
        }],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func count() {\n    do {\n        var i: Int32 = 0\n        while i < 10 {\n            continue\n            i += 1\n        }\n    }\n}"
    );
}

#[test]
fn foreach_renders_for_in() {
    let f = func(
        "each",
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
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func each() {\n    for x in xs {\n        use(x)\n    }\n}"
    );
}

#[test]
fn switch_renders_with_default() {
    let f = func(
        "classify",
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
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func classify(n: Int32) {\n    switch n {\n        case 0:\n            break\n        case 1:\n            break\n        default:\n            break\n    }\n}"
    );
}

// ---- lambda: a Swift closure ----------------------------------------------

#[test]
fn lambda_renders_as_closure() {
    // A single-statement lambda `x -> x + 1` renders as Swift's closure
    //   { (x: Int32) in return x + 1 }
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let c = Item::Const {
        name: "inc".to_string(),
        ty: named("Closure"),
        value: lam,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &swift());
    assert_eq!(
        out,
        "public let inc: Closure = { (x: Int32) in\n    return x + 1\n}"
    );
}

#[test]
fn lambda_with_return_type_annotates_before_in() {
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(r("x")))],
        meta: Meta::new(),
    };
    let c = Item::Const {
        name: "id".to_string(),
        ty: named("Closure"),
        value: lam,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![c], &swift());
    assert_eq!(
        out,
        "public let id: Closure = { (x: Int32) -> Int32 in\n    return x\n}"
    );
}

// ---- calls, operators, casts, literals ------------------------------------

#[test]
fn call_and_operators_render() {
    let f = func(
        "compute",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(binary(
            BinaryOp::Mul,
            add(r("a"), r("b")),
            r("b"),
        )))],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func compute(a: Int32, b: Int32) -> Int32 {\n    return (a + b) * b\n}"
    );
}

#[test]
fn null_follows_ptr_and_is_forbidden() {
    // In the kernel, a `null` literal FOLLOWS the `ptr` primitive: a target that
    // forbids `ptr` also forbids `null`. Swift forbids `ptr` (no raw-pointer
    // source primitive), so a bare kernel `null` is a forbidden construct. Swift
    // *does* have `nil`, but only as the absence value of an `Optional<T>` — a
    // library/reference concept, not the kernel's pointer-null. The `### expr`
    // `null` row therefore never fires; a layer that wants a Swift `Optional`
    // lowers `nil` via the raw escape hatch.
    let c = Item::Const {
        name: "nothing".to_string(),
        ty: named("Optional"),
        value: Expr::NullLiteral,
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    let err = emit_err(vec![c], &swift());
    assert!(err.to_lowercase().contains("forbid") || err.contains("null"), "{err}");
}

#[test]
fn cast_renders_forced_downcast() {
    let f = func(
        "widen",
        vec![param("x", i32t())],
        Type::Primitive(Primitive::I64),
        vec![Statement::Return(Some(Expr::Cast {
            value: Box::new(r("x")),
            ty: Type::Primitive(Primitive::I64),
        }))],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func widen(x: Int32) -> Int64 {\n    return x as! Int64\n}"
    );
}

#[test]
fn struct_lit_renders_memberwise_init() {
    let f = func(
        "make",
        vec![],
        named("Point"),
        vec![Statement::Return(Some(Expr::StructLit {
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
        }))],
    );
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func make() -> Point {\n    return Point(x: 1, y: 2)\n}"
    );
}

#[test]
fn array_literal_and_type_render() {
    let f = func(
        "nums",
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
    let out = emit_ok(vec![f], &swift());
    assert_eq!(
        out,
        "public func nums() -> [Int32] {\n    return [1, 2, 3]\n}"
    );
}

// ---- forbidden constructs (genuine Swift limitations) ---------------------

#[test]
fn protected_visibility_is_forbidden() {
    let f = Item::Function(Function {
        name: "hidden".to_string(),
        visibility: Visibility::Protected,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![],
        meta: Meta::new(),
    });
    let err = emit_err(vec![f], &swift());
    assert!(err.contains("forbid") || err.to_lowercase().contains("forbidden"), "{err}");
}

#[test]
fn pow_operator_is_forbidden() {
    let f = func(
        "sq",
        vec![param("x", i32t())],
        i32t(),
        vec![Statement::Return(Some(binary(BinaryOp::Pow, r("x"), int("2"))))],
    );
    let err = emit_err(vec![f], &swift());
    assert!(err.to_lowercase().contains("forbid") || err.to_lowercase().contains("pow"), "{err}");
}

#[test]
fn unary_pos_is_forbidden() {
    let f = func(
        "p",
        vec![param("x", i32t())],
        i32t(),
        vec![Statement::Return(Some(Expr::Unary {
            op: UnaryOp::Pos,
            operand: Box::new(r("x")),
        }))],
    );
    let err = emit_err(vec![f], &swift());
    assert!(err.to_lowercase().contains("forbid") || err.to_lowercase().contains("pos"), "{err}");
}

#[test]
fn typedef_and_use_render() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    let u = Item::Use {
        path: "Foundation".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    let out = emit_ok(vec![td, u], &swift());
    assert_eq!(out, "typealias Id = Int32\n\nimport Foundation");
}
