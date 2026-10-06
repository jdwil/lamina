//! Round-trip equivalence tests for the concrete-syntax parser.
//!
//! The parser's output must EQUAL the ASTs the engine's test suites already
//! assert against. These tests use the hand-built AST as the oracle: for a
//! representative whole-kernel program and for focused per-construct snippets,
//! we HAND-BUILD the [`File`] AST and PARSE the equivalent `.mdl` source, then
//! assert the two `File` values are STRUCTURALLY EQUAL (the AST derives
//! `PartialEq`, which ignores metadata/attributes per the AST contract).
//!
//! A final group proves the parsed AST drives the emitter correctly by
//! transpiling parsed source through a synthetic fixture definition (as
//! `tests/transpile.rs` does), and parses the worked example from
//! `docs/source-syntax.md` (minus its `raw` block, which is a deferred parse
//! error by design).

use lamina_core::ast::{
    Attr, BinaryOp, CaseBindings, CaseFieldBind, Expr, Field, FieldInit, File, Function, Item,
    Meta, Modifier, Param, Primitive, RawArm, Statement, SwitchCase, Type, TypeAttribute, UnaryOp,
    UseItem, Variant, VariantPayload, Visibility,
};
use lamina_core::lang_doc::parse_language_def;
use lamina_core::parser::parse;
use lamina_core::transpile;

/// Convenience: a private `void` function wrapping `body`, so a per-construct
/// snippet can live inside a function the way real source does.
fn func(name: &str, body: Vec<Statement>) -> Item {
    Item::Function(Function {
        name: name.to_string(),
        visibility: Visibility::Private,
        modifiers: vec![],
        params: vec![],
        return_type: Type::Primitive(Primitive::Void),
        body,
        meta: Meta::new(),
    })
}

/// Parses `src` and asserts it equals the single-item `File` `[item]`.
fn assert_item(src: &str, item: Item) {
    let parsed = parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
    assert_eq!(parsed, File { items: vec![item] }, "source: {src}");
}

/// Parses `src` (a function body) and asserts the body equals `body`.
fn assert_body(src_body: &str, body: Vec<Statement>) {
    let src = format!("fn f(): void {{ {src_body} }}");
    assert_item(&src, func("f", body));
}

/// Parses `expr_src` as the value of `return <expr>;` and asserts it equals
/// `expr`.
fn assert_expr(expr_src: &str, expr: Expr) {
    assert_body(
        &format!("return {expr_src};"),
        vec![Statement::Return(Some(expr))],
    );
}

// ======================================================================
// Items
// ======================================================================

#[test]
fn function_with_params_and_colon_return() {
    assert_item(
        "public async fn add(x: i32, y: i32): i32 { return x + y; }",
        Item::Function(Function {
            name: "add".into(),
            visibility: Visibility::Public,
            modifiers: vec![Modifier::Async],
            params: vec![
                Param {
                    name: "x".into(),
                    ty: Type::Primitive(Primitive::I32),
                    meta: Meta::new(),
                },
                Param {
                    name: "y".into(),
                    ty: Type::Primitive(Primitive::I32),
                    meta: Meta::new(),
                },
            ],
            return_type: Type::Primitive(Primitive::I32),
            body: vec![Statement::Return(Some(Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref("x".into())),
                rhs: Box::new(Expr::Ref("y".into())),
            }))],
            meta: Meta::new(),
        }),
    );
}

#[test]
fn struct_with_fields() {
    assert_item(
        "@equatable\n@displayable\nstruct Point { x: i32, y: i32 }",
        Item::Struct {
            name: "Point".into(),
            visibility: Visibility::Private,
            fields: vec![
                Field {
                    name: "x".into(),
                    ty: Type::Primitive(Primitive::I32),
                    visibility: Visibility::Public,
                    meta: Meta::new(),
                },
                Field {
                    name: "y".into(),
                    ty: Type::Primitive(Primitive::I32),
                    visibility: Visibility::Public,
                    meta: Meta::new(),
                },
            ],
            attributes: vec![],
            meta: Meta::new(),
        },
    );
}

#[test]
fn struct_attributes_and_meta_are_captured() {
    let parsed =
        parse("@equatable\n@meta(origin = \"layer\")\nstruct S { a: i32 }").expect("parse");
    match &parsed.items[0] {
        Item::Struct {
            attributes, meta, ..
        } => {
            assert_eq!(attributes, &vec![TypeAttribute::Equatable]);
            assert_eq!(meta.get("origin"), Some("layer"));
        }
        other => panic!("expected struct, got {other:?}"),
    }
}

