//! End-to-end tests for the shipped `ruby.mdl` language definition.
//!
//! Ruby is a dynamically-typed, `end`-keyword-delimited target (no braces, no
//! type annotations), so these tests verify:
//!
//! * bare-parameter `def`/`end` signatures with an indented body,
//! * `if`/`while`/`each` (foreach) and `case`/`when` (switch) blocks closed by
//!   `end`,
//! * a `struct` → a `class` with `attr_accessor` + `initialize`, and its type
//!   attributes GENERATED as real Ruby methods (`==`, `to_s`, `<=>` +
//!   `include Comparable`, `hash`/`eql?`),
//! * a plain `enum` → a `module` of frozen symbol constants,
//! * a `lambda` → Ruby's stabby lambda `->(x) { … }` (inline, no hoisting),
//! * calls, operators (`**` native; `//`/`>>>` forbidden), and the `cast`
//!   forbidden.
//!
//! There is no concrete Lamina source syntax yet, so each test builds the AST
//! directly and transpiles it with the REAL `ruby.mdl` document shipped in
//! `lamina-defs`, asserting the EXACT emitted string. Every asserted output was
//! hand-verified to be valid, idiomatic Ruby 3.2 and additionally cross-checked
//! with `ruby -c` via [`assert_valid_ruby`] when an interpreter is present.

use std::path::PathBuf;

use lamina_core::ast::{
    BinaryOp, Expr, Field, FieldInit, File, Function, Item, Meta, Param, Primitive, Statement,
    SwitchCase, Type, TypeAttribute, Variant, VariantPayload, Visibility,
};
use lamina_core::emitter::emit;
use lamina_core::lang::LanguageDef;
use lamina_core::load_language_def;

fn ruby() -> LanguageDef {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // <repo> (lamina)
    path.pop(); // jd
    path.push("lamina-defs");
    path.push("languages");
    path.push("ruby.mdl");
    load_language_def(&path).unwrap_or_else(|e| panic!("shipped ruby.mdl should load: {e}"))
}

fn emit_ok(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang).unwrap_or_else(|e| panic!("emit failed: {e}"))
}

fn emit_err(items: Vec<Item>, lang: &LanguageDef) -> String {
    emit(&File { items }, lang)
        .expect_err("expected a forbidden-construct error")
        .to_string()
}

/// Cross-checks that `source` is syntactically valid Ruby by shelling out to
/// `ruby -c`. If `ruby` is not available on the host, the check is skipped (the
/// exact-string assertions are the primary proof). A `ruby` that IS present but
/// rejects the source fails the test loudly.
fn assert_valid_ruby(source: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = match Command::new("ruby")
        .arg("-c")
        .arg("-") // read program from stdin
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return, // no interpreter — skip
    };
    child
        .stdin
        .take()
        .expect("ruby stdin")
        .write_all(source.as_bytes())
        .expect("write source to ruby");
    let output = child.wait_with_output().expect("ruby to run");
    assert!(
        output.status.success(),
        "emitted Ruby failed `ruby -c`:\n{source}\n--- stderr ---\n{}",
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
fn shipped_ruby_def_loads() {
    let _ = ruby();
}

// ---- fn: bare params, no return type, end-delimited body ------------------

#[test]
fn simple_fn_renders_def_end() {
    // def add(a, b)
    //   return a + b
    // end
    let f = func(
        "add",
        vec![param("a", i32t()), param("b", i32t())],
        i32t(),
        vec![Statement::Return(Some(add(r("a"), r("b"))))],
    );
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def add(a, b)\n  return a + b\nend");
    assert_valid_ruby(&out);
}

#[test]
fn void_fn_renders_bare_return() {
    let f = func(
        "noop",
        vec![],
        Type::Primitive(Primitive::Void),
        vec![Statement::Return(None)],
    );
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def noop()\n  return\nend");
    assert_valid_ruby(&out);
}

// ---- nested blocks: while + if, closed by matching `end`s -----------------

#[test]
fn nested_blocks_close_with_ends() {
    // def f(n)
    //   while n > 0
    //     if n == 1
    //       return 1
    //     end
    //     n -= 1
    //   end
    //   return 0
    // end
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
    let expected = "def f(n)\n  while n > 0\n    if n == 1\n      return 1\n    end\n    n -= 1\n  end\n  return 0\nend";
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, expected);
    assert_valid_ruby(&out);
}

// ---- if / else ------------------------------------------------------------

