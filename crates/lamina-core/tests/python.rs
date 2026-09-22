//! End-to-end tests for the shipped `python.mdl` language definition.
//!
//! Python uses indentation rather than braces: a block is a `:` header plus an
//! *indented suite* with no delimiters, so these tests verify that a
//! multi-line body nests correctly on a brace-free target. They also pin:
//!
//! * dynamically-typed signatures with optional type hints
//!   (`def add(a: int, b: int) -> int:`),
//! * a `struct` → `@dataclass`, a plain `enum` → `enum.Enum`,
//! * `if`/`while`/`foreach` suites and Python 3.10 `match`/`case`,
//! * the operators Python natively has that Rust/C lack (`**`, `//`),
//! * a single-expression lambda → `lambda x: x + 1` (`body is single`),
//! * a multi-statement lambda → HOISTED to a module-level `def <fresh>(x): …`
//!   (routed into the `defs` region assembled at the top) plus a reference to
//!   its generated name at the lambda site (`body is block`; kernel blocker #2).
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `python.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. Every asserted output was
//! hand-verified to be valid, idiomatic Python 3.10; the lambda tests
//! additionally cross-check the emitted source with `python3 -c "ast.parse(…)"`
//! via [`assert_valid_python`] when an interpreter is present on the host.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Param, Primitive, Statement,
    SwitchCase, Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn python() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("python.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped python.mdl should load: {e}"))
}

fn emit_ok(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_err(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang)
        .expect_err("expected a forbidden-construct error")
        .to_string()
}

/// Cross-checks that `source` is syntactically valid Python by shelling out to
/// `python3 -c "import ast; ast.parse(...)"`. If `python3` is not available on
/// the host, the check is skipped (the exact-string assertions are the primary
/// proof; this is an extra guard when an interpreter is present). A `python3`
/// that IS present but rejects the source fails the test loudly.
fn assert_valid_python(source: &str) {
    use std::process::Command;
    let script = "import sys, ast; ast.parse(sys.stdin.read())";
    let mut child = match Command::new("python3")
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        // No interpreter on this host — skip the cross-check.
        Err(_) => return,
    };
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("python3 stdin")
        .write_all(source.as_bytes())
        .expect("write source to python3");
    let output = child.wait_with_output().expect("python3 to run");
    assert!(
        output.status.success(),
        "emitted Python failed ast.parse:\n{source}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn shipped_python_def_loads() {
    let _ = python();
}

// ---- fn: a Python def with type hints and an indented suite ---------------

#[test]
fn simple_fn_renders_with_type_hints_and_indented_body() {
    // def add(a: int, b: int) -> int:
    //     return a + b
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def add(a: int, b: int) -> int:\n    return a + b"
    );
}

#[test]
fn void_fn_omits_return_annotation() {
    // def noop():
    //     return
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(emit_ok(vec![f], &python()), "def noop():\n    return");
}

#[test]
fn async_fn_renders_async_def() {
    let mut function = Function {
        name: "fetch".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![lamina_core::ast::Modifier::Async],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("0")))],
        meta: Meta::new(),
    };
    function.modifiers = vec![lamina_core::ast::Modifier::Async];
    assert_eq!(
        emit_ok(vec![Item::Function(function)], &python()),
        "async def fetch() -> int:\n    return 0"
    );
}

// ---- nested indentation: the whole point of the Python validator ----------

#[test]
fn nested_blocks_indent_correctly() {
    // def f(n: int) -> int:
    //     while n > 0:
    //         if n == 1:
    //             return 1
    //         n -= 1
    //     return 0
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
    // Hand-verified valid Python: three levels of indentation (4/8/12 spaces),
    // the compound-assignment idiom `n = n - 1` → `n -= 1`, no braces.
    let expected = "def f(n: int) -> int:\n    while n > 0:\n        if n == 1:\n            return 1\n        n -= 1\n    return 0";
    assert_eq!(emit_ok(vec![f], &python()), expected);
}

// ---- if / elif-ish / while / foreach --------------------------------------

#[test]
fn if_else_renders_indented_suites() {
    // if n == 0:
    //     return 1
    // else:
    //     return 2
    let stmt = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("0")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![stmt]);
    // The else block is a `Block`, which the def renders as `if True:` — so the
    // faithful output nests the block. Assert the exact form.
    let expected =
        "def f(n: int) -> int:\n    if n == 0:\n        return 1\n    else:\n        if True:\n            return 2";
    assert_eq!(emit_ok(vec![f], &python()), expected);
}

#[test]
fn plain_if_renders_indented_suite() {
    let stmt = Statement::If {
        cond: r("flag"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: None,
    };
    let f = func("f", vec![], i32t(), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def f() -> int:\n    if flag:\n        return 1"
    );
}

#[test]
fn while_renders_indented_suite() {
    // while i < 10:
    //     i += 1
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func("loopf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def loopf():\n    while i < 10:\n        i += 1"
    );
}

#[test]
fn foreach_renders_python_for_in() {
    // for x in xs:
    //     total += x
    let step = Statement::assign(r("total"), add(r("total"), r("x"))).unwrap();
    let stmt = Statement::ForEach {
        binding: "x".to_string(),
        iterable: r("xs"),
        body: vec![step],
    };
    let f = func("sumf", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def sumf():\n    for x in xs:\n        total += x"
    );
}

#[test]
fn counted_for_is_forbidden() {
    // Python has no C-style counted `for`; it is forbidden (a layer lowers it to
    // a `while` or a `range`-based `foreach`).
    let init = Statement::Let {
        name: "i".to_string(),
        ty: None,
        value: Some(int("0")),
    };
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::For {
        init: Some(Box::new(init)),
        cond: Some(binary(BinaryOp::Lt, r("i"), int("10"))),
        step: Some(Box::new(step)),
        body: vec![Statement::Break],
    };
    let f = func("f", vec![], Type::Primitive(Primitive::Void), vec![stmt]);
    assert!(
        emit_err(vec![f], &python()).contains("forbid"),
        "C-style counted for should be forbidden in Python"
    );
}

// ---- match / case (Python 3.10+) ------------------------------------------

#[test]
fn switch_renders_as_match_case() {
    // match n:
    //     case 0:
    //         return 1
    //     case 1:
    //         return 2
    //     case _:
    //         return 0
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
    let expected = "def f(n: int) -> int:\n    match n:\n        case 0:\n            return 1\n        case 1:\n            return 2\n        case _:\n            return 0";
    assert_eq!(emit_ok(vec![f], &python()), expected);
}

// ---- let / assignment -----------------------------------------------------

#[test]
fn let_untyped_and_typed_bindings() {
    // x = 5
    let untyped = Statement::Let {
        name: "x".to_string(),
        ty: None,
        value: Some(int("5")),
    };
    let f = func("g", vec![], Type::Primitive(Primitive::Void), vec![untyped]);
    assert_eq!(emit_ok(vec![f], &python()), "def g():\n    x = 5");

    // y: int = 7
    let typed = Statement::Let {
        name: "y".to_string(),
        ty: Some(i32t()),
        value: Some(int("7")),
    };
    let f = func("h", vec![], Type::Primitive(Primitive::Void), vec![typed]);
    assert_eq!(emit_ok(vec![f], &python()), "def h():\n    y: int = 7");
}

// ---- struct -> @dataclass -------------------------------------------------

#[test]
fn struct_renders_as_dataclass() {
    // @dataclass
    // class Point:
    //     x: int
    //     y: int
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &python()),
        "@dataclass\nclass Point:\n    x: int\n    y: int"
    );
}

// ---- struct type attributes -> @dataclass options -------------------------

#[test]
fn equatable_struct_renders_dataclass_eq_option() {
    // A single `equatable` attribute realizes as `@dataclass(eq=True)` — value
    // equality, made explicit. Hand-verified + ast.parse valid Python.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &python());
    assert_eq!(out, "@dataclass(eq=True)\nclass Point:\n    x: int");
    assert_valid_python(&format!("from dataclasses import dataclass\n{out}"));
}

#[test]
fn comparable_and_hashable_render_order_and_frozen_options() {
    // `comparable` -> order=True (generates the ordering dunders); `hashable` ->
    // frozen=True (a frozen dataclass is hashable). Two distinct dataclass
    // options, comma-joined, no collision.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Comparable, TypeAttribute::Hashable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &python());
    assert_eq!(out, "@dataclass(order=True, frozen=True)\nclass P:\n    x: int");
    assert_valid_python(&format!("from dataclasses import dataclass\n{out}"));
}

#[test]
fn displayable_comparable_hashable_all_realize() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![
            TypeAttribute::Displayable,
            TypeAttribute::Comparable,
            TypeAttribute::Hashable,
        ],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &python());
    assert_eq!(
        out,
        "@dataclass(repr=True, order=True, frozen=True)\nclass P:\n    x: int"
    );
    assert_valid_python(&format!("from dataclasses import dataclass\n{out}"));
}

#[test]
fn iterable_attribute_is_forbidden() {
    // A dataclass has no iteration option; Python realizes iteration only via a
    // hand-written `__iter__`, not a declarative dataclass parameter, so the
    // `iterable` attribute is genuinely forbidden.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Iterable],
        meta: Meta::new(),
    };
    let err = emit(&File { items: vec![s] }, &python())
        .expect_err("iterable should be forbidden");
    assert!(
        err.to_string().contains("forbid") || format!("{err:?}").contains("Forbidden"),
        "iterable should surface a forbidden-construct error, got: {err:?}"
    );
}

// ---- enum -> enum.Enum ----------------------------------------------------

#[test]
fn plain_enum_renders_as_enum_class() {
    // class Color(Enum):
    //     RED = auto()
    //     GREEN = auto()
    //     BLUE = auto()
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
        emit_ok(vec![e], &python()),
        "class Color(Enum):\n    RED = auto()\n    GREEN = auto()\n    BLUE = auto()"
    );
}

#[test]
fn enum_with_payload_is_forbidden() {
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
        emit_err(vec![e], &python()).contains("forbid"),
        "enum payloads should be forbidden in Python"
    );
}

// ---- function call --------------------------------------------------------

#[test]
fn call_renders_as_python_call() {
    // return f(x, y + 1)   (a compound arg is parenthesized by the engine)
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def g() -> int:\n    return f(x, (y + 1))"
    );
}

// ---- operators: ** and // (Python HAS these) ------------------------------

#[test]
fn exponent_operator_maps_to_double_star() {
    // return a ** b
    let expr = binary(BinaryOp::Pow, r("a"), r("b"));
    let f = func("p", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def p() -> int:\n    return a ** b"
    );
}

#[test]
fn floor_division_operator_maps_to_double_slash() {
    // return a // b
    let expr = binary(BinaryOp::FloorDiv, r("a"), r("b"));
    let f = func("d", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def d() -> int:\n    return a // b"
    );
}

#[test]
fn unsigned_right_shift_is_forbidden() {
    // Python has no `>>>`.
    let expr = binary(BinaryOp::UShr, r("a"), r("b"));
    let f = func("s", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert!(
        emit_err(vec![f], &python()).contains("forbid"),
        "the `ushr`/`>>>` operator should be forbidden in Python"
    );
}

// ---- cast: forbidden (Python has no C-style cast) -------------------------

#[test]
fn cast_is_forbidden() {
    let cast = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::I64),
    };
    let f = func("c", vec![], i32t(), vec![Statement::Return(Some(cast))]);
    assert!(
        emit_err(vec![f], &python()).contains("forbid"),
        "a C-style cast should be forbidden in Python"
    );
}

// ---- single-expression lambda -> `lambda x: expr` -------------------------

#[test]
fn single_expression_lambda_renders_inline() {
    // A layer that verified the body is a single expression marks the lambda
    // `meta.form = expr` and lowers the one-expression body to an
    // expression-statement, so it renders as Python's `lambda x: x + 1`. The
    // parameter is untyped (Python lambdas take no annotations) and the body's
    // single expression-statement renders as the bare expression.
    let mut meta = Meta::new();
    meta.set("form", "expr");
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Expr(add(r("x"), int("1")))],
        meta,
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    assert_eq!(
        emit_ok(vec![f.clone()], &python()),
        "def mk():\n    g = lambda x: x + 1"
    );
    assert_valid_python(&emit_ok(vec![f], &python()));
}

#[test]
fn two_param_single_expression_lambda() {
    // g = lambda a, b: a + b
    let mut meta = Meta::new();
    meta.set("form", "expr");
    let lambda = Expr::Lambda {
        params: vec![param("a", i32t()), param("b", i32t())],
        return_type: None,
        body: vec![Statement::Expr(add(r("a"), r("b")))],
        meta,
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def mk():\n    g = lambda a, b: a + b"
    );
}

#[test]
fn zero_param_single_expression_lambda() {
    // g = lambda: 1
    let mut meta = Meta::new();
    meta.set("form", "expr");
    let lambda = Expr::Lambda {
        params: vec![],
        return_type: None,
        body: vec![Statement::Expr(int("1"))],
        meta,
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    assert_eq!(emit_ok(vec![f], &python()), "def mk():\n    g = lambda: 1");
}

#[test]
fn single_return_value_lambda_renders_inline() {
    // A `body is single` lambda whose one statement is a value `return` also has
    // a valid inline Python spelling: the `### lambda_tail` projection strips the
    // `return` keyword, leaving the bare expression `lambda x: x + 1`.
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
    let out = emit_ok(vec![f], &python());
    assert_eq!(out, "def mk():\n    g = lambda x: x + 1");
    assert_valid_python(&out);
}