#[test]
fn enum_unit_tuple_struct_variants() {
    assert_item(
        "enum Shape { Empty, Circle(f64), Rect { w: f64, h: f64 } }",
        Item::Enum {
            name: "Shape".into(),
            visibility: Visibility::Private,
            variants: vec![
                Variant {
                    name: "Empty".into(),
                    payload: VariantPayload::None,
                    meta: Meta::new(),
                },
                Variant {
                    name: "Circle".into(),
                    payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::F64)]),
                    meta: Meta::new(),
                },
                Variant {
                    name: "Rect".into(),
                    payload: VariantPayload::Struct(vec![
                        Field {
                            name: "w".into(),
                            ty: Type::Primitive(Primitive::F64),
                            visibility: Visibility::Public,
                            meta: Meta::new(),
                        },
                        Field {
                            name: "h".into(),
                            ty: Type::Primitive(Primitive::F64),
                            visibility: Visibility::Public,
                            meta: Meta::new(),
                        },
                    ]),
                    meta: Meta::new(),
                },
            ],
            attributes: vec![],
            meta: Meta::new(),
        },
    );
}

#[test]
fn const_item() {
    assert_item(
        "const MAX: i32 = 100;",
        Item::Const {
            name: "MAX".into(),
            ty: Type::Primitive(Primitive::I32),
            value: Expr::IntLiteral("100".into()),
            visibility: Visibility::Private,
            meta: Meta::new(),
        },
    );
}

#[test]
fn typedef_item() {
    assert_item(
        "typedef Celsius = f64;",
        Item::TypeDef {
            name: "Celsius".into(),
            target: Type::Primitive(Primitive::F64),
            meta: Meta::new(),
        },
    );
}

#[test]
fn use_bare_aliased_and_selective() {
    assert_item(
        "use http;",
        Item::Use {
            path: "http".into(),
            items: vec![],
            alias: None,
            meta: Meta::new(),
        },
    );
    assert_item(
        "use math as m;",
        Item::Use {
            path: "math".into(),
            items: vec![],
            alias: Some("m".into()),
            meta: Meta::new(),
        },
    );
    assert_item(
        "use models::{ User, Role as R };",
        Item::Use {
            path: "models".into(),
            items: vec![
                UseItem {
                    name: "User".into(),
                    alias: None,
                    meta: Meta::new(),
                },
                UseItem {
                    name: "Role".into(),
                    alias: Some("R".into()),
                    meta: Meta::new(),
                },
            ],
            alias: None,
            meta: Meta::new(),
        },
    );
}

// ======================================================================
// Statements
// ======================================================================

#[test]
fn let_typed_and_inferred() {
    assert_body(
        "let x: i32 = 1;",
        vec![Statement::Let {
            name: "x".into(),
            ty: Some(Type::Primitive(Primitive::I32)),
            value: Some(Expr::IntLiteral("1".into())),
        }],
    );
    assert_body(
        "let y = 2;",
        vec![Statement::Let {
            name: "y".into(),
            ty: None,
            value: Some(Expr::IntLiteral("2".into())),
        }],
    );
}

#[test]
fn if_else_with_parenthesized_cond() {
    assert_body(
        "if (c) { return; } else { return; }",
        vec![Statement::If {
            cond: Expr::Ref("c".into()),
            then_block: vec![Statement::Return(None)],
            else_block: Some(Box::new(Statement::Block(vec![Statement::Return(None)]))),
        }],
    );
}

#[test]
fn while_loop() {
    assert_body(
        "while (go) { break; }",
        vec![Statement::While {
            cond: Expr::Ref("go".into()),
            body: vec![Statement::Break],
        }],
    );
}

#[test]
fn for_counted_loop() {
    assert_body(
        "for (let i = 0; i < 10; i = i + 1) { continue; }",
        vec![Statement::For {
            init: Some(Box::new(Statement::Let {
                name: "i".into(),
                ty: None,
                value: Some(Expr::IntLiteral("0".into())),
            })),
            cond: Some(Expr::Binary {
                op: BinaryOp::Lt,
                lhs: Box::new(Expr::Ref("i".into())),
                rhs: Box::new(Expr::IntLiteral("10".into())),
            }),
            step: Some(Box::new(Statement::Assign {
                target: Expr::Ref("i".into()),
                value: Expr::Binary {
                    op: BinaryOp::Add,
                    lhs: Box::new(Expr::Ref("i".into())),
                    rhs: Box::new(Expr::IntLiteral("1".into())),
                },
            })),
            body: vec![Statement::Continue],
        }],
    );
}