#[test]
fn if_else_renders_single_end() {
    // if n == 0
    //   return 1
    // else
    //   return 2   (wrapped in a begin..end block, as an else-block statement)
    // end
    let stmt = Statement::If {
        cond: binary(BinaryOp::Eq, r("n"), int("0")),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: Some(Box::new(Statement::Block(vec![Statement::Return(Some(
            int("2"),
        ))]))),
    };
    let f = func("f", vec![param("n", i32t())], i32t(), vec![stmt]);
    let expected =
        "def f(n)\n  if n == 0\n    return 1\n  else\n    begin\n      return 2\n    end\n  end\nend";
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, expected);
    assert_valid_ruby(&out);
}

#[test]
fn plain_if_renders_indented_body() {
    let stmt = Statement::If {
        cond: r("flag"),
        then_block: vec![Statement::Return(Some(int("1")))],
        else_block: None,
    };
    let f = func("f", vec![param("flag", i32t())], i32t(), vec![stmt]);
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def f(flag)\n  if flag\n    return 1\n  end\nend");
    assert_valid_ruby(&out);
}

// ---- while ----------------------------------------------------------------

#[test]
fn while_renders_indented_body() {
    // while i < 10
    //   i += 1
    // end
    let step = Statement::assign(r("i"), add(r("i"), int("1"))).unwrap();
    let stmt = Statement::While {
        cond: binary(BinaryOp::Lt, r("i"), int("10")),
        body: vec![step],
    };
    let f = func(
        "loopf",
        vec![param("i", i32t())],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def loopf(i)\n  while i < 10\n    i += 1\n  end\nend");
    assert_valid_ruby(&out);
}

// ---- foreach -> .each do |x| ... end --------------------------------------

#[test]
fn foreach_renders_each_block() {
    // xs.each do |x|
    //   total += x
    // end
    let step = Statement::assign(r("total"), add(r("total"), r("x"))).unwrap();
    let stmt = Statement::ForEach {
        binding: "x".to_string(),
        iterable: r("xs"),
        body: vec![step],
    };
    let f = func(
        "sumf",
        vec![param("xs", i32t()), param("total", i32t())],
        Type::Primitive(Primitive::Void),
        vec![stmt],
    );
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(
        out,
        "def sumf(xs, total)\n  xs.each do |x|\n    total += x\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn counted_for_is_forbidden() {
    // Ruby has no C-style counted `for`; it is forbidden (a layer lowers it to a
    // `while` or a range `each`).
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
        emit_err(vec![f], &ruby()).contains("forbid"),
        "C-style counted for should be forbidden in Ruby"
    );
}

// ---- case / when (switch) -------------------------------------------------

#[test]
fn switch_renders_as_case_when() {
    // case n
    // when 0
    //   return 1
    // when 1
    //   return 2
    // else
    //   return 0
    // end
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
    let expected = "def f(n)\n  case n\n  when 0\n    return 1\n  when 1\n    return 2\n  else\n    return 0\n  end\nend";
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, expected);
    assert_valid_ruby(&out);
}

// ---- let / assignment -----------------------------------------------------

#[test]
fn let_bindings_render_plain_assignment() {
    // Both typed and untyped `let` render `name = value` (Ruby is dynamically
    // typed, no local annotation).
    let untyped = Statement::Let {
        name: "x".to_string(),
        ty: None,
        value: Some(int("5")),
    };
    let f = func("g", vec![], Type::Primitive(Primitive::Void), vec![untyped]);
    assert_eq!(emit_ok(vec![f], &ruby()), "def g()\n  x = 5\nend");

    let typed = Statement::Let {
        name: "y".to_string(),
        ty: Some(i32t()),
        value: Some(int("7")),
    };
    let f = func("h", vec![], Type::Primitive(Primitive::Void), vec![typed]);
    assert_eq!(emit_ok(vec![f], &ruby()), "def h()\n  y = 7\nend");
}

// ---- struct -> class + attr_accessor + initialize -------------------------

#[test]
fn struct_renders_as_class() {
    // class Point
    //   attr_accessor :x, :y
    //
    //   def initialize(x, y)
    //     @x = x
    //     @y = y
    //   end
    // end
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class Point\n  attr_accessor :x, :y\n\n  def initialize(x, y)\n    @x = x\n    @y = y\n  end\nend"
    );
    assert_valid_ruby(&out);
}

// ---- struct type attributes -> generated Ruby methods ---------------------