// ---- multi-statement lambda -> hoisted to a named `def` + reference -------

#[test]
fn multi_statement_lambda_hoists_to_named_def() {
    // Python's `lambda` may hold only ONE expression, never statements. A
    // multi-statement lambda (`body is block`) therefore has no inline form and
    // is HOISTED: the definition lifts it to a module-level `def <fresh>(x): …`
    // (routed into the `defs` region, assembled at the top) and leaves the
    // generated name as the inline reference. This is the whole point of kernel
    // blocker #2 — the engine's projected statement-sequence + region routing +
    // per-lambda fresh_name make the hoist expressible entirely in the def.
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
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
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    let out = emit_ok(vec![f], &python());
    // The hoisted `def` (with a NON-EMPTY body) appears at the top, and the
    // lambda site references its generated name — the SAME `lam_0` in both.
    assert_eq!(
        out,
        "def lam_0(x: int):\n    y = x + 1\n    return y\ndef mk():\n    g = lam_0"
    );
    // The hoisted definition's body is genuinely non-empty (the core-bug proof).
    assert!(out.contains("y = x + 1\n    return y"), "hoisted body must be non-empty:\n{out}");
    // The definition and the reference share ONE generated name.
    assert_eq!(out.matches("lam_0").count(), 2, "def + reference share one name:\n{out}");
    // Cross-check the emitted Python parses (when python3 is present).
    assert_valid_python(&out);
}