#[test]
fn foreach_loop() {
    assert_body(
        "foreach p in points { continue; }",
        vec![Statement::ForEach {
            binding: "p".into(),
            iterable: Expr::Ref("points".into()),
            body: vec![Statement::Continue],
        }],
    );
}

#[test]
fn assignment_statement() {
    assert_body(
        "total = total + 1;",
        vec![Statement::Assign {
            target: Expr::Ref("total".into()),
            value: Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref("total".into())),
                rhs: Box::new(Expr::IntLiteral("1".into())),
            },
        }],
    );
}

#[test]
fn expr_statement_call() {
    assert_body(
        "f();",
        vec![Statement::Expr(Expr::Call {
            callee: Box::new(Expr::Ref("f".into())),
            args: vec![],
        })],
    );
}

#[test]
fn switch_plain_cases_and_default() {
    assert_body(
        "switch (v) { case 1 { break; } case 2 { break; } default { return; } }",
        vec![Statement::Switch {
            scrutinee: Expr::Ref("v".into()),
            cases: vec![
                SwitchCase::new(Expr::IntLiteral("1".into()), vec![Statement::Break]),
                SwitchCase::new(Expr::IntLiteral("2".into()), vec![Statement::Break]),
            ],
            default: Some(vec![Statement::Return(None)]),
        }],
    );
}

#[test]
fn switch_positional_payload_binding() {
    // case Circle(r) → CaseBindings::Positional(["r"])
    assert_body(
        "switch (s) { case Circle(r) { return; } }",
        vec![Statement::Switch {
            scrutinee: Expr::Ref("s".into()),
            cases: vec![SwitchCase::with_bindings(
                Expr::Ref("Circle".into()),
                vec![Statement::Return(None)],
                CaseBindings::Positional(vec!["r".into()]),
            )],
            default: None,
        }],
    );
}

#[test]
fn switch_named_payload_binding() {
    // case Rect { w, h } → CaseBindings::Named([w, h] shorthand)
    assert_body(
        "switch (s) { case Rect { w, h } { return; } }",
        vec![Statement::Switch {
            scrutinee: Expr::Ref("s".into()),
            cases: vec![SwitchCase::with_bindings(
                Expr::Ref("Rect".into()),
                vec![Statement::Return(None)],
                CaseBindings::Named(vec![
                    CaseFieldBind::shorthand("w"),
                    CaseFieldBind::shorthand("h"),
                ]),
            )],
            default: None,
        }],
    );
}

// ======================================================================
// Expressions
// ======================================================================

#[test]
fn literals() {
    assert_expr("42", Expr::IntLiteral("42".into()));
    assert_expr("3.14", Expr::FloatLiteral("3.14".into()));
    assert_expr("true", Expr::BoolLiteral(true));
    assert_expr("false", Expr::BoolLiteral(false));
    assert_expr("null", Expr::NullLiteral);
    assert_expr("\"hi\"", Expr::StringLiteral("hi".into()));
    assert_expr("'a'", Expr::CharLiteral("a".into()));
}

#[test]
fn field_index_and_call_chain() {
    // a.b[0](x)
    assert_expr(
        "a.b[0](x)",
        Expr::Call {
            callee: Box::new(Expr::Index {
                obj: Box::new(Expr::Field {
                    obj: Box::new(Expr::Ref("a".into())),
                    field: "b".into(),
                }),
                index: Box::new(Expr::IntLiteral("0".into())),
            }),
            args: vec![Expr::Ref("x".into())],
        },
    );
}

#[test]
fn unary_operators() {
    assert_expr(
        "-x",
        Expr::Unary {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Ref("x".into())),
        },
    );
    assert_expr(
        "!b",
        Expr::Unary {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Ref("b".into())),
        },
    );
    assert_expr(
        "~m",
        Expr::Unary {
            op: UnaryOp::BitNot,
            operand: Box::new(Expr::Ref("m".into())),
        },
    );
}