#[test]
fn equatable_struct_generates_eq_method() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![TypeAttribute::Equatable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class Point\n  attr_accessor :x, :y\n\n  def initialize(x, y)\n    @x = x\n    @y = y\n  end\n\n  def ==(other)\n    @x == other.x && @y == other.y\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn displayable_struct_generates_to_s_method() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class Point\n  attr_accessor :x, :y\n\n  def initialize(x, y)\n    @x = x\n    @y = y\n  end\n\n  def to_s\n    \"Point(x=#{@x}, y=#{@y})\"\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn comparable_struct_generates_spaceship_and_include() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("a", i32t()), field("b", i32t())],
        attributes: vec![TypeAttribute::Comparable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class P\n  attr_accessor :a, :b\n\n  def initialize(a, b)\n    @a = a\n    @b = b\n  end\n\n  include Comparable\n\n  def <=>(other)\n    (@a <=> other.a).nonzero? || (@b <=> other.b)\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn hashable_struct_generates_hash_and_eql() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Hashable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class P\n  attr_accessor :x\n\n  def initialize(x)\n    @x = x\n  end\n\n  def hash\n    [@x].hash\n  end\n\n  def eql?(other)\n    self.class == other.class && hash == other.hash\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn copyable_attribute_is_inherent() {
    // `copyable` is inherent in Ruby (Object#dup/clone), so it emits no method —
    // byte-identical to an attribute-free class.
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Copyable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class P\n  attr_accessor :x\n\n  def initialize(x)\n    @x = x\n  end\nend"
    );
    assert_valid_ruby(&out);
}

#[test]
fn multiple_attributes_generate_multiple_methods() {
    let s = Item::Struct {
        name: "Point".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t()), field("y", i32t())],
        attributes: vec![TypeAttribute::Equatable, TypeAttribute::Displayable],
        meta: Meta::new(),
    };
    let out = emit_ok(vec![s], &ruby());
    assert_eq!(
        out,
        "class Point\n  attr_accessor :x, :y\n\n  def initialize(x, y)\n    @x = x\n    @y = y\n  end\n\n  def ==(other)\n    @x == other.x && @y == other.y\n  end\n\n  def to_s\n    \"Point(x=#{@x}, y=#{@y})\"\n  end\nend"
    );
    assert_valid_ruby(&out);
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
        emit_err(vec![s], &ruby()).contains("forbid"),
        "cloneable should be forbidden in Ruby (Object#clone is shallow)"
    );
}

#[test]
fn iterable_attribute_is_forbidden() {
    let s = Item::Struct {
        name: "P".to_string(),
        visibility: Visibility::Public,
        fields: vec![field("x", i32t())],
        attributes: vec![TypeAttribute::Iterable],
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![s], &ruby()).contains("forbid"),
        "iterable should be forbidden in Ruby (a value object is not a collection)"
    );
}

// ---- enum -> module of symbol constants -----------------------------------

#[test]
fn plain_enum_renders_as_symbol_module() {
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
    let out = emit_ok(vec![e], &ruby());
    assert_eq!(
        out,
        "module Color\n  RED = :RED\n  GREEN = :GREEN\n  BLUE = :BLUE\nend"
    );
    assert_valid_ruby(&out);
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
        emit_err(vec![e], &ruby()).contains("forbid"),
        "enum payloads should be forbidden in Ruby (no native sum type)"
    );
}

// ---- function call --------------------------------------------------------

#[test]
fn call_renders_as_ruby_call() {
    // return f(x, y + 1)  (a compound arg is parenthesized by the engine)
    let call = Expr::Call {
        callee: Box::new(r("f")),
        args: vec![r("x"), add(r("y"), int("1"))],
    };
    let f = func("g", vec![], i32t(), vec![Statement::Return(Some(call))]);
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def g()\n  return f(x, (y + 1))\nend");
    assert_valid_ruby(&out);
}

// ---- operators: ** (Ruby HAS this) ----------------------------------------

#[test]
fn exponent_operator_maps_to_double_star() {
    let expr = binary(BinaryOp::Pow, r("a"), r("b"));
    let f = func("p", vec![param("a", i32t()), param("b", i32t())], i32t(), vec![Statement::Return(Some(expr))]);
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def p(a, b)\n  return a ** b\nend");
    assert_valid_ruby(&out);
}

