//! End-to-end tests for the shipped `kotlin.mdl` language definition.
//!
//! These verify that the imperative constructs map onto their native Kotlin
//! forms (a `fun` function, a primary-constructor `class`, a `when` for a
//! `switch`, `if`/`while`/`for`, a lambda `{ x -> … }`, a call, and the operator
//! spellings), and pin Kotlin's richer realizations: a struct requesting a
//! behavioral attribute Kotlin's `data class` provides (equality, hashing,
//! display, copy) is declared a `data class`; a payload-bearing `enum` is
//! realized as a `sealed class` hierarchy (a unit variant → `data object`, a
//! tuple/struct variant → nested `data class`). What Kotlin genuinely cannot
//! host at the operator level (the bitwise/`pow`/`floordiv`/`ushr` operators,
//! which Kotlin spells as infix functions, and unary `+`) stays forbidden.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `kotlin.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. The
//! `kotlin_output_compiles_with_kotlinc` test in `compile_check.rs` is the
//! mechanical backstop that feeds a representative program to the real compiler.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, FieldInit, Function, Item, Meta, Modifier, Param, Primitive,
    Statement, SwitchCase, Type, TypeAttribute, UnaryOp, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn kotlin() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("kotlin.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped kotlin.mdl should load: {e}"))
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
fn shipped_kotlin_def_loads() {
    let _ = kotlin();
}

// ---- fun: a Kotlin function definition -----------------------------------

#[test]
fn simple_fn_renders_as_kotlin_fun() {
    // public fun add(a: Int, b: Int): Int {
    //     return a + b
    // }
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun add(a: Int, b: Int): Int {\n    return a + b\n}"
    );
}

#[test]
fn void_fn_renders_unit_return() {
    // public fun noop(): Unit {
    //     return
    // }
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun noop(): Unit {\n    return\n}"
    );
}

#[test]
fn suspend_fn_renders_modifier() {
    // A callable with the async modifier is a Kotlin `suspend fun`.
    let f = Item::Function(Function {
        name: "fetch".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![Modifier::Async],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public suspend fun fetch(): Int {\n    return 0\n}"
    );
}

// ---- class: a Kotlin primary-constructor class ---------------------------

#[test]
fn struct_renders_as_kotlin_class() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &kotlin()),
        "public class Point(\n    val x: Int,\n    val y: Int,\n)"
    );
}

#[test]
fn struct_with_equatable_becomes_data_class() {
    // Kotlin's `data class` auto-provides equals/hashCode/toString/copy, so a
    // struct requesting a behavioral attribute it realizes is declared `data`.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Hashable,
            TypeAttribute::Displayable,
            TypeAttribute::Cloneable,
        ],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &kotlin()),
        "public data class Point(\n    val x: Int,\n    val y: Int,\n)"
    );
}

#[test]
fn struct_copyable_is_realized_by_data_class() {
    // `copyable` is realized by a data class's `copy(...)`.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Copyable],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &kotlin()),
        "public data class P(\n    val x: Int,\n)"
    );
}

#[test]
fn struct_comparable_is_forbidden() {
    // Kotlin realizes a total order via `Comparable<T>`, not a data-class-like
    // keyword, so `comparable` has no single realization here.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Comparable],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![s], &kotlin()).contains("forbid"),
        "comparable has no data-class realization in Kotlin"
    );
}

// ---- enum: a plain Kotlin enum class -------------------------------------

#[test]
fn plain_enum_renders_as_enum_class() {
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
    assert_eq!(
        emit_ok(vec![e], &kotlin()),
        "public enum class Color {\n    Red,\n    Green,\n    Blue,\n}"
    );
}

// ---- enum: a payload-bearing enum -> sealed class hierarchy --------------

#[test]
fn enum_with_tuple_payload_renders_as_sealed_class() {
    // A payload-bearing enum becomes a `sealed class` with each variant a nested
    // subclass: a unit variant → `data object`, a tuple variant → `data class`
    // of positionally-named components.
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
    let expected = "public sealed class Shape {\n    \
        data object Empty : Shape()\n    \
        data class Circle(val c0: Int) : Shape()\n    \
        data class Rect(val c0: Int, val c1: Int) : Shape()\n}";
    assert_eq!(emit_ok(vec![e], &kotlin()), expected);
}

#[test]
fn enum_with_struct_payload_renders_named_data_class() {
    // A struct-style payload variant is a nested `data class` of its named
    // fields extending the sealed parent.
    let e = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Rect".to_string(),
            payload: VariantPayload::Struct(vec![field("w", i32t()), field("h", i32t())]),
            meta: Meta::new(),
        }],
        attributes: vec![],
        meta: Meta::new(),
    };
    let expected = "public sealed class Shape {\n    \
        data class Rect(val w: Int, val h: Int) : Shape()\n}";
    assert_eq!(emit_ok(vec![e], &kotlin()), expected);
}

// ---- if / while / for: native Kotlin control flow ------------------------

#[test]
fn if_else_renders_directly() {
    let stmt = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("0")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun f(n: Int): Int {\n    if (n == 0) {\n        return 1\n    } else {\n        return 2\n    }\n}"
    );
}

#[test]
fn while_renders_directly_with_compound_assign() {
    // `i = i + 1` is recognized as `i += 1`.
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func("loopf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun loopf(): Unit {\n    while (i < 10) {\n        i += 1\n    }\n}"
    );
}