#[test]
fn all_binary_operator_spellings_parse() {
    // Each spelling maps to the expected kernel op (associativity aside).
    let cases: &[(&str, BinaryOp)] = &[
        ("+", BinaryOp::Add),
        ("-", BinaryOp::Sub),
        ("*", BinaryOp::Mul),
        ("/", BinaryOp::Div),
        ("%", BinaryOp::Rem),
        ("**", BinaryOp::Pow),
        ("==", BinaryOp::Eq),
        ("!=", BinaryOp::Ne),
        ("<", BinaryOp::Lt),
        ("<=", BinaryOp::Le),
        (">", BinaryOp::Gt),
        (">=", BinaryOp::Ge),
        ("&&", BinaryOp::And),
        ("||", BinaryOp::Or),
        ("&", BinaryOp::BitAnd),
        ("|", BinaryOp::BitOr),
        ("^", BinaryOp::BitXor),
        ("<<", BinaryOp::Shl),
        (">>", BinaryOp::Shr),
        (">>>", BinaryOp::UShr),
    ];
    for (spelling, op) in cases {
        assert_expr(
            &format!("a {spelling} b"),
            Expr::Binary {
                op: *op,
                lhs: Box::new(Expr::Ref("a".into())),
                rhs: Box::new(Expr::Ref("b".into())),
            },
        );
    }
}

#[test]
fn cast_expression() {
    assert_expr(
        "x as i64",
        Expr::Cast {
            value: Box::new(Expr::Ref("x".into())),
            ty: Type::Primitive(Primitive::I64),
        },
    );
}

#[test]
fn struct_literal_expression() {
    assert_expr(
        "Point { x: 1, y: 2 }",
        Expr::StructLit {
            type_name: "Point".into(),
            fields: vec![
                FieldInit {
                    name: "x".into(),
                    value: Expr::IntLiteral("1".into()),
                    meta: Meta::new(),
                },
                FieldInit {
                    name: "y".into(),
                    value: Expr::IntLiteral("2".into()),
                    meta: Meta::new(),
                },
            ],
            meta: Meta::new(),
        },
    );
}

#[test]
fn array_literal_expression() {
    assert_expr(
        "[1, 2, 3]",
        Expr::ArrayLit {
            elems: vec![
                Expr::IntLiteral("1".into()),
                Expr::IntLiteral("2".into()),
                Expr::IntLiteral("3".into()),
            ],
            meta: Meta::new(),
        },
    );
}

#[test]
fn lambda_with_and_without_return_type() {
    assert_expr(
        "(x: f64): f64 => { return x; }",
        Expr::Lambda {
            params: vec![Param {
                name: "x".into(),
                ty: Type::Primitive(Primitive::F64),
                meta: Meta::new(),
            }],
            return_type: Some(Type::Primitive(Primitive::F64)),
            body: vec![Statement::Return(Some(Expr::Ref("x".into())))],
            meta: Meta::new(),
        },
    );
    assert_expr(
        "(x: i32) => { return x; }",
        Expr::Lambda {
            params: vec![Param {
                name: "x".into(),
                ty: Type::Primitive(Primitive::I32),
                meta: Meta::new(),
            }],
            return_type: None,
            body: vec![Statement::Return(Some(Expr::Ref("x".into())))],
            meta: Meta::new(),
        },
    );
}

#[test]
fn array_type_sized_and_unsized() {
    // A parameter typed as a sized / unsized array exercises the array TYPE.
    let parsed = parse("fn f(a: [i32; 3], b: [i32]): void { return; }").expect("parse");
    match &parsed.items[0] {
        Item::Function(func) => {
            assert_eq!(
                func.params[0].ty,
                Type::Array {
                    elem: Box::new(Type::Primitive(Primitive::I32)),
                    len: Some("3".into()),
                }
            );
            assert_eq!(
                func.params[1].ty,
                Type::Array {
                    elem: Box::new(Type::Primitive(Primitive::I32)),
                    len: None,
                }
            );
        }
        other => panic!("expected function, got {other:?}"),
    }
}

// ======================================================================
// Tree core
// ======================================================================