#[test]
fn two_distinct_multi_statement_lambdas_get_distinct_names() {
    // Two DISTINCT multi-statement lambdas must hoist to two DISTINCT names
    // (no collision), each definition/reference pair internally consistent —
    // the per-lambda fresh_name keying (cause #4).
    let mk_lambda = |var: &str| Expr::Lambda {
        params: vec![param("x", i32t())],
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
    let body = vec![
        Statement::Let {
            name: "g".to_string(),
            ty: None,
            value: Some(mk_lambda("y")),
        },
        Statement::Let {
            name: "h".to_string(),
            ty: None,
            value: Some(mk_lambda("z")),
        },
    ];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    let out = emit_ok(vec![f], &python());
    // Two hoisted defs with distinct names, each referenced once.
    assert!(out.contains("def lam_0("), "first hoisted def missing:\n{out}");
    assert!(out.contains("def lam_1("), "second hoisted def missing:\n{out}");
    assert!(out.contains("g = lam_0"), "first reference must use lam_0:\n{out}");
    assert!(out.contains("h = lam_1"), "second reference must use lam_1:\n{out}");
    // No collision: lam_0 and lam_1 each appear exactly twice (def + ref).
    assert_eq!(out.matches("lam_0").count(), 2, "lam_0 = def + ref:\n{out}");
    assert_eq!(out.matches("lam_1").count(), 2, "lam_1 = def + ref:\n{out}");
    // Cross-check the emitted Python parses (when python3 is present).
    assert_valid_python(&out);
}

// ---- struct literal -> dataclass construction -----------------------------

#[test]
fn struct_literal_renders_as_keyword_construction() {
    // return Point(x=1, y=2)
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
        emit_ok(vec![f], &python()),
        "def mk() -> Point:\n    return Point(x=1, y=2)"
    );
}

