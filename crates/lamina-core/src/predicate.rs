//! The `When` predicate model.
//!
//! Language definitions branch by way of a **logic table**, not a scripting
//! language. Each `When` row carries a predicate; the engine evaluates rows
//! top-to-bottom and the first whose predicate holds selects that row's
//! template (`else` always holds). This gives branching without
//! Turing-completeness.
//!
//! The predicate grammar is deliberately tiny and **closed**: a fixed registry
//! of *facts* about the node being rendered and its inherited render context,
//! combined only with `&&`, `||`, `!`, and parentheses. A language file cannot
//! invent a new fact — it may only use the ones the engine defines. If a new
//! distinction is needed, a fact is added to the engine's registry, never to
//! the predicate language.
//!
//! ```text
//! when      := "else" | or_expr
//! or_expr   := and_expr ("||" and_expr)*
//! and_expr  := unary ("&&" unary)*
//! unary     := "!"? atom
//! atom      := fact | "(" or_expr ")"
//! fact      := IDENT ("is" IDENT)?
//! ```
//!
//! A bare `IDENT` is a boolean fact (e.g. `export`, `async`). `IDENT is IDENT`
//! is an enum query (e.g. `ret is void`, `vis is public`, `caller is async`).

use crate::error::PredicateError;

/// A parsed `When` predicate: either the catch-all `else` or a boolean
/// combination of facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    /// The catch-all row; always matches. Must be last in a table.
    Else,
    /// A boolean expression over facts.
    Expr(PredExpr),
}

/// A boolean expression over facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PredExpr {
    /// A single fact query.
    Fact(Fact),
    /// Logical NOT.
    Not(Box<PredExpr>),
    /// Logical AND.
    And(Box<PredExpr>, Box<PredExpr>),
    /// Logical OR.
    Or(Box<PredExpr>, Box<PredExpr>),
}

/// A single fact: a boolean flag or an enum query (`key is value`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// The fact key (e.g. `export`, `ret`, `vis`, `caller`).
    pub key: String,
    /// For an enum query, the value being tested (e.g. `void` in `ret is
    /// void`). `None` for a boolean flag.
    pub value: Option<String>,
}

/// The set of facts the engine can answer about the thing being rendered.
///
/// This is the **closed** registry: the evaluator answers a [`Fact`] by
/// consulting the fields here. A language file cannot add to it. `bool` flags
/// answer boolean facts; the enum fields answer `key is value` queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderContext {
    /// `export` — the node is publicly visible.
    pub export: bool,
    /// Modifier flags (answer boolean facts of the same name).
    pub is_async: bool,
    /// `const`.
    pub is_const: bool,
    /// `unsafe`.
    pub is_unsafe: bool,
    /// `throws`.
    pub is_throws: bool,
    /// `extern`.
    pub is_extern: bool,
    /// `inline`.
    pub is_inline: bool,
    /// `generator`.
    pub is_generator: bool,
    /// Answers `ret is void|never|type`.
    pub ret: Option<RetKind>,
    /// Answers `vis is public|protected|private`.
    pub vis: Option<VisKind>,
    /// Answers `caller is async|sync` — an *inherited* fact threaded down when
    /// rendering a child (e.g. a statement inside an async function).
    pub caller: Option<CallerKind>,
    /// `first` — this element is the first in a collection being looped
    /// (`params`, `body`). Lets an item template render its own leading
    /// separator conditionally instead of the engine joining with a fixed one.
    pub first: bool,
    /// `last` — this element is the last in a collection being looped.
    pub last: bool,
    /// Answers `expr is int|float|bool|string|char|null|ref|field|index|call|
    /// unary|binary` — the dispatch kind of the expression being rendered.
    /// `None` when the node being rendered is not an expression.
    pub expr: Option<ExprKind>,
    /// Answers `stmt is block|let|return|if|while|for|foreach|switch|break|
    /// continue|expr` — the dispatch kind of the statement being rendered.
    /// `None` when the node being rendered is not a statement.
    pub stmt: Option<StmtKind>,
    /// Answers `item is function|struct|enum|typedef|const|use` — the dispatch
    /// kind of the top-level item being rendered. `None` when the node being
    /// rendered is not a top-level item.
    pub item: Option<ItemKind>,
    /// `has_value` — the statement being rendered has a value sub-part (a `let`
    /// with an initializer, or a `return` with a returned expression). Lets the
    /// `### statement` row for `let`/`return` render `{value}` conditionally.
    pub has_value: bool,
    /// `has_type` — a `let` statement carries an explicit type annotation.
    pub has_type: bool,
    /// `has_else` — an `if` statement carries an `else` branch.
    pub has_else: bool,
    /// `has_init` — a counted `for` loop carries an initializer.
    pub has_init: bool,
    /// `has_cond` — a counted `for` loop carries a continuation condition.
    pub has_cond: bool,
    /// `has_step` — a counted `for` loop carries a step statement.
    pub has_step: bool,
    /// `has_default` — a `switch` carries a `default` branch.
    pub has_default: bool,
}