#[test]
fn tree_node_with_attrs_and_children() {
    assert_body(
        "node div(class = \"box\") { node p { text \"Hello\" } };",
        vec![Statement::Expr(Expr::Node {
            name: "div".into(),
            attrs: vec![Attr {
                name: "class".into(),
                value: Expr::StringLiteral("box".into()),
                meta: Meta::new(),
            }],
            children: vec![Expr::Node {
                name: "p".into(),
                attrs: vec![],
                children: vec![Expr::Text(Box::new(Expr::StringLiteral("Hello".into())))],
                meta: Meta::new(),
            }],
            meta: Meta::new(),
        })],
    );
}

#[test]
fn top_level_tree_value() {
    assert_item(
        "node html { text \"hi\" }",
        Item::Tree(Expr::Node {
            name: "html".into(),
            attrs: vec![],
            children: vec![Expr::Text(Box::new(Expr::StringLiteral("hi".into())))],
            meta: Meta::new(),
        }),
    );
}

// ======================================================================
// Whole-kernel representative program (minus raw)
// ======================================================================

#[test]
fn whole_kernel_program_round_trips() {
    let src = r#"
        use math::{ sqrt };
        const ORIGIN_X: f64 = 0.0;
        @equatable
        struct Point { x: f64, y: f64 }
        enum Shape {
            Empty,
            Circle(f64),
            Rect { w: f64, h: f64 },
        }
        public fn area(s: Shape): f64 {
            switch (s) {
                case Circle(r) { return r; }
                case Rect { w, h } { return w; }
                default { return 0.0; }
            }
        }
    "#;
    let parsed = parse(src).expect("whole-kernel program parses");

    let expected = File {
        items: vec![
            Item::Use {
                path: "math".into(),
                items: vec![UseItem {
                    name: "sqrt".into(),
                    alias: None,
                    meta: Meta::new(),
                }],
                alias: None,
                meta: Meta::new(),
            },
            Item::Const {
                name: "ORIGIN_X".into(),
                ty: Type::Primitive(Primitive::F64),
                value: Expr::FloatLiteral("0.0".into()),
                visibility: Visibility::Private,
                meta: Meta::new(),
            },
            Item::Struct {
                name: "Point".into(),
                visibility: Visibility::Private,
                fields: vec![
                    Field {
                        name: "x".into(),
                        ty: Type::Primitive(Primitive::F64),
                        visibility: Visibility::Public,
                        meta: Meta::new(),
                    },
                    Field {
                        name: "y".into(),
                        ty: Type::Primitive(Primitive::F64),
                        visibility: Visibility::Public,
                        meta: Meta::new(),
                    },
                ],
                attributes: vec![],
                meta: Meta::new(),
            },
            Item::Enum {
                name: "Shape".into(),
                visibility: Visibility::Private,
                variants: vec![
                    Variant {
                        name: "Empty".into(),
                        payload: VariantPayload::None,
                        meta: Meta::new(),
                    },
                    Variant {
                        name: "Circle".into(),
                        payload: VariantPayload::Tuple(vec![Type::Primitive(Primitive::F64)]),
                        meta: Meta::new(),
                    },
                    Variant {
                        name: "Rect".into(),
                        payload: VariantPayload::Struct(vec![
                            Field {
                                name: "w".into(),
                                ty: Type::Primitive(Primitive::F64),
                                visibility: Visibility::Public,
                                meta: Meta::new(),
                            },
                            Field {
                                name: "h".into(),
                                ty: Type::Primitive(Primitive::F64),
                                visibility: Visibility::Public,
                                meta: Meta::new(),
                            },
                        ]),
                        meta: Meta::new(),
                    },
                ],
                attributes: vec![],
                meta: Meta::new(),
            },
            Item::Function(Function {
                name: "area".into(),
                visibility: Visibility::Public,
                modifiers: vec![],
                params: vec![Param {
                    name: "s".into(),
                    ty: Type::Named("Shape".into()),
                    meta: Meta::new(),
                }],
                return_type: Type::Primitive(Primitive::F64),
                body: vec![Statement::Switch {
                    scrutinee: Expr::Ref("s".into()),
                    cases: vec![
                        SwitchCase::with_bindings(
                            Expr::Ref("Circle".into()),
                            vec![Statement::Return(Some(Expr::Ref("r".into())))],
                            CaseBindings::Positional(vec!["r".into()]),
                        ),
                        SwitchCase::with_bindings(
                            Expr::Ref("Rect".into()),
                            vec![Statement::Return(Some(Expr::Ref("w".into())))],
                            CaseBindings::Named(vec![
                                CaseFieldBind::shorthand("w"),
                                CaseFieldBind::shorthand("h"),
                            ]),
                        ),
                    ],
                    default: Some(vec![Statement::Return(Some(Expr::FloatLiteral(
                        "0.0".into(),
                    )))]),
                }],
                meta: Meta::new(),
            }),
        ],
    };
    assert_eq!(parsed, expected);
}

