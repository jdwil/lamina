//! End-to-end tests for the shipped `java.mdl` language definition.
//!
//! Java is the class-only Tier 3 target: it has NO free functions — every
//! callable lives in a class. This def emits each top-level `fn` as a
//! `public static` method (the honest Java spelling of a free function), a
//! `struct` as a `record` (when it requests a record-realizable attribute) or a
//! plain `class` with a canonical constructor, a payloadless `enum` as a native
//! Java `enum`, and a payload-bearing `enum` as a `sealed interface` permitting
//! one `record` per variant (the idiomatic Java 17+ discriminated union).
//! `if`/`while`/`for` (C-style)/`foreach` (enhanced-for)/`switch` and lambdas
//! (`(x) -> { … }`) map to native forms. What Java genuinely cannot host stays
//! forbidden: the `**`/`//` operators (library calls in Java), a `typedef`
//! (Java has no transparent alias), an unsigned `u64`/`usize`/`u128` primitive
//! (no signed home), an fnptr *type* (no first-class function type).
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `java.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. The
//! `java_output_compiles_with_javac` test in `compile_check.rs` is the
//! mechanical backstop that wraps a representative program in a class and feeds
//! it to the real `javac`.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Param, Primitive,
    Statement, SwitchCase, Type, TypeAttribute, UnaryOp, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn java() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("java.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped java.mdl should load: {e}"))
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
fn shipped_java_def_loads() {
    let _ = java();
}

// ---- fn: a public static method (the no-free-functions idiom) ------------

#[test]
fn simple_fn_renders_as_static_method() {
    // public static int add(int a, int b) {
    //     return a + b;
    // }
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static int add(int a, int b) {\n    return a + b;\n}"
    );
}

#[test]
fn void_fn_renders_void_return() {
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static void noop() {\n    return;\n}"
    );
}

#[test]
fn private_fn_renders_private_static() {
    let f = Item::Function(Function {
        name: "helper".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &java()),
        "private static int helper() {\n    return 0;\n}"
    );
}

// ---- struct: a plain class (no attributes) -------------------------------

#[test]
fn struct_without_attributes_renders_as_class() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &java()),
        "public class Point {\n    public final int x;\n    public final int y;\n\n    \
         public Point(int x, int y) {\n        this.x = x;\n        this.y = y;\n    }\n}"
    );
}

// ---- struct: a record (record-realizable attributes) ---------------------

#[test]
fn struct_with_equatable_becomes_record() {
    // A Java `record` auto-provides equals/hashCode/toString, so a struct
    // requesting those behaviors is declared a `record`.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![
            TypeAttribute::Equatable,
            TypeAttribute::Hashable,
            TypeAttribute::Displayable,
        ],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &java()),
        "public record Point(int x, int y) {}"
    );
}

#[test]
fn struct_comparable_is_forbidden() {
    // A Java record does not auto-provide Comparable; it needs a hand-written
    // compareTo, so `comparable` has no record realization here.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Comparable],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![s], &java()).contains("forbid"),
        "comparable has no record realization in Java"
    );
}

// ---- enum: a native Java enum --------------------------------------------

#[test]
fn plain_enum_renders_as_java_enum() {
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
        emit_ok(vec![e], &java()),
        "public enum Color {\n    Red,\n    Green,\n    Blue\n}"
    );
}

// ---- enum: a payload-bearing enum -> sealed interface + records ----------

#[test]
fn enum_with_tuple_payload_renders_as_sealed_interface() {
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
    let expected = "public sealed interface Shape permits Empty, Circle, Rect {}\n\n\
        public record Empty() implements Shape {}\n\n\
        public record Circle(int c0) implements Shape {}\n\n\
        public record Rect(int c0, int c1) implements Shape {}";
    assert_eq!(emit_ok(vec![e], &java()), expected);
}

#[test]
fn enum_with_struct_payload_renders_named_record() {
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
    let expected = "public sealed interface Shape permits Rect {}\n\n\
        public record Rect(int w, int h) implements Shape {}";
    assert_eq!(emit_ok(vec![e], &java()), expected);
}

// ---- if / while / for / foreach: native Java control flow ----------------

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
        emit_ok(vec![f], &java()),
        "public static int f(int n) {\n    if (n == 0) {\n        return 1;\n    } else {\n        return 2;\n    }\n}"
    );
}

#[test]
fn while_renders_directly_with_compound_assign() {
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func("loopf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static void loopf() {\n    while (i < 10) {\n        i += 1;\n    }\n}"
    );
}

#[test]
fn counted_for_uses_native_c_style_header() {
    // Java has a native C-style counted for; the init/step render in
    // terminator-free clause form.
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
        emit_ok(vec![f], &java()),
        "public static void sumf() {\n    for (int i = 0; i < 10; i += 1) {\n        sum += i;\n    }\n}"
    );
}

#[test]
fn foreach_renders_enhanced_for() {
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
        emit_ok(vec![f], &java()),
        "public static void iter() {\n    for (var x : xs) {\n        use(x);\n    }\n}"
    );
}

// ---- switch: a classic Java switch statement -----------------------------

#[test]
fn switch_renders_as_switch_statement() {
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
    let expected = "public static int f(int n) {\n    switch (n) {\n        case 0:\n            return 1;\n            break;\n        case 1:\n            return 2;\n            break;\n        default:\n            return 0;\n            break;\n    }\n}";
    assert_eq!(emit_ok(vec![f], &java()), expected);
}

// ---- call / operators -----------------------------------------------------

#[test]
fn call_renders_as_java_call() {
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static int g() {\n    return f(x, (y + 1));\n}"
    );
}

#[test]
fn operators_use_java_spellings() {
    // return a != b && !c
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
        emit_ok(vec![f], &java()),
        "public static boolean test() {\n    return (a != b) && (!c);\n}"
    );
}

#[test]
fn ushr_and_bitwise_are_native() {
    // Java has native >>> and every bitwise operator.
    let expr = binary(BinaryOp::UShr, r("a"), int("2"));
    let f = func("s", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static int s() {\n    return a >>> 2;\n}"
    );
}

#[test]
fn pow_and_floordiv_are_forbidden() {
    // Java has no ** or // operator (Math.pow / Math.floorDiv).
    for op in [BinaryOp::Pow, BinaryOp::FloorDiv] {
        let expr = binary(op, r("a"), r("b"));
        let f = func("p", vec![], i32t(), vec![Statement::Return(Some(expr))]);
        assert!(
            emit_err(vec![f], &java()).contains("forbid"),
            "{op:?} should be forbidden in Java (it is a library call)"
        );
    }
}

// ---- lambda: a Java lambda `(x) -> { … }` --------------------------------

#[test]
fn lambda_renders_as_java_lambda() {
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
        emit_ok(vec![f], &java()),
        "public static void mk() {\n    var g = (x) -> { return x + 1; };\n}"
    );
}

// ---- cast / struct literal / array ---------------------------------------

#[test]
fn cast_renders_as_java_cast() {
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
        emit_ok(vec![f], &java()),
        "public static long widen() {\n    return (long) x;\n}"
    );
}

#[test]
fn struct_literal_renders_as_new_constructor_call() {
    // new Point(1, 2)  (Java's canonical constructor is positional)
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
        emit_ok(vec![f], &java()),
        "public static Point mk() {\n    return new Point(1, 2);\n}"
    );
}

#[test]
fn array_literal_renders_as_initializer() {
    let arr = Expr::ArrayLit {
        elems: vec![int("10"), int("20"), int("30")],
        meta: Meta::new(),
    };
    let g = Statement::Let {
        name: "xs".to_string(),
        ty: Some(Type::Array {
            elem: Box::new(i32t()),
            len: None,
        }),
        value: Some(arr),
    };
    let f = func("arrf", vec![], Type::Primitive(Primitive::Void), vec![g]);
    assert_eq!(
        emit_ok(vec![f], &java()),
        "public static void arrf() {\n    int[] xs = {10, 20, 30};\n}"
    );
}

// ---- items: const / use --------------------------------------------------

#[test]
fn const_renders_as_static_final() {
    let konst = Item::Const {
        name: "answer".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![konst], &java()),
        "public static final int answer = 42;"
    );
}

#[test]
fn use_renders_as_import() {
    let u = Item::Use {
        path: "java.util.List".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![u], &java()), "import java.util.List;");
}

#[test]
fn typedef_is_forbidden() {
    // Java has no transparent type alias.
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![td], &java()).contains("forbid"),
        "Java has no typedef/type-alias construct"
    );
}

// ---- multiple items: blank-line separated --------------------------------

#[test]
fn multiple_items_are_blank_line_separated() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    let f = func("f", vec![], i32t(), vec![Statement::Return(Some(int("0")))]);
    assert_eq!(
        emit_ok(vec![s, f], &java()),
        "public record P(int x) {}\n\npublic static int f() {\n    return 0;\n}"
    );
}
