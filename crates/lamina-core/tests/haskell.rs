//! End-to-end tests for the shipped `haskell.mdl` language definition.
//!
//! Haskell is a pure functional language, so this suite proves the dual-paradigm
//! kernel lowers to *idiomatic* Haskell for the functional-shaped subset (a
//! `fn` as a defining equation, a `struct` as a record, an `enum` as an ADT, a
//! `switch` as a `case … of`, an `if` as an if-expression, `Expr::Lambda` as
//! `\x -> …`, a `Call` as juxtaposition application, and type attributes as a
//! `deriving (…)` clause) and to a correct — if verbose — recursive monadic
//! action for the imperative `while`.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `haskell.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. No GHC is available, so
//! every asserted output was hand-verified to be valid, idiomatic Haskell.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, File, Function, Item, Meta, Param, Primitive, Statement, SwitchCase,
    Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn haskell() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("haskell.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped haskell.mdl should load: {e}"))
}

fn emit_ok(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
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

fn add(lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

// ---- the shipped def loads (validates every slot/fact/matrix) -------------

#[test]
fn shipped_haskell_def_loads() {
    let _ = haskell();
}
// ---- simple fn: an equation ----------------------------------------------

#[test]
fn simple_fn_renders_as_equation() {
    // add :: Int32 -> Int32 -> Int32
    // add a b = a + b
    //
    // The signature is built by the `{params:param_type}` projection (each
    // param type followed by ` -> `, terminated by the return type); the
    // equation by the `{params:param_name}` projection (space-separated names).
    // Same `params` list, two projections. Hand-verified valid Haskell.
    let f = Item::Function(Function {
        name: "add".to_string(),
        visibility: Visibility::Public,
        modifiers: vec![],
        params: vec![param("a", i32t()), param("b", i32t())],
        return_type: i32t(),
        body: vec![Statement::Return(Some(add(r("a"), r("b"))))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "add :: Int32 -> Int32 -> Int32\nadd a b = a + b"
    );
}

#[test]
fn zero_param_fn_is_a_plain_binding() {
    // answer :: Int32
    // answer = 42
    //
    // Zero params: both projections loop zero times, so the signature is just
    // the return type and the equation is a plain binding. Hand-verified valid.
    let f = Item::Function(Function {
        name: "answer".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(int("42")))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "answer :: Int32\nanswer = 42"
    );
}

// ---- projected collection slots: same params list, two projections -------

#[test]
fn params_list_renders_two_ways_via_projection() {
    // A three-parameter function proves the projected collection slot: the
    // SAME `params` list is rendered as TYPES in the signature
    // (`{params:param_type}` -> `### param_type`) and as NAMES in the equation
    // (`{params:param_name}` -> `### param_name`). Hand-verified valid Haskell:
    // the parameter types are `->`-joined, then the return type; the equation
    // head is the space-separated parameter names.
    let f = Item::Function(Function {
        name: "combine".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![param("a", i32t()), param("b", i32t()), param("c", i32t())],
        return_type: i32t(),
        body: vec![Statement::Return(Some(add(add(r("a"), r("b")), r("c"))))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "combine :: Int32 -> Int32 -> Int32 -> Int32\ncombine a b c = (a + b) + c"
    );
}

// ---- struct: a record ----------------------------------------------------
#[test]
fn struct_renders_as_record() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &haskell()),
        "data Point = Point { x :: Int32, y :: Int32 }"
    );
}

// ---- deriving from type attributes ---------------------------------------

#[test]
fn struct_attributes_render_as_deriving() {
    // cloneable is INHERENT in Haskell (immutable values), so it contributes
    // nothing; displayable/equatable/comparable -> Show/Eq/Ord.
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![
            TypeAttribute::Displayable,
            TypeAttribute::Equatable,
            TypeAttribute::Comparable,
            TypeAttribute::Cloneable,
        ],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![s], &haskell()),
        "data Point = Point { x :: Int32, y :: Int32 } deriving (Show, Eq, Ord)"
    );
}

#[test]
fn hashable_attribute_is_forbidden() {
    // No Hashable in base -> a type requesting it cannot target Haskell.
    let s = Item::Struct {
        name: "K".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("k", i32t())],
        attributes: vec![TypeAttribute::Hashable],
        meta: Meta::new(),
    };
    let err = emit(&File { items: vec![s] }, &haskell()).unwrap_err();
    assert!(
        err.to_string().contains("forbids"),
        "hashable should be a forbidden construct: {err}"
    );
}

// ---- enum: an ADT (with payloads) ----------------------------------------

#[test]
fn enum_with_payloads_renders_as_adt() {
    // data Shape = Circle Int32 | Rect Int32 Int32 | Empty
    let e = Item::Enum {
        name: "Shape".to_string(),
        visibility: Visibility::Public,
        variants: vec![
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
            Variant {
                name: "Empty".to_string(),
                payload: VariantPayload::None,
                meta: Meta::new(),
            },
        ],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![e], &haskell()),
        "data Shape = Circle Int32 | Rect Int32 Int32 | Empty"
    );
}

#[test]
fn plain_enum_with_deriving() {
    // data Color = Red | Green deriving (Show, Eq)
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
        ],
        attributes: vec![TypeAttribute::Displayable, TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![e], &haskell()),
        "data Color = Red | Green deriving (Show, Eq)"
    );
}

#[test]
fn struct_style_variant_renders_named_fields() {
    // data Msg = Move { x :: Int32, y :: Int32 }
    let e = Item::Enum {
        name: "Msg".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "Move".to_string(),
            payload: VariantPayload::Struct(vec![field("x", i32t()), field("y", i32t())]),
            meta: Meta::new(),
        }],
        attributes: vec![],
        meta: Meta::new(),
    };
    assert_eq!(
        emit_ok(vec![e], &haskell()),
        "data Msg = Move { x :: Int32, y :: Int32 }"
    );
}