// ---- array literal + index + list type ------------------------------------

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
        emit_ok(vec![f], &python()),
        "def arrf() -> int:\n    [10, 20, 30]\n    return arr[1]"
    );
}

#[test]
fn list_type_annotation_renders() {
    // A parameter typed as a fixed array renders `list[int]` (Python drops the
    // length).
    let f = func(
        "takes",
        vec![param(
            "xs",
            Type::Array {
                elem: Box::new(i32t()),
                len: Some("3".to_string()),
            },
        )],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    assert_eq!(
        emit_ok(vec![f], &python()),
        "def takes(xs: list[int]):\n    return"
    );
}

// ---- items: typedef / const / use -----------------------------------------

#[test]
fn typedef_const_and_use_render() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![td], &python()), "Id = int");

    let konst = Item::Const {
        name: "ANSWER".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![konst], &python()), "ANSWER: int = 42");

    let bare = Item::Use {
        path: "os".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![bare], &python()), "import os");

    let aliased = Item::Use {
        path: "numpy".to_string(),
        items: vec![],
        alias: Some("np".to_string()),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![aliased], &python()), "import numpy as np");

    let selective = Item::Use {
        path: "typing".to_string(),
        items: vec![
            lamina_core::ast::UseItem {
                name: "List".to_string(),
                alias: None,
                meta: Meta::new(),
            },
            lamina_core::ast::UseItem {
                name: "Dict".to_string(),
                alias: Some("D".to_string()),
                meta: Meta::new(),
            },
        ],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![selective], &python()),
        "from typing import List, Dict as D"
    );
}

// ---- multiple items: blank-line separated ---------------------------------

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
        emit_ok(vec![s, f], &python()),
        "@dataclass\nclass P:\n    x: int\n\ndef f() -> int:\n    return 0"
    );
}

// ---- raw item passes through unchanged ------------------------------------

#[test]
fn raw_item_passes_through() {
    let raw = Item::Raw {
        code: "# hand-written\nprint('hi')".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![raw], &python()), "# hand-written\nprint('hi')");
}