/// The return-shape a node has, for `ret is ...` queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetKind {
    /// Returns nothing (`void`).
    Void,
    /// Never returns (`never`).
    Never,
    /// Returns an ordinary value type.
    Type,
}

/// A visibility level, for `vis is ...` queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisKind {
    /// `public`.
    Public,
    /// `protected`.
    Protected,
    /// `private`.
    Private,
}

/// The enclosing caller's synchrony, for `caller is ...` queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerKind {
    /// The enclosing callable is async.
    Async,
    /// The enclosing callable is synchronous.
    Sync,
}

/// The dispatch kind of an expression, for `expr is ...` queries.
///
/// This is the closed set of expression kinds a language definition may branch
/// on in its `### expr` `When`-table. It mirrors the engine's `Expr` variants;
/// the emitter maps each `Expr` to its [`ExprKind`] when building the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    /// An integer literal (`expr is int`).
    Int,
    /// A float literal (`expr is float`).
    Float,
    /// A boolean literal (`expr is bool`).
    Bool,
    /// A string literal (`expr is string`).
    String,
    /// A character literal (`expr is char`).
    Char,
    /// The `null` literal (`expr is null`).
    Null,
    /// A variable reference (`expr is ref`).
    Ref,
    /// Field access (`expr is field`).
    Field,
    /// Indexing (`expr is index`).
    Index,
    /// A call (`expr is call`).
    Call,
    /// A unary operator expression (`expr is unary`).
    Unary,
    /// A binary operator expression (`expr is binary`).
    Binary,
}

impl ExprKind {
    /// The `expr is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            ExprKind::Int => "int",
            ExprKind::Float => "float",
            ExprKind::Bool => "bool",
            ExprKind::String => "string",
            ExprKind::Char => "char",
            ExprKind::Null => "null",
            ExprKind::Ref => "ref",
            ExprKind::Field => "field",
            ExprKind::Index => "index",
            ExprKind::Call => "call",
            ExprKind::Unary => "unary",
            ExprKind::Binary => "binary",
        }
    }
}

/// The dispatch kind of a statement, for `stmt is ...` queries.
///
/// This is the closed set of statement kinds a language definition may branch
/// on in its `### statement` `When`-table. It mirrors the engine's `Statement`
/// variants; the emitter maps each `Statement` to its [`StmtKind`] when
/// building the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StmtKind {
    /// A nested block (`stmt is block`).
    Block,
    /// A local binding (`stmt is let`).
    Let,
    /// A return (`stmt is return`).
    Return,
    /// An `if`/`else` (`stmt is if`).
    If,
    /// A `while` loop (`stmt is while`).
    While,
    /// A counted `for` loop (`stmt is for`).
    For,
    /// A `foreach` iterator loop (`stmt is foreach`).
    ForEach,
    /// A `switch` (`stmt is switch`).
    Switch,
    /// A `break` (`stmt is break`).
    Break,
    /// A `continue` (`stmt is continue`).
    Continue,
    /// An expression-statement (`stmt is expr`).
    Expr,
}

