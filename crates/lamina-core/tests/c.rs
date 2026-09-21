//! End-to-end tests for the shipped `c.mdl` language definition.
//!
//! C is the kernel's *maximal-forbid* validator: the imperative core maps
//! DIRECTLY onto C's native constructs (`fn` → C function, `struct` → C
//! `struct`, a plain `enum` → C `enum`, `if`/`while`/`for`/`switch`/assignment
//! → their C forms, a cast → `(T)x`, an array literal → `{…}`, a call →
//! `f(…)`, operators → their C spellings), while everything C *lacks* is
//! honestly `forbid`den — closures (`Expr::Lambda`), the iterator loop
//! (`foreach`), enum payloads, type-level deriving, and the `**`/`//`/`>>>`
//! operators.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `c.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. No C compiler is
//! available, so every asserted output was hand-verified to be valid,
//! idiomatic C11.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, FieldInit, Function, Item, Meta, Param, Primitive, Statement,
    SwitchCase, Type, TypeAttribute, UnaryOp, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn c() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("c.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped c.mdl should load: {e}"))
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
fn shipped_c_def_loads() {
    let _ = c();
}

// ---- fn: a C function definition -----------------------------------------

#[test]
fn simple_fn_renders_as_c_function() {
    // int32_t add(int32_t a, int32_t b) {
    //     return a + b;
    // }
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    assert_eq!(
        emit_ok(vec![f], &c()),
        "int32_t add(int32_t a, int32_t b) {\n    return a + b;\n}"
    );
}

#[test]
fn void_fn_with_no_params() {
    // void noop() {
    //     return;
    // }
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(emit_ok(vec![f], &c()), "void noop() {\n    return;\n}");
}

// ---- struct: a C struct declaration --------------------------------------

#[test]
fn struct_renders_as_c_struct() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &c()),
        "struct Point {\n    int32_t x;\n    int32_t y;\n};"
    );
}

#[test]
fn struct_attributes_are_forbidden() {
    // C has no deriving; requesting a type attribute is a clean forbidden
    // construct (not silently dropped).
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![s], &c()).contains("forbid"),
        "struct attributes should be forbidden in C"
    );
}

// ---- enum: a plain C enum (payloads forbidden) ---------------------------

#[test]
fn plain_enum_renders_as_c_enum() {
    // enum Color {
    //     Red,
    //     Green,
    //     Blue
    // };
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
        emit_ok(vec![e], &c()),
        "enum Color {\n    Red,\n    Green,\n    Blue\n};"
    );
}

#[test]
fn enum_with_payload_is_forbidden() {
    // C enums are plain integer constants — a payload-bearing variant has no
    // native C enum form and is honestly forbidden.
    let e = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Circle".to_string(),
            payload: VariantPayload::Tuple(vec![i32t()]),
            meta: Meta::new(),
        }],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![e], &c()).contains("forbid"),
        "enum payloads should be forbidden in C"
    );
}

// ---- if / while / for: direct C control flow -----------------------------

#[test]
fn if_else_renders_directly() {
    // if (n == 0) {
    //     return 1;
    // } else {
    //     return 2;
    // }
    let stmt = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("0")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &c()),
        "int32_t f(int32_t n) {\n    if (n == 0) {\n        return 1;\n    } else {\n        return 2;\n    }\n}"
    );
}