// ======================================================================
// Worked example from docs/source-syntax.md (minus the raw block)
// ======================================================================

#[test]
fn worked_example_parses_cleanly() {
    // The worked example from docs/source-syntax.md, verbatim save for the
    // `fast_sqrt` function (whose body is a `raw { … }` block — a deferred parse
    // error by design, so it is omitted here).
    let src = r#"
// geometry.mdl — a Lamina source module

use math::{ sqrt };

const ORIGIN_X: f64 = 0.0;

@equatable
@displayable
struct Point {
    x: f64,
    y: f64,
}

enum Shape {
    Empty,
    Circle(f64),
    Rect { w: f64, h: f64 },
}

public fn area(s: Shape): f64 {
    switch (s) {
        case Circle(r) { return 3.14159 * r ** 2; }
        case Rect(rect) { return rect.w * rect.h; }
        default { return 0.0; }
    }
}

public fn distance(a: Point, b: Point): f64 {
    let dx: f64 = a.x - b.x;
    let dy: f64 = a.y - b.y;
    return sqrt(dx * dx + dy * dy);
}

public fn scale_all(points: [Point], factor: f64): f64 {
    let total = 0.0;
    foreach p in points {
        total = total + p.x * factor;
    }
    for (let i = 0; i < 10; i = i + 1) {
        total = total + 1.0;
    }
    let f = (x: f64): f64 => { return x * 2.0; };
    return total;
}

public fn render(): void {
    node div(class = "box") {
        node p {
            text "Hello"
        }
    }
}
"#;
    let parsed = parse(src).expect("worked example parses");
    // use, const, struct, enum, area, distance, scale_all, render = 8 items.
    assert_eq!(parsed.items.len(), 8);
    // The last item is the tree-returning `render` function.
    match parsed.items.last().expect("render item") {
        Item::Function(f) => {
            assert_eq!(f.name, "render");
            assert!(matches!(f.body[0], Statement::Expr(Expr::Node { .. })));
        }
        other => panic!("expected render function, got {other:?}"),
    }
}

// ======================================================================
// Floor division (`~/`) and the `raw` escape hatch
// ======================================================================

#[test]
fn floordiv_tilde_slash_parses_at_multiplicative_precedence() {
    // `~/` is the Dart-style floor-division spelling, mapped to
    // `BinaryOp::FloorDiv` at the same (multiplicative) tier as `*` `/` `%`.
    assert_expr(
        "a ~/ b",
        Expr::Binary {
            op: BinaryOp::FloorDiv,
            lhs: Box::new(Expr::Ref("a".into())),
            rhs: Box::new(Expr::Ref("b".into())),
        },
    );
    // Multiplicative tier: `a + b ~/ c` groups as `a + (b ~/ c)`.
    assert_expr(
        "a + b ~/ c",
        Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("a".into())),
            rhs: Box::new(Expr::Binary {
                op: BinaryOp::FloorDiv,
                lhs: Box::new(Expr::Ref("b".into())),
                rhs: Box::new(Expr::Ref("c".into())),
            }),
        },
    );
}

#[test]
fn raw_single_arm_string_expr_parses() {
    // `raw <target> "string"` at expression position → a one-arm `Expr::Raw`
    // (no `else`), the string contents captured verbatim (unescaped).
    assert_body(
        "let x = raw rust \"foo.cast::<u32>()\";",
        vec![Statement::Let {
            name: "x".into(),
            ty: None,
            value: Some(Expr::Raw {
                arms: vec![RawArm {
                    target: "rust".into(),
                    version: None,
                    code: "foo.cast::<u32>()".into(),
                }],
                default: None,
                meta: Meta::new(),
            }),
        }],
    );
}