impl StmtKind {
    /// The `stmt is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            StmtKind::Block => "block",
            StmtKind::Let => "let",
            StmtKind::Return => "return",
            StmtKind::If => "if",
            StmtKind::While => "while",
            StmtKind::For => "for",
            StmtKind::ForEach => "foreach",
            StmtKind::Switch => "switch",
            StmtKind::Break => "break",
            StmtKind::Continue => "continue",
            StmtKind::Expr => "expr",
        }
    }
}

/// The dispatch kind of a top-level item, for `item is ...` queries.
///
/// This is the closed set of item kinds a language definition may branch on. It
/// mirrors the engine's `Item` variants; the emitter maps each `Item` to its
/// [`ItemKind`] when building the context, and dispatches the item to the
/// target's matching `## <Item>` section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// A function (`item is function`).
    Function,
    /// A `struct` (`item is struct`).
    Struct,
    /// An `enum` (`item is enum`).
    Enum,
    /// A type alias (`item is typedef`).
    TypeDef,
    /// An item-level constant (`item is const`).
    Const,
    /// A `use` import (`item is use`).
    Use,
}

impl ItemKind {
    /// The `item is <kind>` value spelling for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Function => "function",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::TypeDef => "typedef",
            ItemKind::Const => "const",
            ItemKind::Use => "use",
        }
    }
}

impl RenderContext {
    /// Evaluates a predicate against this context.
    pub fn eval(&self, predicate: &Predicate) -> bool {
        match predicate {
            Predicate::Else => true,
            Predicate::Expr(expr) => self.eval_expr(expr),
        }
    }

    fn eval_expr(&self, expr: &PredExpr) -> bool {
        match expr {
            PredExpr::Fact(fact) => self.eval_fact(fact),
            PredExpr::Not(inner) => !self.eval_expr(inner),
            PredExpr::And(a, b) => self.eval_expr(a) && self.eval_expr(b),
            PredExpr::Or(a, b) => self.eval_expr(a) || self.eval_expr(b),
        }
    }

    /// Answers a single fact from the closed registry. An unknown key or an
    /// unknown enum value evaluates to `false` (predicates are validated at
    /// parse time via [`validate_fact`], so this is a defensive fallback).
    fn eval_fact(&self, fact: &Fact) -> bool {
        match (fact.key.as_str(), fact.value.as_deref()) {
            // Boolean flags.
            ("export", None) => self.export,
            ("async", None) => self.is_async,
            ("const", None) => self.is_const,
            ("unsafe", None) => self.is_unsafe,
            ("throws", None) => self.is_throws,
            ("extern", None) => self.is_extern,
            ("inline", None) => self.is_inline,
            ("generator", None) => self.is_generator,
            ("first", None) => self.first,
            ("last", None) => self.last,
            // Optional statement sub-part flags.
            ("has_value", None) => self.has_value,
            ("has_type", None) => self.has_type,
            ("has_else", None) => self.has_else,
            ("has_init", None) => self.has_init,
            ("has_cond", None) => self.has_cond,
            ("has_step", None) => self.has_step,
            ("has_default", None) => self.has_default,
            // Enum queries.
            ("ret", Some("void")) => self.ret == Some(RetKind::Void),
            ("ret", Some("never")) => self.ret == Some(RetKind::Never),
            ("ret", Some("type")) => self.ret == Some(RetKind::Type),
            ("vis", Some("public")) => self.vis == Some(VisKind::Public),
            ("vis", Some("protected")) => self.vis == Some(VisKind::Protected),
            ("vis", Some("private")) => self.vis == Some(VisKind::Private),
            ("caller", Some("async")) => self.caller == Some(CallerKind::Async),
            ("caller", Some("sync")) => self.caller == Some(CallerKind::Sync),
            // Expression dispatch: `expr is <kind>`. The value must be a known
            // kind spelling and match the expression currently being rendered.
            ("expr", Some(kind)) => {
                self.expr.map(|k| k.as_str() == kind).unwrap_or(false)
                    && expr_kind_is_known(kind)
            }
            // Statement dispatch: `stmt is <kind>`. The value must be a known
            // kind spelling and match the statement currently being rendered.
            ("stmt", Some(kind)) => {
                self.stmt.map(|k| k.as_str() == kind).unwrap_or(false)
                    && stmt_kind_is_known(kind)
            }
            // Item dispatch: `item is <kind>`. The value must be a known kind
            // spelling and match the item currently being rendered.
            ("item", Some(kind)) => {
                self.item.map(|k| k.as_str() == kind).unwrap_or(false)
                    && item_kind_is_known(kind)
            }
            _ => false,
        }
    }
}