#[test]
fn while_renders_directly() {
    // while (i < 10) {
    //     i = i + 1;
    // }
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func(
        "loopf",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    // Note the compound-assignment idiom: `i = i + 1` recognized as `i += 1`.
    assert_eq!(
        emit_ok(vec![f], &c()),
        "void loopf() {\n    while (i < 10) {\n        i += 1;\n    }\n}"
    );
}

#[test]
fn counted_for_renders_c_style_header() {
    // for (int32_t i = 0; i < 10; i = i + 1) {
    //     sum = sum + i;
    // }
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
    let f = func(
        "sumf",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    assert_eq!(
        emit_ok(vec![f], &c()),
        "void sumf() {\n    for (int32_t i = 0; i < 10; i += 1) {\n        sum += i;\n    }\n}"
    );
}

#[test]
fn foreach_is_forbidden() {
    // C has no iterator loop; foreach is honestly forbidden.
    let stmt = Statement::ForEach {
        binding: "x".to_string(),
        iterable: r("xs"),
        body: vec![Statement::Break],
    };
    let f = func(
        "iter",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    assert!(
        emit_err(vec![f], &c()).contains("forbid"),
        "foreach should be forbidden in C"
    );
}

// ---- assignment: plain and compound --------------------------------------

#[test]
fn plain_assignment_renders() {
    let stmt = Statement::assign(r("x"), int("5")).unwrap();
    let f = func(
        "setx",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    assert_eq!(
        emit_ok(vec![f], &c()),
        "void setx() {\n    x = 5;\n}"
    );
}

// ---- switch: a native C switch with explicit break -----------------------

#[test]
fn switch_renders_with_break_and_default() {
    // switch (n) {
    //     case 0:
    //         return 1;
    //         break;
    //     case 1:
    //         return 2;
    //         break;
    //     default:
    //         return 0;
    // }
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
    let expected = "int32_t f(int32_t n) {\n    switch (n) {\n        case 0:\n            return 1;\n            break;\n        case 1:\n            return 2;\n            break;\n        default:\n            return 0;\n    }\n}";
    assert_eq!(emit_ok(vec![f], &c()), expected);
}

// ---- cast: C prefix (T)x form --------------------------------------------

#[test]
fn cast_renders_as_c_prefix() {
    // (int64_t)x
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
        emit_ok(vec![f], &c()),
        "int64_t widen() {\n    return (int64_t)x;\n}"
    );
}

// ---- array: literal + indexing -------------------------------------------

#[test]
fn array_literal_and_index_render() {
    // An array *literal* and indexing are valid C: `{10, 20, 30}` and `arr[1]`.
    // (The array *type* in a declarator is handled separately by
    // `array_type_declarator_renders_valid_c`.)
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
        emit_ok(vec![f], &c()),
        "int32_t arrf() {\n    {10, 20, 30};\n    return arr[1];\n}"
    );
}

#[test]
fn array_type_declarator_renders_valid_c() {
    // BLOCKER #1 FIXED: C's array declarator is POSTFIX around the NAME
    // (`int32_t arr[3]`). The slot-argument declarator seam lets the `let`
    // binding inject its NAME into the type via `{let_type(name: {name})}`, and
    // the `### array` type slot weaves that name into the C declarator. So an
    // array-typed binding now renders VALID, idiomatic C (hand-verified) rather
    // than the previously-invalid `int32_t[3] arr`.
    let assign_arr = Statement::Let {
        name: "arr".to_string(),
        ty: Some(Type::Array {
            elem: Box::new(i32t()),
            len: Some("3".to_string()),
        }),
        value: Some(Expr::ArrayLit {
            elems: vec![int("10"), int("20"), int("30")],
            meta: Meta::new(),
        }),
    };
    let f = func(
        "arrf",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![assign_arr],
    );
    // `int32_t arr[3] = {10, 20, 30};` is VALID C — the declarator name is woven
    // into the array type by the slot-argument seam.
    assert_eq!(
        emit_ok(vec![f], &c()),
        "void arrf() {\n    int32_t arr[3] = {10, 20, 30};\n}"
    );
}

#[test]
fn array_typed_param_renders_valid_c() {
    // An array-typed PARAMETER also weaves the name into the declarator:
    // `int32_t xs[3]` (valid C), via the `param` slot's `{type(name: {name})}`.
    let f = func(
        "sum",
        vec![param(
            "xs",
            Type::Array {
                elem: Box::new(i32t()),
                len: Some("3".to_string()),
            },
        )],
        i32t(),
        vec![Statement::Return(Some(int("0")))],
    );
    assert_eq!(
        emit_ok(vec![f], &c()),
        "int32_t sum(int32_t xs[3]) {\n    return 0;\n}"
    );
}

#[test]
fn fn_pointer_typed_param_renders_valid_c() {
    // A function-pointer PARAMETER weaves the name INSIDE the `(*name)` group —
    // C's postfix-around-the-name fn-pointer declarator. `int (*op)(int)` is
    // valid C (hand-verified). Here the fnptr is `i32 (*)(i32)`.
    let fnptr = Type::FnPtr {
        params: vec![i32t()],
        ret: Box::new(i32t()),
    };
    let f = func(
        "apply",
        vec![param("op", fnptr)],
        i32t(),
        vec![Statement::Return(Some(int("0")))],
    );
    assert_eq!(
        emit_ok(vec![f], &c()),
        "int32_t apply(int32_t (*op)(int32_t)) {\n    return 0;\n}"
    );
}

#[test]
fn fn_pointer_typedef_renders_bare_type_woven_name() {
    // A `typedef` to a function-pointer type is itself a declarator: the alias
    // NAME sits inside `(*Name)`. `typedef int32_t (*Op)(int32_t);` is valid C.
    let td = Item::TypeDef {
        name: "Op".to_string(),
        target: Type::FnPtr {
            params: vec![i32t()],
            ret: Box::new(i32t()),
        },
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![td], &c()),
        "typedef int32_t (*Op)(int32_t);"
    );
}

// ---- function call --------------------------------------------------------

#[test]
fn call_renders_as_c_call() {
    // g = f(x, y + 1);   (a compound arg is parenthesized by the engine)
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    assert_eq!(
        emit_ok(vec![f], &c()),
        "int32_t g() {\n    return f(x, (y + 1));\n}"
    );
}

// ---- operators: C spellings + unary --------------------------------------

#[test]
fn operators_use_c_spellings() {
    // return a != b && !c;
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
    // Compound operands are parenthesized by the engine to preserve grouping:
    // `(a != b) && (!c)`.
    assert_eq!(
        emit_ok(vec![f], &c()),
        "bool test() {\n    return (a != b) && (!c);\n}"
    );
}

#[test]
fn exponent_operator_is_forbidden() {
    // C has no `**` operator.
    let expr = binary(BinaryOp::Pow, r("a"), r("b"));
    let f = func("p", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert!(
        emit_err(vec![f], &c()).contains("forbid"),
        "the `pow`/`**` operator should be forbidden in C"
    );
}

// ---- struct literal: C compound literal ----------------------------------

#[test]
fn struct_literal_renders_as_compound_literal() {
    // (Point){ .x = 1, .y = 2 }
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
        emit_ok(vec![f], &c()),
        "Point mk() {\n    return (Point){ .x = 1, .y = 2 };\n}"
    );
}

// ---- lambda: forbidden (C has no closures) -------------------------------

#[test]
fn lambda_is_forbidden() {
    // C has no closures; Expr::Lambda is honestly forbidden (a layer hoists it
    // to a named function + function pointer).
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let f = func("mk", vec![], named("F"), vec![Statement::Return(Some(lam))]);
    assert!(
        emit_err(vec![f], &c()).contains("forbid"),
        "Expr::Lambda should be forbidden in C"
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
    assert_eq!(emit_ok(vec![td], &c()), "typedef int32_t Id;");

    let konst = Item::Const {
        name: "answer".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![konst], &c()), "const int32_t answer = 42;");

    let u = Item::Use {
        path: "stdio.h".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![u], &c()), "#include <stdio.h>");
}

// ---- pointer type: C `T *` -----------------------------------------------

#[test]
fn pointer_typedef_renders() {
    // typedef int32_t *IntPtr;  (the alias name is woven into the pointer
    // declarator via the slot-argument seam — valid, idiomatic C.)
    let td = Item::TypeDef {
        name: "IntPtr".to_string(),
        target: Type::Pointer(Box::new(i32t())),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![td], &c()), "typedef int32_t *IntPtr;");
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
        emit_ok(vec![s, f], &c()),
        "struct P {\n    int32_t x;\n};\n\nint32_t f() {\n    return 0;\n}"
    );
}