#[test]
fn raw_single_arm_string_with_version_parses() {
    // A version constraint is ACCEPTED and stored opaquely (not matched on).
    assert_body(
        "let x = raw rust >= 1.70 \"foo.cast::<u32>()\";",
        vec![Statement::Let {
            name: "x".into(),
            ty: None,
            value: Some(Expr::Raw {
                arms: vec![RawArm {
                    target: "rust".into(),
                    version: Some(">= 1.70".into()),
                    code: "foo.cast::<u32>()".into(),
                }],
                default: None,
                meta: Meta::new(),
            }),
        }],
    );
}

#[test]
fn raw_single_arm_block_item_parses() {
    // `raw <target> { verbatim-block }` at item position → a one-arm
    // `Item::Raw`; the brace-balanced body is captured verbatim (edges trimmed).
    assert_item(
        "raw rust {\n    macro_rules! id { ($x:expr) => { $x }; }\n}",
        Item::Raw {
            arms: vec![RawArm {
                target: "rust".into(),
                version: None,
                code: "macro_rules! id { ($x:expr) => { $x }; }".into(),
            }],
            default: None,
            meta: Meta::new(),
        },
    );
}

#[test]
fn raw_grouped_multi_arm_with_else_parses() {
    // The worked example's `fast_sqrt` body: a grouped `raw { … }` with three
    // target arms and an `else` fallback → the expected multi-arm `Statement::Raw`
    // AST (structural PartialEq). The brace-balanced arm bodies are verbatim.
    let src = r#"fn f(): void {
    raw {
        rust   { return x.sqrt(); }
        python { return math.sqrt(x) }
        else   { return x; }
    }
}"#;
    assert_item(
        src,
        func(
            "f",
            vec![Statement::Raw {
                arms: vec![
                    RawArm {
                        target: "rust".into(),
                        version: None,
                        code: "return x.sqrt();".into(),
                    },
                    RawArm {
                        target: "python".into(),
                        version: None,
                        code: "return math.sqrt(x)".into(),
                    },
                ],
                default: Some("return x;".into()),
                meta: Meta::new(),
            }],
        ),
    );
}

#[test]
fn raw_grouped_arm_with_version_parses() {
    // A grouped arm may carry a version constraint; it is stored, not matched.
    let src = r#"fn f(): void {
    raw {
        python >= 3.10 { result = (x := compute()) }
        python < 3.10  { result = compute() }
    }
}"#;
    assert_item(
        src,
        func(
            "f",
            vec![Statement::Raw {
                arms: vec![
                    RawArm {
                        target: "python".into(),
                        version: Some(">= 3.10".into()),
                        code: "result = (x := compute())".into(),
                    },
                    RawArm {
                        target: "python".into(),
                        version: Some("< 3.10".into()),
                        code: "result = compute()".into(),
                    },
                ],
                default: None,
                meta: Meta::new(),
            }],
        ),
    );
}

// ======================================================================
// Parse → emit through a synthetic fixture definition
// ======================================================================

/// A minimal synthetic Rust-ish fixture definition sufficient to emit a
/// function whose body is a `return <int>;`, proving the PARSED AST drives the
/// emitter (identical in spirit to `tests/transpile.rs`).
const FIXTURE_DEF: &str = concat!(
    "# Lamina Language Definition: fixture\n",
    "\n",
    "```lang-meta\nlamina-format: 0.0.0\ntarget: fixture\ntarget-version: 1\n```\n",
    "\n",
    "## Function\n",
    "\n",
    "```template\n",
    "{vis}fn {name}({params}){ret} {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### ret\n",
    "| When | Template |\n",
    "|------|----------|\n",
    "| else | \" -> {ret_type}\" |\n",
    "\n",
    "### vis\n",
    "| When          | Template |\n",
    "|---------------|----------|\n",
    "| vis is public | \"pub \" |\n",
    "| else          | \"\" |\n",
    "\n",
    "### param\n",
    "| When  | Template |\n",
    "|-------|----------|\n",
    "| first | \"{name}: {type}\" |\n",
    "| else  | \", {name}: {type}\" |\n",
    "\n",
    "### statement\n",
    "```template\n",
    "return {value};\n",
    "```\n",
    "\n",
    "### expr\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| expr is int | \"{value}\" |\n",
    "| else        | forbid |\n",
    "\n",
    "## Capabilities\n",
    "| Primitive | Action | Target |\n",
    "|-----------|--------|--------|\n",
    "| i32 | identity | i32 |\n",
    "| i8 | identity | i8 |\n",
    "| i16 | identity | i16 |\n",
    "| i64 | identity | i64 |\n",
    "| i128 | identity | i128 |\n",
    "| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n",
    "| u32 | identity | u32 |\n",
    "| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n",
    "| isize | identity | isize |\n",
    "| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n",
    "| bf16 | identity | bf16 |\n",
    "| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n",
    "| f128 | identity | f128 |\n",
    "| bool | identity | bool |\n",
    "| void | alias | () |\n",
    "| never | alias | ! |\n",
    "| byte | alias | u8 |\n",
    "| bytes | wrap | Vec |\n",
    "| char | identity | char |\n",
    "| str | wrap | String |\n",
    "| ptr | wrap | Ptr |\n",
    "| fnptr | wrap | Fn |\n",
);