/// Returns `true` if `kind` is a known `expr is <kind>` value spelling. Keeps
/// the closed `expr` fact vocabulary in one place, shared by [`RenderContext::
/// eval_fact`] and [`validate_fact`].
fn expr_kind_is_known(kind: &str) -> bool {
    matches!(
        kind,
        "int" | "float"
            | "bool"
            | "string"
            | "char"
            | "null"
            | "ref"
            | "field"
            | "index"
            | "call"
            | "unary"
            | "binary"
    )
}

/// Returns `true` if `kind` is a known `stmt is <kind>` value spelling. Keeps
/// the closed `stmt` fact vocabulary in one place, shared by [`RenderContext::
/// eval_fact`] and [`validate_fact`].
fn stmt_kind_is_known(kind: &str) -> bool {
    matches!(
        kind,
        "block"
            | "let"
            | "return"
            | "if"
            | "while"
            | "for"
            | "foreach"
            | "switch"
            | "break"
            | "continue"
            | "expr"
    )
}

/// Returns `true` if `kind` is a known `item is <kind>` value spelling. Keeps
/// the closed `item` fact vocabulary in one place, shared by [`RenderContext::
/// eval_fact`] and [`validate_fact`].
fn item_kind_is_known(kind: &str) -> bool {
    matches!(
        kind,
        "function" | "struct" | "enum" | "typedef" | "const" | "use"
    )
}

/// Validates that a fact is part of the closed registry. Used at parse time so
/// a malformed `When` predicate fails loudly rather than silently evaluating to
/// `false`.
fn validate_fact(fact: &Fact) -> Result<(), PredicateError> {
    let known = matches!(
        (fact.key.as_str(), fact.value.as_deref()),
        ("export", None)
            | ("async", None)
            | ("const", None)
            | ("unsafe", None)
            | ("throws", None)
            | ("extern", None)
            | ("inline", None)
            | ("generator", None)
            | ("first", None)
            | ("last", None)
            | ("has_value", None)
            | ("has_type", None)
            | ("has_else", None)
            | ("has_init", None)
            | ("has_cond", None)
            | ("has_step", None)
            | ("has_default", None)
            | ("ret", Some("void"))
            | ("ret", Some("never"))
            | ("ret", Some("type"))
            | ("vis", Some("public"))
            | ("vis", Some("protected"))
            | ("vis", Some("private"))
            | ("caller", Some("async"))
            | ("caller", Some("sync"))
    );
    // `expr is <kind>` is validated against the closed kind vocabulary.
    let known = known || matches!((fact.key.as_str(), fact.value.as_deref()), ("expr", Some(k)) if expr_kind_is_known(k));
    // `stmt is <kind>` is validated against the closed statement vocabulary.
    let known = known || matches!((fact.key.as_str(), fact.value.as_deref()), ("stmt", Some(k)) if stmt_kind_is_known(k));
    // `item is <kind>` is validated against the closed item vocabulary.
    let known = known || matches!((fact.key.as_str(), fact.value.as_deref()), ("item", Some(k)) if item_kind_is_known(k));
    if known {
        Ok(())
    } else {
        Err(PredicateError::UnknownFact {
            fact: match &fact.value {
                Some(v) => format!("{} is {}", fact.key, v),
                None => fact.key.clone(),
            },
        })
    }
}