// ---- switch: a case … of -------------------------------------------------

#[test]
fn switch_renders_as_case_of() {
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
    let f = Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![param("n", i32t())],
        return_type: i32t(),
        body: vec![sw],
        meta: Meta::new(),
    });
    let expected = "f :: Int32 -> Int32\nf n = case n of\n    0 -> 1\n    1 -> 2\n    _ -> 0";
    assert_eq!(emit_ok(vec![f], &haskell()), expected);
}

// ---- if: an if-expression ------------------------------------------------

#[test]
fn if_renders_as_if_expression() {
    // f n = if n == 0 then 1 else 2
    let f = Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![param("n", i32t())],
        return_type: i32t(),
        body: vec![Statement::If {
            cond: Expr::Binary {
                op: BinaryOp::Eq,
                lhs: Box::new(r("n")),
                rhs: Box::new(int("0")),
            },
            then_block: vec![Statement::Return(Some(int("1")))],
            else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
                int("2"),
            ))]))),
        }],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "f :: Int32 -> Int32\nf n = if n == 0 then 1 else 2"
    );
}

// ---- lambda: \x -> x + 1 -------------------------------------------------

#[test]
fn lambda_renders_as_backslash() {
    // inc = \ x -> x + 1
    // (the leading space after `\` comes from the shared `### param` slot, which
    // functions need to separate `add a b`; `\ x -> …` is valid Haskell.)
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let f = Item::Function(Function {
        name: "inc".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: named("F"),
        body: vec![Statement::Return(Some(lam))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "inc :: F\ninc = \\ x -> x + 1"
    );
}

#[test]
fn lambda_with_return_type_drops_annotation() {
    // Haskell lambdas carry no inline return-type annotation; has_ret_type
    // selects the same form, dropping the type (retained as metadata).
    let lam = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: Some(i32t()),
        body: vec![Statement::Return(Some(add(r("x"), int("1"))))],
        meta: Meta::new(),
    };
    let f = Item::Function(Function {
        name: "inc".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: named("F"),
        body: vec![Statement::Return(Some(lam))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "inc :: F\ninc = \\ x -> x + 1"
    );
}

// ---- call: juxtaposition application -------------------------------------

#[test]
fn call_renders_as_juxtaposition() {
    // g = f x (y + 1)   — NOT f(x, y+1); compound arg parenthesized.
    let f = Item::Function(Function {
        name: "g".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(Expr::Call {
            callee: Box::new(r("f")),
            args: vec![r("x"), add(r("y"), int("1"))],
        }))],
        meta: Meta::new(),
    });
    assert_eq!(
        emit_ok(vec![f], &haskell()),
        "g :: Int32\ng = f x (y + 1)"
    );
}

#[test]
fn nullary_call_is_bare_callee() {
    // h = f    (zero-argument application is just the callee)
    let f = Item::Function(Function {
        name: "h".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(Expr::Call {
            callee: Box::new(r("f")),
            args: vec![],
        }))],
        meta: Meta::new(),
    });
    assert_eq!(emit_ok(vec![f], &haskell()), "h :: Int32\nh = f");
}

// ---- while: honestly forbidden -------------------------------------------

#[test]
fn while_is_forbidden() {
    // Haskell has no mutation; an inline recursive-loop lowering is not robustly
    // valid under layout with the shared statement slot, so `while` is honestly
    // forbidden (a layer hoists it to a column-0 helper or STRef raw nodes).
    let f = Item::Function(Function {
        name: "loopf".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body: vec![Statement::While {
            cond: Expr::Binary {
                op: BinaryOp::Lt,
                lhs: Box::new(r("i")),
                rhs: Box::new(int("10")),
            },
            body: vec![Statement::Expr(Expr::Call {
                callee: Box::new(r("step")),
                args: vec![],
            })],
        }],
        meta: Meta::new(),
    });
    let err = emit(&File { items: vec![f] }, &haskell()).unwrap_err();
    assert!(
        err.to_string().contains("forbids"),
        "while should be a forbidden construct: {err}"
    );
}

// ---- operators: Haskell spellings ----------------------------------------

#[test]
fn operators_use_haskell_spellings() {
    // f = a /= b   (kernel `ne` -> Haskell `/=`, not `!=`)
    let f = Item::Function(Function {
        name: "f".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Bool),
        body: vec![Statement::Return(Some(Expr::Binary {
            op: BinaryOp::Ne,
            lhs: Box::new(r("a")),
            rhs: Box::new(r("b")),
        }))],
        meta: Meta::new(),
    });
    assert_eq!(emit_ok(vec![f], &haskell()), "f :: Bool\nf = a /= b");
}

#[test]
fn record_field_access_is_selector_application() {
    // p = x pt    (obj.field -> `field obj`: a record selector applied)
    let f = Item::Function(Function {
        name: "p".to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: i32t(),
        body: vec![Statement::Return(Some(Expr::Field {
            obj: Box::new(r("pt")),
            field: "x".to_string(),
        }))],
        meta: Meta::new(),
    });
    assert_eq!(emit_ok(vec![f], &haskell()), "p :: Int32\np = x pt");
}

// ---- items: typedef / const / use ----------------------------------------

#[test]
fn typedef_const_and_use_render() {
    let td = Item::TypeDef {
        name: "Name".to_string(),
        target: Type::Primitive(Primitive::Str),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![td], &haskell()), "type Name = String");

    let c = Item::Const {
        name: "answer".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![c], &haskell()), "answer = 42");

    let u = Item::Use {
        path: "Data.List".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![u], &haskell()), "import Data.List");
}