#[test]
fn parse_then_emit_through_fixture_def() {
    let lang = parse_language_def(FIXTURE_DEF).expect("fixture def parses");
    // Parsed with the ratified colon-return + a parameter.
    let out =
        transpile("public fn answer(n: i32): i32 { return 42; }", &lang).expect("parse then emit");
    assert_eq!(out, "pub fn answer(n: i32) -> i32 {\n    return 42;\n}");
}

/// A fixture definition whose `target` is `fixture` and whose `### statement`
/// renders a raw statement verbatim — so a grouped `raw { … }` transpiled
/// through it emits exactly the `fixture` arm (proving parse → lower → emit).
const RAW_FIXTURE_DEF: &str = concat!(
    "# Lamina Language Definition: fixture\n",
    "\n",
    "```lang-meta\nlamina-format: 0.0.0\ntarget: fixture\ntarget-version: 1\n```\n",
    "\n",
    "## Function\n",
    "\n",
    "```template\n",
    "fn {name}() {{\n",
    "    {body}\n",
    "}}\n",
    "```\n",
    "\n",
    "### statement\n",
    "| When        | Template |\n",
    "|-------------|----------|\n",
    "| stmt is raw | \"{value}\" |\n",
    "| else        | forbid |\n",
    "\n",
    "## Capabilities\n",
    "| Primitive | Action | Target |\n",
    "|-----------|--------|--------|\n",
    "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
    "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
    "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
    "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
    "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
    "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
    "| void | identity | void |\n| never | identity | never |\n| byte | identity | byte |\n",
    "| bytes | identity | bytes |\n| char | identity | char |\n| str | identity | str |\n",
    "| ptr | forbid | |\n| fnptr | forbid | |\n",
);

#[test]
fn transpile_grouped_raw_emits_the_target_arm() {
    // The def's target is `fixture`; a grouped raw with a `fixture` arm plus
    // other arms must lower to the `fixture` arm and emit it verbatim.
    let lang = parse_language_def(RAW_FIXTURE_DEF).expect("raw fixture def parses");
    let src = r#"fn f(): void {
    raw {
        rust    { let r = x.sqrt(); }
        fixture { FIXTURE_LINE; }
        else    { fallback(); }
    }
}"#;
    let out = transpile(src, &lang).expect("parse then lower then emit");
    assert_eq!(out, "fn f() {\n    FIXTURE_LINE;\n}");
}

#[test]
fn transpile_raw_uses_else_when_no_arm_matches() {
    // No `fixture` arm, but an `else` fallback → the fallback is emitted.
    let lang = parse_language_def(RAW_FIXTURE_DEF).expect("raw fixture def parses");
    let src = r#"fn f(): void {
    raw {
        rust   { let r = x.sqrt(); }
        else   { FALLBACK_LINE; }
    }
}"#;
    let out = transpile(src, &lang).expect("parse then lower then emit");
    assert_eq!(out, "fn f() {\n    FALLBACK_LINE;\n}");
}

#[test]
fn transpile_raw_no_arm_no_else_is_a_lower_error() {
    // No `fixture` arm and no `else` → a hard lower error (never silently
    // dropped).
    let lang = parse_language_def(RAW_FIXTURE_DEF).expect("raw fixture def parses");
    let src = r#"fn f(): void {
    raw {
        rust   { let r = x.sqrt(); }
        python { r = x }
    }
}"#;
    let err = transpile(src, &lang).expect_err("no matching arm must fail");
    assert!(
        matches!(
            err,
            lamina_core::TranspileError::Lower(lamina_core::LowerError::NoRawArmForTarget { .. })
        ),
        "expected a NoRawArmForTarget lower error, got {err:?}"
    );
}