/// Parses a `When`-cell string into a [`Predicate`], validating every fact
/// against the closed registry.
///
/// # Errors
///
/// Returns a [`PredicateError`] on a syntax error or an unknown fact.
pub fn parse_predicate(src: &str) -> Result<Predicate, PredicateError> {
    let trimmed = src.trim();
    if trimmed == "else" {
        return Ok(Predicate::Else);
    }
    let tokens = lex_predicate(trimmed)?;
    let mut parser = PredParser { tokens, pos: 0 };
    let expr = parser.parse_or()?;
    if parser.pos != parser.tokens.len() {
        return Err(PredicateError::Syntax {
            detail: format!("unexpected trailing tokens in {src:?}"),
        });
    }
    Ok(Predicate::Expr(expr))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PTok {
    Ident(String),
    Is,
    And,
    Or,
    Not,
    LParen,
    RParen,
}

fn lex_predicate(src: &str) -> Result<Vec<PTok>, PredicateError> {
    let mut tokens = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                tokens.push(PTok::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(PTok::RParen);
                i += 1;
            }
            '!' => {
                tokens.push(PTok::Not);
                i += 1;
            }
            '&' if i + 1 < bytes.len() && bytes[i + 1] == b'&' => {
                tokens.push(PTok::And);
                i += 2;
            }
            '|' if i + 1 < bytes.len() && bytes[i + 1] == b'|' => {
                tokens.push(PTok::Or);
                i += 2;
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < bytes.len() {
                    let ch = bytes[i] as char;
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let word = &src[start..i];
                match word {
                    "is" => tokens.push(PTok::Is),
                    _ => tokens.push(PTok::Ident(word.to_string())),
                }
            }
            _ => {
                return Err(PredicateError::Syntax {
                    detail: format!("unexpected character {c:?} in predicate"),
                })
            }
        }
    }
    Ok(tokens)
}

struct PredParser {
    tokens: Vec<PTok>,
    pos: usize,
}

impl PredParser {
    fn peek(&self) -> Option<&PTok> {
        self.tokens.get(self.pos)
    }