#[test]
fn counted_for_desugars_to_while() {
    // Kotlin has no C-style counted `for`, so it is desugared to a block-scoped
    // `while` (init before, step at the end of each iteration).
    let init = Statement::Let {
        name: "i".to_string(),
        ty: Some(i32t()),
        value: Some(int("0")),
    };
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let body = Statement::assign(r("sum"), add(r("sum"), r("i"))).unwrap();
    let stmt = Statement::For {
        init: Some(Box::new(init)),
        cond: Some(binary(BinaryOp::Lt, r("i"), int("10"))),
        step: Some(Box::new(step)),
        body: vec![body],
    };
    let f = func("sumf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun sumf(): Unit {\n    {\n        val i: Int = 0\n        while (i < 10) {\n            sum += i\n            i += 1\n        }\n    }\n}"
    );
}

#[test]
fn foreach_renders_native_for_in() {
    // An iterator loop maps to Kotlin's native `for (x in iter)`.
    let stmt = Statement::ForEach {
        binding: "x".to_string(),
        iterable: r("xs"),
        body: vec![Statement::Expr(Expr::Call {
            callee: Box::new(r("use")),
            args: vec![r("x")],
        })],
    };
    let f = func("iter", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun iter(): Unit {\n    for (x in xs) {\n        use(x)\n    }\n}"
    );
}

// ---- switch: a Kotlin when expression ------------------------------------

#[test]
fn switch_renders_as_when() {
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
    let expected = "public fun f(n: Int): Int {\n    when (n) {\n        0 -> {\n            return 1\n        }\n        1 -> {\n            return 2\n        }\n        else -> {\n            return 0\n        }\n    }\n}";
    assert_eq!(emit_ok(vec![f], &kotlin()), expected);
}

// ---- call / operators -----------------------------------------------------

#[test]
fn call_renders_as_kotlin_call() {
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun g(): Int {\n    return f(x, (y + 1))\n}"
    );
}

#[test]
fn operators_use_kotlin_spellings() {
    // return a != b && !c   (compound operands parenthesized by the engine)
    let expr = binary(
        BinaryOp::And,
        binary(BinaryOp::Ne, r("a"), r("b")),
        Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(r("c")),
        },
    );
    let f = func(
        "test",
        vec![],
        Type::Primitive(Primitive::Bool),
        vec![Statement::Return(Some(expr))],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun test(): Boolean {\n    return (a != b) && (!c)\n}"
    );
}

#[test]
fn bitwise_and_pow_operators_are_forbidden() {
    // Kotlin has no symbolic bitwise/pow operators (they are infix functions).
    for op in [
        BinaryOp::Pow,
        BinaryOp::FloorDiv,
        BinaryOp::Shl,
        BinaryOp::BitAnd,
        BinaryOp::BitOr,
        BinaryOp::UShr,
    ] {
        let expr = binary(op, r("a"), r("b"));
        let f = func("p", vec![], i32t(), vec![Statement::Return(Some(expr))]);
        assert!(
            emit_err(vec![f], &kotlin()).contains("forbid"),
            "{op:?} should be forbidden in Kotlin (it is an infix function)"
        );
    }
}

// ---- lambda: a Kotlin lambda `{ x -> … }` --------------------------------

#[test]
fn lambda_renders_as_kotlin_lambda() {
    // val g = { x: Int -> return x + 1 }
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let g = Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lam),
    };
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), vec![g]);
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun mk(): Unit {\n    val g = { x: Int -> return x + 1 }\n}"
    );
}

// ---- cast / struct literal / array ---------------------------------------

#[test]
fn cast_renders_as_kotlin_as() {
    let cast = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::I64),
    };
    let f = func(
        "widen",
        vec![],
        Type::Primitive(Primitive::I64),
        vec![Statement::Return(Some(cast))],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun widen(): Long {\n    return x as Long\n}"
    );
}

#[test]
fn struct_literal_renders_as_named_constructor_call() {
    // Point(x = 1, y = 2)
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
    let f = func(
        "mk",
        vec![],
        named("Point"),
        vec![Statement::Return(Some(lit))],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun mk(): Point {\n    return Point(x = 1, y = 2)\n}"
    );
}

#[test]
fn array_literal_renders_as_arrayof() {
    let arr = Expr::ArrayLit {
        elems: vec![int("10"), int("20"), int("30")],
        meta: Meta::new(),
    };
    let f = func(
        "arrf",
        vec![],
        Type::Array {
            elem: Box::new(i32t()),
            len: None,
        },
        vec![Statement::Return(Some(arr))],
    );
    assert_eq!(
        emit_ok(vec![f], &kotlin()),
        "public fun arrf(): Array<Int> {\n    return arrayOf(10, 20, 30)\n}"
    );
}

// ---- items: typedef / const / use ----------------------------------------

#[test]
fn typedef_const_and_use_render() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![td], &kotlin()), "typealias Id = Int");

    let konst = Item::Const {
        name: "answer".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![konst], &kotlin()),
        "public val answer: Int = 42"
    );

    let u = Item::Use {
        path: "kotlin.math.sqrt".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![u], &kotlin()), "import kotlin.math.sqrt");
}

// ---- multiple items: blank-line separated --------------------------------

#[test]
fn multiple_items_are_blank_line_separated() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    let f = func("f", vec![], i32t(), vec![Statement::Return(Some(int("0")))]);
    assert_eq!(
        emit_ok(vec![s, f], &kotlin()),
        "public class P(\n    val x: Int,\n)\n\npublic fun f(): Int {\n    return 0\n}"
    );
}