#[test]
fn floor_division_operator_is_forbidden() {
    let expr = binary(BinaryOp::FloorDiv, r("a"), r("b"));
    let f = func("d", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert!(
        emit_err(vec![f], &ruby()).contains("forbid"),
        "floor division `//` should be forbidden in Ruby"
    );
}

#[test]
fn unsigned_right_shift_is_forbidden() {
    let expr = binary(BinaryOp::UShr, r("a"), r("b"));
    let f = func("s", vec![], i32t(), vec![Statement::Return(Some(expr))]);
    assert!(
        emit_err(vec![f], &ruby()).contains("forbid"),
        "the `ushr`/`>>>` operator should be forbidden in Ruby"
    );
}

// ---- cast: forbidden (Ruby has no C-style cast) ---------------------------

#[test]
fn cast_is_forbidden() {
    let cast = Expr::Cast {
        value: Box::new(r("x")),
        ty: Type::Primitive(Primitive::I64),
    };
    let f = func("c", vec![], i32t(), vec![Statement::Return(Some(cast))]);
    assert!(
        emit_err(vec![f], &ruby()).contains("forbid"),
        "a C-style cast should be forbidden in Ruby"
    );
}

// ---- lambda -> stabby lambda ----------------------------------------------

#[test]
fn single_expression_lambda_renders_stabby() {
    // g = ->(x) {
    //   x + 1
    // }
    let lambda = Expr::Lambda {
        params: vec![param("x", i32t())],
        return_type: None,
        body: vec![Statement::Expr(add(r("x"), int("1")))],
        meta: Meta::new(),
    };
    let body = vec![Statement::Let {
        name: "g".to_string(),
        ty: None,
        value: Some(lambda),
    }];
    let f = func("mk", vec![], Type::Primitive(Primitive::Void), body);
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def mk()\n  g = ->(x) {\n    x + 1\n  }\nend");
    assert_valid_ruby(&out);
}

#[test]
fn multi_statement_lambda_renders_inline() {
    // Ruby lambdas hold a full statement sequence, so a multi-statement lambda
    // renders INLINE (no hoisting, unlike Python/C).
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
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(
        out,
        "def mk()\n  g = ->(x) {\n    y = x + 1\n    return y\n  }\nend"
    );
    assert_valid_ruby(&out);
}

// ---- struct literal -> Type.new(args) -------------------------------------

#[test]
fn struct_literal_renders_as_new_call() {
    // return Point.new(1, 2)
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
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def mk()\n  return Point.new(1, 2)\nend");
    assert_valid_ruby(&out);
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
        vec![param("arr", i32t())],
        i32t(),
        vec![Statement::Expr(arr), Statement::Return(Some(idx))],
    );
    let out = emit_ok(vec![f], &ruby());
    assert_eq!(out, "def arrf(arr)\n  [10, 20, 30]\n  return arr[1]\nend");
    assert_valid_ruby(&out);
}

// ---- items: typedef / const / use -----------------------------------------

#[test]
fn typedef_const_and_use_render() {
    let td = Item::TypeDef {
        name: "Id".to_string(),
        target: i32t(),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![td], &ruby()), "Id = Integer");

    let konst = Item::Const {
        name: "ANSWER".to_string(),
        ty: i32t(),
        value: int("42"),
        visibility: Visibility::Public,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![konst], &ruby()), "ANSWER = 42");

    let bare = Item::Use {
        path: "json".to_string(),
        items: vec![],
        alias: None,
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![bare], &ruby()), "require \"json\"");
}

#[test]
fn selective_and_aliased_use_are_forbidden() {
    let selective = Item::Use {
        path: "set".to_string(),
        items: vec![lamina_core::ast::UseItem {
            name: "Set".to_string(),
            alias: None,
            meta: Meta::new(),
        }],
        alias: None,
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![selective], &ruby()).contains("forbid"),
        "selective require should be forbidden in Ruby"
    );

    let aliased = Item::Use {
        path: "set".to_string(),
        items: vec![],
        alias: Some("s".to_string()),
        meta: Meta::new(),
    };
    assert!(
        emit_err(vec![aliased], &ruby()).contains("forbid"),
        "aliased require should be forbidden in Ruby"
    );
}

// ---- multiple items: blank-line separated ---------------------------------

#[test]
fn multiple_items_are_blank_line_separated() {
    let e = Item::Enum {
        name: "Color".to_string(),
        visibility: Visibility::Public,
        variants: vec![Variant {
            name: "RED".to_string(),
            payload: VariantPayload::None,
            meta: Meta::new(),
        }],
        attributes: vec![],
        meta: Meta::new(),
    };
    let f = func("f", vec![], i32t(), vec![Statement::Return(Some(int("0")))]);
    let out = emit_ok(vec![e, f], &ruby());
    assert_eq!(
        out,
        "module Color\n  RED = :RED\nend\n\ndef f()\n  return 0\nend"
    );
    assert_valid_ruby(&out);
}

// ---- raw item passes through unchanged ------------------------------------

#[test]
fn raw_item_passes_through() {
    let raw = Item::Raw {
        code: "# hand-written\nputs 'hi'".to_string(),
        meta: Meta::new(),
    };
    assert_eq!(emit_ok(vec![raw], &ruby()), "# hand-written\nputs 'hi'");
}