    fn parse_or(&mut self) -> Result<PredExpr, PredicateError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&PTok::Or) {
            self.pos += 1;
            let right = self.parse_and()?;
            left = PredExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<PredExpr, PredicateError> {
        let mut left = self.parse_unary()?;
        while self.peek() == Some(&PTok::And) {
            self.pos += 1;
            let right = self.parse_unary()?;
            left = PredExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<PredExpr, PredicateError> {
        if self.peek() == Some(&PTok::Not) {
            self.pos += 1;
            let inner = self.parse_unary()?;
            return Ok(PredExpr::Not(Box::new(inner)));
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<PredExpr, PredicateError> {
        match self.peek().cloned() {
            Some(PTok::LParen) => {
                self.pos += 1;
                let inner = self.parse_or()?;
                if self.peek() != Some(&PTok::RParen) {
                    return Err(PredicateError::Syntax {
                        detail: "expected `)`".to_string(),
                    });
                }
                self.pos += 1;
                Ok(inner)
            }
            Some(PTok::Ident(key)) => {
                self.pos += 1;
                // Optional `is IDENT`.
                let value = if self.peek() == Some(&PTok::Is) {
                    self.pos += 1;
                    match self.peek().cloned() {
                        Some(PTok::Ident(v)) => {
                            self.pos += 1;
                            Some(v)
                        }
                        _ => {
                            return Err(PredicateError::Syntax {
                                detail: format!("expected a value after `{key} is`"),
                            })
                        }
                    }
                } else {
                    None
                };
                let fact = Fact { key, value };
                validate_fact(&fact)?;
                Ok(PredExpr::Fact(fact))
            }
            _ => Err(PredicateError::Syntax {
                detail: "expected a fact or `(`".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> RenderContext {
        RenderContext::default()
    }

    #[test]
    fn else_always_matches() {
        assert!(ctx().eval(&parse_predicate("else").expect("parse")));
    }

    #[test]
    fn boolean_fact() {
        let p = parse_predicate("export").expect("parse");
        assert!(!ctx().eval(&p));
        let c = RenderContext {
            export: true,
            ..Default::default()
        };
        assert!(c.eval(&p));
    }

    #[test]
    fn enum_query() {
        let p = parse_predicate("ret is void").expect("parse");
        let c = RenderContext {
            ret: Some(RetKind::Void),
            ..Default::default()
        };
        assert!(c.eval(&p));
        let c2 = RenderContext {
            ret: Some(RetKind::Type),
            ..Default::default()
        };
        assert!(!c2.eval(&p));
    }

    #[test]
    fn and_or_not_and_precedence() {
        // `export && ret is void` — AND binds both facts.
        let p = parse_predicate("export && ret is void").expect("parse");
        let c = RenderContext {
            export: true,
            ret: Some(RetKind::Void),
            ..Default::default()
        };
        assert!(c.eval(&p));
        let c2 = RenderContext {
            export: false,
            ret: Some(RetKind::Void),
            ..Default::default()
        };
        assert!(!c2.eval(&p));

        // `!export || ret is never` — OR is looser than AND/NOT.
        let p2 = parse_predicate("!export || ret is never").expect("parse");
        assert!(c2.eval(&p2)); // !export is true
    }

    #[test]
    fn parentheses_group() {
        let p = parse_predicate("(export || async) && ret is type").expect("parse");
        let c = RenderContext {
            is_async: true,
            ret: Some(RetKind::Type),
            ..Default::default()
        };
        assert!(c.eval(&p));
    }

    #[test]
    fn inherited_caller_fact() {
        let p = parse_predicate("caller is async").expect("parse");
        let c = RenderContext {
            caller: Some(CallerKind::Async),
            ..Default::default()
        };
        assert!(c.eval(&p));
    }

    #[test]
    fn loop_facts_first_and_last() {
        let first = parse_predicate("first").expect("parse");
        let last = parse_predicate("last").expect("parse");
        let c = RenderContext {
            first: true,
            last: false,
            ..Default::default()
        };
        assert!(c.eval(&first));
        assert!(!c.eval(&last));
        // `!first` is how a non-first element renders its leading separator.
        let not_first = parse_predicate("!first").expect("parse");
        let mid = RenderContext {
            first: false,
            ..Default::default()
        };
        assert!(mid.eval(&not_first));
    }

    #[test]
    fn rejects_unknown_fact() {
        let err = parse_predicate("frobnicate").expect_err("unknown");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }

    #[test]
    fn rejects_unknown_enum_value() {
        let err = parse_predicate("ret is banana").expect_err("unknown value");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }

    #[test]
    fn rejects_syntax_error() {
        assert!(parse_predicate("export &&").is_err());
        assert!(parse_predicate("(export").is_err());
    }

    #[test]
    fn expr_kind_dispatch_fact() {
        let p = parse_predicate("expr is binary").expect("parse");
        let c = RenderContext {
            expr: Some(ExprKind::Binary),
            ..Default::default()
        };
        assert!(c.eval(&p));
        let c2 = RenderContext {
            expr: Some(ExprKind::Call),
            ..Default::default()
        };
        assert!(!c2.eval(&p));
        // No expression in context -> the fact does not hold.
        assert!(!ctx().eval(&p));
    }

    #[test]
    fn expr_kind_all_values_parse() {
        for kind in [
            "int", "float", "bool", "string", "char", "null", "ref", "field", "index", "call",
            "unary", "binary",
        ] {
            parse_predicate(&format!("expr is {kind}"))
                .unwrap_or_else(|e| panic!("`expr is {kind}` should parse: {e:?}"));
        }
    }

    #[test]
    fn rejects_unknown_expr_kind() {
        let err = parse_predicate("expr is lambda").expect_err("unknown expr kind");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }

    #[test]
    fn stmt_kind_dispatch_fact() {
        let p = parse_predicate("stmt is if").expect("parse");
        let c = RenderContext {
            stmt: Some(StmtKind::If),
            ..Default::default()
        };
        assert!(c.eval(&p));
        let c2 = RenderContext {
            stmt: Some(StmtKind::While),
            ..Default::default()
        };
        assert!(!c2.eval(&p));
        // No statement in context -> the fact does not hold.
        assert!(!ctx().eval(&p));
    }

    #[test]
    fn stmt_kind_all_values_parse() {
        for kind in [
            "block", "let", "return", "if", "while", "for", "foreach", "switch", "break",
            "continue", "expr",
        ] {
            parse_predicate(&format!("stmt is {kind}"))
                .unwrap_or_else(|e| panic!("`stmt is {kind}` should parse: {e:?}"));
        }
    }

    #[test]
    fn rejects_unknown_stmt_kind() {
        let err = parse_predicate("stmt is loop").expect_err("unknown stmt kind");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }

    #[test]
    fn item_kind_dispatch_fact() {
        let p = parse_predicate("item is struct").expect("parse");
        let c = RenderContext {
            item: Some(ItemKind::Struct),
            ..Default::default()
        };
        assert!(c.eval(&p));
        let c2 = RenderContext {
            item: Some(ItemKind::Enum),
            ..Default::default()
        };
        assert!(!c2.eval(&p));
        // No item in context -> the fact does not hold.
        assert!(!ctx().eval(&p));
    }

    #[test]
    fn item_kind_all_values_parse() {
        for kind in ["function", "struct", "enum", "typedef", "const", "use"] {
            parse_predicate(&format!("item is {kind}"))
                .unwrap_or_else(|e| panic!("`item is {kind}` should parse: {e:?}"));
        }
    }

    #[test]
    fn item_const_distinct_from_const_modifier() {
        // `item is const` (an enum query) is distinct from the boolean `const`
        // modifier fact.
        let item_const = parse_predicate("item is const").expect("parse");
        let mod_const = parse_predicate("const").expect("parse");
        let c = RenderContext {
            item: Some(ItemKind::Const),
            is_const: false,
            ..Default::default()
        };
        assert!(c.eval(&item_const));
        assert!(!c.eval(&mod_const));
    }

    #[test]
    fn rejects_unknown_item_kind() {
        let err = parse_predicate("item is trait").expect_err("unknown item kind");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }

    #[test]
    fn optional_part_facts_evaluate() {
        let p = parse_predicate("stmt is return && has_value").expect("parse");
        let with = RenderContext {
            stmt: Some(StmtKind::Return),
            has_value: true,
            ..Default::default()
        };
        assert!(with.eval(&p));
        let without = RenderContext {
            stmt: Some(StmtKind::Return),
            has_value: false,
            ..Default::default()
        };
        assert!(!without.eval(&p));

        // Each has_* fact is a distinct closed boolean fact.
        for fact in [
            "has_value",
            "has_type",
            "has_else",
            "has_init",
            "has_cond",
            "has_step",
            "has_default",
        ] {
            parse_predicate(fact).unwrap_or_else(|e| panic!("`{fact}` should parse: {e:?}"));
        }
    }

    #[test]
    fn rejects_unknown_has_fact() {
        let err = parse_predicate("has_frob").expect_err("unknown has fact");
        assert!(matches!(err, PredicateError::UnknownFact { .. }));
    }
}
