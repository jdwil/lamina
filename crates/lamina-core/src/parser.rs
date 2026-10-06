//! A recursive-descent parser for the full ratified Lamina concrete source
//! syntax (`docs/source-syntax.md`), producing the kernel [`crate::ast`].
//!
//! # Shape
//!
//! The parser is one function per grammar production, mirroring the spec:
//! `parse_file` loops items; `parse_item` dispatches on the leading keyword to
//! `fn` / `struct` / `enum` / `const` / `typedef` / `use` / a top-level
//! `node`/`text` tree value; statements and expressions each have their own
//! production functions. Expressions use **precedence climbing** (`parse_binary`)
//! over a fixed [precedence table](#precedence); parentheses override grouping.
//!
//! # Precedence
//!
//! Binary operators bind by the conventional C/Rust ordering (lowest → highest):
//!
//! | Level | Operators | Assoc |
//! |-------|-----------|-------|
//! | 1  | `\|\|` | left |
//! | 2  | `&&` | left |
//! | 3  | `\|` | left |
//! | 4  | `^` | left |
//! | 5  | `&` | left |
//! | 6  | `==` `!=` | left |
//! | 7  | `<` `<=` `>` `>=` | left |
//! | 8  | `<<` `>>` `>>>` | left |
//! | 9  | `+` `-` | left |
//! | 10 | `*` `/` `%` | left |
//! | 11 | `**` | right |
//!
//! Unary `-` `!` `~` `+` bind tighter than any binary; postfix `.field`,
//! `[index]`, `(args)`, and the `as` cast bind tightest.
//!
//! # Resolved grammar ambiguities
//!
//! - **Struct-literal `{` vs block `{`.** In expression position a `Name { … }`
//!   is a [struct literal](Expr::StructLit); a bare `{` opening a statement is a
//!   block. The two never collide because a struct literal is only recognized
//!   when a bare identifier (a type name) is immediately followed by `{` *in
//!   expression position*, and statement/`if`/`while`/`for`/`switch` headers use
//!   **parenthesized** conditions, so a `{` after a condition always opens a
//!   block, never a struct literal. To keep an `if (cond) { … }` header
//!   unambiguous, struct-literal recognition is **suppressed** while parsing the
//!   controlling expression of `if`/`while`/`for`/`foreach`/`switch` and
//!   re-enabled inside parentheses and argument lists.
//! - **Lambda vs parenthesized expression.** A leading `(` is a lambda iff the
//!   parenthesized list is a (possibly empty) comma-separated list of
//!   `name: Type` parameters followed by `)` then either `:` (a return type) or
//!   `=>`. The parser speculatively scans the parenthesized region for this
//!   shape before committing; otherwise `(` is an ordinary grouped expression.
//! - **`//`** is always a line comment (see the lexer); floor division is
//!   spelled **`~/`** (Dart-style) and maps to [`BinaryOp::FloorDiv`] at
//!   multiplicative precedence.

use crate::ast::{
    is_lvalue, Attr, BinaryOp, CaseBindings, CaseFieldBind, Expr, Field, FieldInit, File, Function,
    Item, Meta, Modifier, Param, Primitive, RawArm, Statement, SwitchCase, Type, TypeAttribute,
    UnaryOp, UseItem, Variant, VariantPayload, Visibility,
};
use crate::error::ParseError;
use crate::lexer::{lex, SpannedToken, Token};

/// Parses Lamina source into a [`File`] AST.
///
/// This is the engine's entry point for the concrete source syntax; it lexes
/// `src` and runs the recursive-descent grammar.
///
/// # Errors
///
/// Returns a [`ParseError`] if the source does not conform to the ratified
/// grammar (`docs/source-syntax.md`), with the offending source position where
/// available. The `raw` escape hatch is fully parsed into a multi-arm raw node
/// (its verbatim arm bodies are captured by the lexer); target resolution
/// happens later, in [`lower`](crate::lower).
pub fn parse(src: &str) -> Result<File, ParseError> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_file()
}

/// The recursive-descent parser state: the token stream and a cursor.
struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    // ---- Token stream helpers -----------------------------------------

    /// Peeks the current token kind without consuming.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    /// Peeks the token `ahead` positions past the cursor.
    fn peek_at(&self, ahead: usize) -> Option<&Token> {
        self.tokens.get(self.pos + ahead).map(|t| &t.token)
    }

    /// Advances past the current token, returning its kind.
    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).map(|t| t.token.clone());
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    /// Returns `true` and consumes if the current token equals `token`.
    fn eat(&mut self, token: &Token) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// A positioned "expected X" error at the current token.
    fn error(&self, expected: impl Into<String>) -> ParseError {
        match self.tokens.get(self.pos) {
            Some(t) => ParseError::ExpectedAt {
                expected: expected.into(),
                found: t.token.describe(),
                line: t.span.line,
                column: t.span.column,
            },
            None => ParseError::Expected {
                expected: expected.into(),
                found: "end of input".to_string(),
            },
        }
    }

    /// Consumes a token equal to `expected` or returns a positioned error whose
    /// "expected" text is `what`.
    fn expect(&mut self, expected: &Token, what: &str) -> Result<(), ParseError> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(what))
        }
    }

    /// Consumes and returns an identifier, or a positioned error.
    fn expect_ident(&mut self, what: &str) -> Result<String, ParseError> {
        match self.peek() {
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.pos += 1;
                Ok(name)
            }
            _ => Err(self.error(what)),
        }
    }

    // ---- File / items -------------------------------------------------

    /// `file := item*`
    fn parse_file(&mut self) -> Result<File, ParseError> {
        let mut items = Vec::new();
        while self.peek().is_some() {
            items.push(self.parse_item()?);
        }
        Ok(File { items })
    }

    /// Dispatches a top-level item on its leading keyword.
    ///
    /// `item := attributes? (fn | struct | enum | const | typedef | use | tree)`
    fn parse_item(&mut self) -> Result<Item, ParseError> {
        // Leading `@attr` / `@meta(...)` annotations apply to the next struct or
        // enum (and are collected here so the dispatch sees the real keyword).
        let (attributes, meta) = self.parse_annotations()?;

        // Visibility may precede the item keyword (functions, structs, enums,
        // consts). Modifiers may precede `fn`.
        let visibility = self.parse_optional_visibility();
        let modifiers = self.parse_modifiers();

        match self.peek() {
            Some(Token::Fn) => Ok(Item::Function(self.parse_function(visibility, modifiers)?)),
            Some(Token::Struct) => self.parse_struct(visibility, attributes, meta),
            Some(Token::Enum) => self.parse_enum(visibility, attributes, meta),
            Some(Token::Const) => self.parse_const(visibility),
            Some(Token::TypeDef) => self.parse_typedef(),
            Some(Token::Use) => self.parse_use(),
            Some(Token::Node) | Some(Token::Text) => {
                // A top-level tree value (a declarative-only document root).
                let expr = self.parse_expr()?;
                Ok(Item::Tree(expr))
            }
            Some(Token::RawConstruct { .. }) => {
                let (arms, default) = self.take_raw_construct()?;
                Ok(Item::Raw {
                    arms,
                    default,
                    meta: Meta::new(),
                })
            }
            _ => Err(self.error("a top-level item (fn/struct/enum/const/typedef/use/node)")),
        }
    }

    /// Parses zero or more leading `@attr` / `@meta(k = "v", …)` annotations.
    fn parse_annotations(&mut self) -> Result<(Vec<TypeAttribute>, Meta), ParseError> {
        let mut attributes = Vec::new();
        let mut meta = Meta::new();
        while self.eat(&Token::At) {
            let name = self.expect_ident("an attribute or `meta` name after `@`")?;
            if name == "meta" {
                self.expect(&Token::LParen, "`(` after `@meta`")?;
                self.parse_meta_entries(&mut meta)?;
                self.expect(&Token::RParen, "`)` to close `@meta(...)`")?;
            } else if let Some(attr) = TypeAttribute::from_name(&name) {
                attributes.push(attr);
            } else {
                return Err(ParseError::Expected {
                    expected: "a known type attribute (equatable/displayable/…) or `meta`"
                        .to_string(),
                    found: format!("`@{name}`"),
                });
            }
        }
        Ok((attributes, meta))
    }

    /// Parses the `key = "value", …` entries of an `@meta(...)` annotation.
    fn parse_meta_entries(&mut self, meta: &mut Meta) -> Result<(), ParseError> {
        if self.peek() == Some(&Token::RParen) {
            return Ok(());
        }
        loop {
            let key = self.expect_ident("a metadata key")?;
            self.expect(&Token::Eq, "`=` in a metadata entry")?;
            let value = match self.advance() {
                Some(Token::Str(s)) => s,
                _ => return Err(self.error("a double-quoted metadata value")),
            };
            meta.set(key, value);
            if !self.eat(&Token::Comma) {
                break;
            }
            if self.peek() == Some(&Token::RParen) {
                break;
            }
        }
        Ok(())
    }

    /// Parses an optional leading visibility keyword.
    fn parse_optional_visibility(&mut self) -> Visibility {
        if let Some(Token::Ident(word)) = self.peek() {
            if let Some(vis) = Visibility::from_name(word) {
                self.pos += 1;
                return vis;
            }
        }
        Visibility::Private
    }

    /// Parses an optional leading **field** visibility keyword
    /// (`public`/`protected`/`private`, full words). Unlike items — whose
    /// omitted visibility defaults to [`Visibility::Private`] — a struct/variant
    /// field whose visibility is omitted keeps the field default
    /// [`Visibility::Public`], matching the pre-syntax behavior so existing
    /// field lists render byte-identically.
    fn parse_optional_field_visibility(&mut self) -> Visibility {
        if let Some(Token::Ident(word)) = self.peek() {
            if let Some(vis) = Visibility::from_name(word) {
                self.pos += 1;
                return vis;
            }
        }
        Visibility::Public
    }

    /// Parses zero or more on/off modifier keywords preceding `fn`.
    ///
    /// Modifiers are lexed as identifiers (except `const`, which has its own
    /// keyword and is recognized here); they are collected in source order.
    fn parse_modifiers(&mut self) -> Vec<Modifier> {
        let mut modifiers = Vec::new();
        loop {
            match self.peek() {
                Some(Token::Const) => {
                    // `const` is a modifier here ONLY when a `fn` (or further
                    // modifiers) follows; a `const` item declaration is handled
                    // by the item dispatcher. Look ahead: if the token after the
                    // run of modifiers is `fn`, treat `const` as a modifier.
                    if self.const_is_modifier() {
                        modifiers.push(Modifier::Const);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Some(Token::Ident(word)) => {
                    if let Some(m) = Modifier::from_name(word) {
                        if m != Modifier::Const {
                            modifiers.push(m);
                            self.pos += 1;
                            continue;
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
        modifiers
    }

    /// Decides whether a leading `const` is a function modifier (`const fn …`)
    /// rather than a `const` item declaration (`const NAME: T = v;`). It is a
    /// modifier iff, skipping a run of modifier keywords after it, the next
    /// token is `fn`.
    fn const_is_modifier(&self) -> bool {
        let mut i = self.pos + 1; // past `const`
        while let Some(t) = self.tokens.get(i).map(|t| &t.token) {
            match t {
                Token::Const => i += 1,
                Token::Ident(word) if Modifier::from_name(word).is_some() => i += 1,
                Token::Fn => return true,
                _ => return false,
            }
        }
        false
    }

    /// `function := "fn" IDENT "(" params? ")" ret? block`
    ///
    /// `ret` is either the ratified `: Type` form or the legacy `-> Type` arrow
    /// (kept for backward compatibility with existing fixtures); an omitted
    /// return type defaults to `void`.
    fn parse_function(
        &mut self,
        visibility: Visibility,
        modifiers: Vec<Modifier>,
    ) -> Result<Function, ParseError> {
        self.expect(&Token::Fn, "keyword `fn`")?;
        let name = self.expect_ident("a function name")?;
        self.expect(&Token::LParen, "`(` after the function name")?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen, "`)` to close the parameter list")?;
        let return_type = self.parse_return_type()?;
        let body = self.parse_block()?;
        Ok(Function {
            name,
            visibility,
            modifiers,
            params,
            return_type,
            body,
            meta: Meta::new(),
        })
    }

    /// `params := (param ("," param)*)?` where `param := IDENT ":" type`.
    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.peek() == Some(&Token::RParen) {
            return Ok(params);
        }
        loop {
            let name = self.expect_ident("a parameter name")?;
            self.expect(&Token::Colon, "`:` between a parameter name and its type")?;
            let ty = self.parse_type()?;
            params.push(Param {
                name,
                ty,
                meta: Meta::new(),
            });
            if !self.eat(&Token::Comma) {
                break;
            }
            // Allow a trailing comma before `)`.
            if self.peek() == Some(&Token::RParen) {
                break;
            }
        }
        Ok(params)
    }

    /// Parses an optional return type after a parameter list. Accepts both the
    /// ratified `: Type` and the legacy `-> Type` spellings; a missing return
    /// type is `void`.
    fn parse_return_type(&mut self) -> Result<Type, ParseError> {
        if self.eat(&Token::Colon) || self.eat(&Token::Arrow) {
            self.parse_type()
        } else {
            Ok(Type::Primitive(Primitive::Void))
        }
    }

    /// `struct := "struct" IDENT "{" (field ("," field)*)? ","? "}"`
    fn parse_struct(
        &mut self,
        visibility: Visibility,
        attributes: Vec<TypeAttribute>,
        meta: Meta,
    ) -> Result<Item, ParseError> {
        self.expect(&Token::Struct, "keyword `struct`")?;
        let name = self.expect_ident("a struct name")?;
        self.expect(&Token::LBrace, "`{` to open the struct body")?;
        let fields = self.parse_fields(&Token::RBrace)?;
        self.expect(&Token::RBrace, "`}` to close the struct body")?;
        Ok(Item::Struct {
            name,
            visibility,
            fields,
            attributes,
            meta,
        })
    }

    /// Parses a comma-separated field list up to (but not consuming) `end`.
    fn parse_fields(&mut self, end: &Token) -> Result<Vec<Field>, ParseError> {
        let mut fields = Vec::new();
        if self.peek() == Some(end) {
            return Ok(fields);
        }
        loop {
            // Optional leading field visibility (`public`/`protected`/`private`
            // as full words). A bare field keeps the field default — `Public` —
            // so a field list written without any visibility keyword parses
            // byte-identically to before this syntax existed.
            let visibility = self.parse_optional_field_visibility();
            let name = self.expect_ident("a field name")?;
            self.expect(&Token::Colon, "`:` between a field name and its type")?;
            let ty = self.parse_type()?;
            fields.push(Field {
                name,
                ty,
                visibility,
                meta: Meta::new(),
            });
            if !self.eat(&Token::Comma) {
                break;
            }
            if self.peek() == Some(end) {
                break;
            }
        }
        Ok(fields)
    }

    /// `enum := "enum" IDENT "{" (variant ("," variant)*)? ","? "}"`
    fn parse_enum(
        &mut self,
        visibility: Visibility,
        attributes: Vec<TypeAttribute>,
        meta: Meta,
    ) -> Result<Item, ParseError> {
        self.expect(&Token::Enum, "keyword `enum`")?;
        let name = self.expect_ident("an enum name")?;
        self.expect(&Token::LBrace, "`{` to open the enum body")?;
        let mut variants = Vec::new();
        if self.peek() != Some(&Token::RBrace) {
            loop {
                variants.push(self.parse_variant()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
                if self.peek() == Some(&Token::RBrace) {
                    break;
                }
            }
        }
        self.expect(&Token::RBrace, "`}` to close the enum body")?;
        Ok(Item::Enum {
            name,
            visibility,
            variants,
            attributes,
            meta,
        })
    }

    /// `variant := IDENT ("(" type ("," type)* ")" | "{" field ("," field)* "}")?`
    fn parse_variant(&mut self) -> Result<Variant, ParseError> {
        let name = self.expect_ident("a variant name")?;
        let payload = match self.peek() {
            Some(Token::LParen) => {
                self.pos += 1;
                let mut types = Vec::new();
                if self.peek() != Some(&Token::RParen) {
                    loop {
                        types.push(self.parse_type()?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                        if self.peek() == Some(&Token::RParen) {
                            break;
                        }
                    }
                }
                self.expect(&Token::RParen, "`)` to close a tuple variant payload")?;
                VariantPayload::Tuple(types)
            }
            Some(Token::LBrace) => {
                self.pos += 1;
                let fields = self.parse_fields(&Token::RBrace)?;
                self.expect(&Token::RBrace, "`}` to close a struct variant payload")?;
                VariantPayload::Struct(fields)
            }
            _ => VariantPayload::None,
        };
        Ok(Variant {
            name,
            payload,
            meta: Meta::new(),
        })
    }

    /// `const := "const" IDENT ":" type "=" expr ";"`
    fn parse_const(&mut self, visibility: Visibility) -> Result<Item, ParseError> {
        self.expect(&Token::Const, "keyword `const`")?;
        let name = self.expect_ident("a constant name")?;
        self.expect(&Token::Colon, "`:` between a constant name and its type")?;
        let ty = self.parse_type()?;
        self.expect(&Token::Eq, "`=` in a constant declaration")?;
        let value = self.parse_expr()?;
        self.expect(&Token::Semicolon, "`;` to end a constant declaration")?;
        Ok(Item::Const {
            name,
            ty,
            value,
            visibility,
            meta: Meta::new(),
        })
    }

    /// `typedef := "typedef" IDENT "=" type ";"`
    fn parse_typedef(&mut self) -> Result<Item, ParseError> {
        self.expect(&Token::TypeDef, "keyword `typedef`")?;
        let name = self.expect_ident("a type-alias name")?;
        self.expect(&Token::Eq, "`=` in a typedef")?;
        let target = self.parse_type()?;
        self.expect(&Token::Semicolon, "`;` to end a typedef")?;
        Ok(Item::TypeDef {
            name,
            target,
            meta: Meta::new(),
        })
    }

    /// `use := "use" path ("::" "{" useitem ("," useitem)* "}")? ("as" IDENT)? ";"`
    ///
    /// The path is a `::`-separated run of identifiers (optionally prefixed with
    /// `./` for a local path, which is lexed as `.`/`/`-free so is captured by
    /// identifiers). A selective `{ a, b as c }` list or a module `as alias`
    /// may follow.
    fn parse_use(&mut self) -> Result<Item, ParseError> {
        self.expect(&Token::Use, "keyword `use`")?;
        let mut path = self.parse_use_path()?;
        let mut items = Vec::new();
        let mut alias = None;

        // A selective import: `path::{ a, b as c }`.
        if self.peek() == Some(&Token::ColonColon) && self.peek_at(1) == Some(&Token::LBrace) {
            self.pos += 2; // `::` `{`
            if self.peek() != Some(&Token::RBrace) {
                loop {
                    let name = self.expect_ident("an imported item name")?;
                    let item_alias = if self.eat(&Token::As) {
                        Some(self.expect_ident("an alias after `as`")?)
                    } else {
                        None
                    };
                    items.push(UseItem {
                        name,
                        alias: item_alias,
                        meta: Meta::new(),
                    });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                    if self.peek() == Some(&Token::RBrace) {
                        break;
                    }
                }
            }
            self.expect(&Token::RBrace, "`}` to close a selective import")?;
        } else if self.eat(&Token::As) {
            // A module alias: `path as p`.
            alias = Some(self.expect_ident("an alias after `as`")?);
        }

        // A trailing part may continue the path after a `::` to a bare name was
        // already folded in `parse_use_path`; nothing more here.
        self.expect(&Token::Semicolon, "`;` to end a `use`")?;
        // Normalize a `./`-style local path: a leading `.` segment was captured
        // as part of the path text already.
        if path.is_empty() {
            path = String::new();
        }
        Ok(Item::Use {
            path,
            items,
            alias,
            meta: Meta::new(),
        })
    }

    /// Parses a `use` path: a `::`-separated run of identifiers, with an
    /// optional leading `./` local-path marker. Stops before a trailing
    /// `::{ … }` selective list (left for the caller).
    fn parse_use_path(&mut self) -> Result<String, ParseError> {
        let mut path = String::new();
        // Optional leading `./` for a local source path.
        if self.peek() == Some(&Token::Dot) && self.peek_at(1) == Some(&Token::Slash) {
            self.pos += 2;
            path.push_str("./");
        }
        let first = self.expect_ident("a module path")?;
        path.push_str(&first);
        // Continue consuming `::ident` segments, but stop if the next after
        // `::` is `{` (a selective list) — that is the caller's concern.
        while self.peek() == Some(&Token::ColonColon) {
            if self.peek_at(1) == Some(&Token::LBrace) {
                break;
            }
            self.pos += 1; // `::`
            let seg = self.expect_ident("a module path segment after `::`")?;
            path.push_str("::");
            path.push_str(&seg);
        }
        Ok(path)
    }

    // ---- Types --------------------------------------------------------

    /// `type := primitive | named | "[" type (";" INT)? "]"`
    ///
    /// A bare identifier is a primitive if it spells one of the frozen kernel
    /// primitive names, otherwise a user-defined [`Type::Named`]. An array type
    /// is `[elem]` (unsized) or `[elem; N]` (sized).
    ///
    /// Two **prefix** forms appear in TYPE POSITION ONLY (after a `:` in a
    /// `let`/field/param, inside a fnptr param/return list, in a typedef target,
    /// etc.) — never in expression position, so a leading `*` is unambiguously
    /// "pointer to" rather than the multiplication operator:
    ///
    /// - `*T` is a [`Type::Pointer`] to `T`. It composes, so `**i32` is a
    ///   pointer-to-pointer. (The lexer folds `**` into a single [`Token::StarStar`],
    ///   so this consumes that token as a pair of `*` prefixes.)
    /// - `*fn(T1, T2, …): R` is a [`Type::FnPtr`]. The `*` means "pointer to";
    ///   `fn(params): ret` names the signature (mirroring the `fn name(params):
    ///   ret` declaration shape, minus the name). A bare `fn(…)` WITHOUT a
    ///   leading `*` is NOT a standalone type (the kernel has only the fnptr
    ///   primitive), so `fn` is reachable here only through the `*` arm.
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        match self.peek() {
            // `*T` — pointer to `T`, type position only (no multiply here).
            Some(Token::Star) => {
                self.pos += 1;
                self.parse_pointer_or_fnptr()
            }
            // `**T` — the lexer folds `**` into one token; treat it as two `*`
            // prefixes so `**i32` is a pointer-to-pointer type.
            Some(Token::StarStar) => {
                self.pos += 1;
                let inner = self.parse_pointer_or_fnptr()?;
                Ok(Type::Pointer(Box::new(inner)))
            }
            Some(Token::LBracket) => {
                self.pos += 1;
                let elem = self.parse_type()?;
                let len = if self.eat(&Token::Semicolon) {
                    match self.advance() {
                        Some(Token::Int(n)) => Some(n),
                        _ => return Err(self.error("an array length after `;`")),
                    }
                } else {
                    None
                };
                self.expect(&Token::RBracket, "`]` to close an array type")?;
                Ok(Type::Array {
                    elem: Box::new(elem),
                    len,
                })
            }
            Some(Token::Ident(name)) => {
                let name = name.clone();
                self.pos += 1;
                match Primitive::from_name(&name) {
                    Some(p) => Ok(Type::Primitive(p)),
                    None => Ok(Type::Named(name)),
                }
            }
            _ => Err(self.error("a type")),
        }
    }

    /// Parses the type that follows a consumed leading `*` in type position.
    ///
    /// If the next token is `fn`, this is the function-pointer form `*fn(T1,
    /// …): R` and yields a [`Type::FnPtr`]; otherwise the `*` is an ordinary
    /// pointer prefix and this yields a [`Type::Pointer`] wrapping the inner
    /// type (which may itself begin with `*`, so pointers compose).
    fn parse_pointer_or_fnptr(&mut self) -> Result<Type, ParseError> {
        if self.peek() == Some(&Token::Fn) {
            self.parse_fnptr_type()
        } else {
            let inner = self.parse_type()?;
            Ok(Type::Pointer(Box::new(inner)))
        }
    }

    /// Parses the function-pointer signature after a consumed `*`: `fn(T1, T2,
    /// …): R`. The parameter list is a (possibly empty) comma-separated list of
    /// types; the return type follows the ratified `: Type` form (an omitted
    /// return type defaults to `void`, mirroring a declaration).
    fn parse_fnptr_type(&mut self) -> Result<Type, ParseError> {
        self.expect(&Token::Fn, "keyword `fn` in a function-pointer type")?;
        self.expect(&Token::LParen, "`(` after `fn` in a function-pointer type")?;
        let mut params = Vec::new();
        if self.peek() != Some(&Token::RParen) {
            loop {
                params.push(self.parse_type()?);
                if !self.eat(&Token::Comma) {
                    break;
                }
                // Allow a trailing comma before `)`.
                if self.peek() == Some(&Token::RParen) {
                    break;
                }
            }
        }
        self.expect(&Token::RParen, "`)` to close a function-pointer parameter list")?;
        let ret = self.parse_return_type()?;
        Ok(Type::FnPtr {
            params,
            ret: Box::new(ret),
        })
    }

    // ---- Statements ---------------------------------------------------

    /// `block := "{" statement* "}"` — returns the statement vector.
    fn parse_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        self.expect(&Token::LBrace, "`{` to open a block")?;
        let mut body = Vec::new();
        while self.peek() != Some(&Token::RBrace) {
            if self.peek().is_none() {
                return Err(self.error("`}` to close a block"));
            }
            body.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace, "`}` to close a block")?;
        Ok(body)
    }

    /// Dispatches a statement on its leading token.
    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        match self.peek() {
            Some(Token::LBrace) => Ok(Statement::Block(self.parse_block()?)),
            Some(Token::Let) => self.parse_let(),
            Some(Token::Return) => self.parse_return(),
            Some(Token::If) => self.parse_if(),
            Some(Token::While) => self.parse_while(),
            Some(Token::For) => self.parse_for(),
            Some(Token::ForEach) => self.parse_foreach(),
            Some(Token::Switch) => self.parse_switch(),
            Some(Token::Break) => {
                self.pos += 1;
                self.expect(&Token::Semicolon, "`;` after `break`")?;
                Ok(Statement::Break)
            }
            Some(Token::Continue) => {
                self.pos += 1;
                self.expect(&Token::Semicolon, "`;` after `continue`")?;
                Ok(Statement::Continue)
            }
            Some(Token::RawConstruct { .. }) => {
                let (arms, default) = self.take_raw_construct()?;
                // A raw statement supplies its own terminator inside the
                // verbatim body; a trailing `;` after the construct is optional
                // (mirroring how a brace-bodied statement self-terminates).
                self.eat(&Token::Semicolon);
                Ok(Statement::Raw {
                    arms,
                    default,
                    meta: Meta::new(),
                })
            }
            // A tree-core node/text at statement position is an
            // expression-statement whose brace/standalone form makes a trailing
            // `;` OPTIONAL (matching the worked example's `render` body, where a
            // bare `node … { … }` is the whole body). This mirrors how block
            // statements self-terminate.
            Some(Token::Node) | Some(Token::Text) => {
                let expr = self.parse_primary(true)?;
                self.eat(&Token::Semicolon);
                Ok(Statement::Expr(expr))
            }
            _ => self.parse_expr_or_assign_statement(),
        }
    }

    /// `let := "let" IDENT (":" type)? ("=" expr)? ";"`
    fn parse_let(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::Let, "keyword `let`")?;
        let name = self.expect_ident("a binding name after `let`")?;
        let ty = if self.eat(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let value = if self.eat(&Token::Eq) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&Token::Semicolon, "`;` to end a `let`")?;
        Ok(Statement::Let { name, ty, value })
    }

    /// `return := "return" expr? ";"`
    fn parse_return(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::Return, "keyword `return`")?;
        if self.eat(&Token::Semicolon) {
            return Ok(Statement::Return(None));
        }
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon, "`;` to end a `return`")?;
        Ok(Statement::Return(Some(expr)))
    }

    /// `if := "if" "(" expr ")" block ("else" (if | block))?`
    fn parse_if(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::If, "keyword `if`")?;
        let cond = self.parse_parenthesized_condition()?;
        let then_block = self.parse_block()?;
        let else_block = if self.eat(&Token::Else) {
            if self.peek() == Some(&Token::If) {
                Some(Box::new(self.parse_if()?))
            } else {
                Some(Box::new(Statement::Block(self.parse_block()?)))
            }
        } else {
            None
        };
        Ok(Statement::If {
            cond,
            then_block,
            else_block,
        })
    }

    /// `while := "while" "(" expr ")" block`
    fn parse_while(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::While, "keyword `while`")?;
        let cond = self.parse_parenthesized_condition()?;
        let body = self.parse_block()?;
        Ok(Statement::While { cond, body })
    }

    /// `for := "for" "(" init? ";" cond? ";" step? ")" block`
    ///
    /// `init` is a `let` or an expression/assignment (no trailing `;` consumed
    /// by the clause — the header's own `;` separates them); `step` is an
    /// expression or assignment.
    fn parse_for(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::For, "keyword `for`")?;
        self.expect(&Token::LParen, "`(` after `for`")?;

        // init clause (optional), terminated by the header `;`.
        let init = if self.peek() == Some(&Token::Semicolon) {
            None
        } else if self.peek() == Some(&Token::Let) {
            Some(Box::new(self.parse_let()?))
        } else {
            // An assignment or expression clause; it carries its own `;`.
            Some(Box::new(self.parse_expr_or_assign_statement()?))
        };
        // If init was a `let`/assign/expr statement, it already consumed its
        // `;`. If init was None, consume the first header `;` now.
        if init.is_none() {
            self.expect(&Token::Semicolon, "`;` after the `for` initializer")?;
        }

        // cond (optional), terminated by `;`.
        let cond = if self.peek() == Some(&Token::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&Token::Semicolon, "`;` after the `for` condition")?;

        // step (optional), terminated by `)`.
        let step = if self.peek() == Some(&Token::RParen) {
            None
        } else {
            Some(Box::new(self.parse_for_step()?))
        };
        self.expect(&Token::RParen, "`)` to close the `for` header")?;

        let body = self.parse_block()?;
        Ok(Statement::For {
            init,
            cond,
            step,
            body,
        })
    }

    /// Parses a `for` step clause (an assignment or expression) WITHOUT a
    /// trailing `;` — the header's `)` terminates it.
    fn parse_for_step(&mut self) -> Result<Statement, ParseError> {
        let expr = self.parse_expr()?;
        if self.eat(&Token::Eq) {
            let value = self.parse_expr()?;
            self.make_assign(expr, value)
        } else {
            Ok(Statement::Expr(expr))
        }
    }

    /// `foreach := "foreach" IDENT "in" expr block`
    fn parse_foreach(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::ForEach, "keyword `foreach`")?;
        let binding = self.expect_ident("a binding name after `foreach`")?;
        self.expect(&Token::In, "keyword `in` in a `foreach`")?;
        let iterable = self.parse_condition_expr()?;
        let body = self.parse_block()?;
        Ok(Statement::ForEach {
            binding,
            iterable,
            body,
        })
    }

    /// `switch := "switch" "(" expr ")" "{" case* default? "}"`
    ///
    /// `case := "case" value payload? block` and `default := "default" block`.
    fn parse_switch(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::Switch, "keyword `switch`")?;
        let scrutinee = self.parse_parenthesized_condition()?;
        self.expect(&Token::LBrace, "`{` to open the switch body")?;
        let mut cases = Vec::new();
        let mut default = None;
        loop {
            match self.peek() {
                Some(Token::Case) => {
                    self.pos += 1;
                    let (value, bindings) = self.parse_case_pattern()?;
                    let body = self.parse_block()?;
                    cases.push(SwitchCase::with_bindings(value, body, bindings));
                }
                Some(Token::Default) => {
                    self.pos += 1;
                    let body = self.parse_block()?;
                    default = Some(body);
                }
                Some(Token::RBrace) => break,
                _ => return Err(self.error("`case`, `default`, or `}` in a switch")),
            }
        }
        self.expect(&Token::RBrace, "`}` to close the switch body")?;
        Ok(Statement::Switch {
            scrutinee,
            cases,
            default,
        })
    }

    /// Parses a switch `case` pattern: a matched value and an optional payload
    /// binding. `case Circle(r)` binds a tuple payload positionally;
    /// `case Rect { w, h }` binds a struct payload by field name; a plain
    /// `case 1` or `case Foo` has no binding.
    ///
    /// Returns the matched value expression and the [`CaseBindings`].
    fn parse_case_pattern(&mut self) -> Result<(Expr, CaseBindings), ParseError> {
        // A bare identifier may be a variant name carrying a payload binding.
        if let Some(Token::Ident(name)) = self.peek() {
            let name = name.clone();
            // Positional binding: `Variant(a, b, …)`.
            if self.peek_at(1) == Some(&Token::LParen) {
                self.pos += 2; // ident `(`
                let mut names = Vec::new();
                if self.peek() != Some(&Token::RParen) {
                    loop {
                        names.push(self.expect_ident("a positional binding name")?);
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&Token::RParen, "`)` to close a positional case binding")?;
                return Ok((Expr::Ref(name), CaseBindings::Positional(names)));
            }
            // Named binding: `Variant { a, b, … }` (shorthand field = local).
            if self.peek_at(1) == Some(&Token::LBrace) {
                self.pos += 2; // ident `{`
                let mut binds = Vec::new();
                if self.peek() != Some(&Token::RBrace) {
                    loop {
                        let field = self.expect_ident("a named binding field")?;
                        // An explicit rename `field: local` is permitted; the
                        // shorthand `{ w, h }` binds local == field.
                        if self.eat(&Token::Colon) {
                            let bind = self.expect_ident("a local name after `:`")?;
                            binds.push(CaseFieldBind::new(field, bind));
                        } else {
                            binds.push(CaseFieldBind::shorthand(field));
                        }
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                        if self.peek() == Some(&Token::RBrace) {
                            break;
                        }
                    }
                }
                self.expect(&Token::RBrace, "`}` to close a named case binding")?;
                return Ok((Expr::Ref(name), CaseBindings::Named(binds)));
            }
            // A bare variant/identifier with no binding.
            self.pos += 1;
            return Ok((Expr::Ref(name), CaseBindings::None));
        }
        // A non-identifier case value (an integer, etc.): parse as an atom-level
        // expression (struct literals are not valid case values).
        let value = self.parse_expr_no_struct()?;
        Ok((value, CaseBindings::None))
    }

    /// Parses an expression statement or an assignment statement.
    ///
    /// `expr-stmt := expr ";"` and `assign := lvalue "=" expr ";"`. The leading
    /// expression is parsed first; a following `=` makes it an assignment.
    fn parse_expr_or_assign_statement(&mut self) -> Result<Statement, ParseError> {
        let expr = self.parse_expr()?;
        if self.eat(&Token::Eq) {
            let value = self.parse_expr()?;
            self.expect(&Token::Semicolon, "`;` to end an assignment")?;
            self.make_assign(expr, value)
        } else {
            self.expect(&Token::Semicolon, "`;` to end an expression statement")?;
            Ok(Statement::Expr(expr))
        }
    }

    /// Builds an [`Statement::Assign`], validating the target is an lvalue.
    fn make_assign(&self, target: Expr, value: Expr) -> Result<Statement, ParseError> {
        if is_lvalue(&target) {
            Ok(Statement::Assign { target, value })
        } else {
            let (line, column) = self.current_pos();
            Err(ParseError::InvalidAssignTarget {
                detail: format!(
                    "the left-hand side ({}) is not a place that can be assigned to",
                    target.kind().as_str()
                ),
                line,
                column,
            })
        }
    }

    /// The 1-based position of the current token (or the last one at EOF).
    fn current_pos(&self) -> (usize, usize) {
        let idx = self.pos.min(self.tokens.len().saturating_sub(1));
        match self.tokens.get(idx) {
            Some(t) => (t.span.line, t.span.column),
            None => (0, 0),
        }
    }

    /// Parses the parenthesized controlling expression of an `if`/`while`/
    /// `switch` header. The surrounding `(` `)` make a trailing `{` unambiguous
    /// (it always opens the body block, never a struct literal).
    fn parse_parenthesized_condition(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LParen, "`(` to open the condition")?;
        let expr = self.parse_expr()?;
        self.expect(&Token::RParen, "`)` to close the condition")?;
        Ok(expr)
    }

    /// Parses a controlling expression that is NOT parenthesized (the `foreach`
    /// iterable), suppressing struct-literal recognition so the following body
    /// `{` opens the loop block rather than a struct literal.
    fn parse_condition_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_no_struct()
    }

    /// Consumes the current [`Token::RawConstruct`] and returns its parsed arms
    /// and optional `else` fallback.
    ///
    /// The lexer does all raw scanning (the arm bodies are verbatim target code,
    /// captured as opaque text), so this is a trivial unpack shared by the three
    /// raw positions (expr / statement / item). The three nodes differ only in
    /// which `*::Raw` they wrap the identical `(arms, default)` payload in.
    fn take_raw_construct(&mut self) -> Result<(Vec<RawArm>, Option<String>), ParseError> {
        match self.advance() {
            Some(Token::RawConstruct { arms, default }) => Ok((arms, default)),
            other => Err(self.error(format!(
                "a `raw` construct (internal: unexpected {other:?})"
            ))),
        }
    }

    // ---- Expressions (precedence climbing) ----------------------------

    /// Parses a full expression (struct literals allowed).
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(0, true)
    }

    /// Parses a full expression with struct-literal recognition suppressed
    /// (used in control-flow headers before a body `{`).
    fn parse_expr_no_struct(&mut self) -> Result<Expr, ParseError> {
        self.parse_binary(0, false)
    }

    /// Precedence-climbing binary-expression parser.
    ///
    /// `min_bp` is the minimum binding power a binary operator must have to be
    /// consumed at this level; `allow_struct` threads struct-literal
    /// permission down to the primary parser.
    fn parse_binary(&mut self, min_bp: u8, allow_struct: bool) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary(allow_struct)?;
        while let Some(op) = self.peek().and_then(binary_op) {
            let (lbp, rbp, right_assoc) = binding_power(op);
            if lbp < min_bp {
                break;
            }
            self.pos += 1;
            let next_min = if right_assoc { rbp } else { rbp + 1 };
            let rhs = self.parse_binary(next_min, allow_struct)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    /// `unary := ("-" | "!" | "~" | "+") unary | postfix`
    fn parse_unary(&mut self, allow_struct: bool) -> Result<Expr, ParseError> {
        let op = match self.peek() {
            Some(Token::Minus) => Some(UnaryOp::Neg),
            Some(Token::Bang) => Some(UnaryOp::Not),
            Some(Token::Tilde) => Some(UnaryOp::BitNot),
            Some(Token::Plus) => Some(UnaryOp::Pos),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            let operand = self.parse_unary(allow_struct)?;
            Ok(Expr::Unary {
                op,
                operand: Box::new(operand),
            })
        } else {
            self.parse_postfix(allow_struct)
        }
    }

    /// `postfix := primary (".field" | "[index]" | "(args)" | "as" type)*`
    fn parse_postfix(&mut self, allow_struct: bool) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary(allow_struct)?;
        loop {
            match self.peek() {
                Some(Token::Dot) => {
                    self.pos += 1;
                    let field = self.expect_ident("a field name after `.`")?;
                    expr = Expr::Field {
                        obj: Box::new(expr),
                        field,
                    };
                }
                Some(Token::LBracket) => {
                    self.pos += 1;
                    let index = self.parse_expr()?;
                    self.expect(&Token::RBracket, "`]` to close an index")?;
                    expr = Expr::Index {
                        obj: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Some(Token::LParen) => {
                    self.pos += 1;
                    let args = self.parse_args()?;
                    self.expect(&Token::RParen, "`)` to close a call")?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                Some(Token::As) => {
                    self.pos += 1;
                    let ty = self.parse_type()?;
                    expr = Expr::Cast {
                        value: Box::new(expr),
                        ty,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// Parses a comma-separated argument list up to (not consuming) `)`.
    fn parse_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if self.peek() == Some(&Token::RParen) {
            return Ok(args);
        }
        loop {
            // Arguments always permit struct literals (unambiguous inside `()`).
            args.push(self.parse_binary(0, true)?);
            if !self.eat(&Token::Comma) {
                break;
            }
            if self.peek() == Some(&Token::RParen) {
                break;
            }
        }
        Ok(args)
    }

    /// `primary := literal | ref | struct_lit | array_lit | lambda | tree |
    ///             "(" expr ")"`
    fn parse_primary(&mut self, allow_struct: bool) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Int(_)) => match self.advance() {
                Some(Token::Int(v)) => Ok(Expr::IntLiteral(v)),
                _ => unreachable!("peeked Int"),
            },
            Some(Token::Float(_)) => match self.advance() {
                Some(Token::Float(v)) => Ok(Expr::FloatLiteral(v)),
                _ => unreachable!("peeked Float"),
            },
            Some(Token::Str(_)) => match self.advance() {
                Some(Token::Str(v)) => Ok(Expr::StringLiteral(v)),
                _ => unreachable!("peeked Str"),
            },
            Some(Token::Char(_)) => match self.advance() {
                Some(Token::Char(v)) => Ok(Expr::CharLiteral(v)),
                _ => unreachable!("peeked Char"),
            },
            Some(Token::True) => {
                self.pos += 1;
                Ok(Expr::BoolLiteral(true))
            }
            Some(Token::False) => {
                self.pos += 1;
                Ok(Expr::BoolLiteral(false))
            }
            Some(Token::Null) => {
                self.pos += 1;
                Ok(Expr::NullLiteral)
            }
            Some(Token::LBracket) => self.parse_array_literal(),
            Some(Token::Node) => self.parse_node(),
            Some(Token::Text) => self.parse_text(),
            Some(Token::RawConstruct { .. }) => {
                let (arms, default) = self.take_raw_construct()?;
                Ok(Expr::Raw {
                    arms,
                    default,
                    meta: Meta::new(),
                })
            }
            Some(Token::LParen) => {
                if self.looks_like_lambda() {
                    self.parse_lambda()
                } else {
                    self.pos += 1;
                    let expr = self.parse_binary(0, true)?;
                    self.expect(&Token::RParen, "`)` to close a parenthesized expression")?;
                    Ok(expr)
                }
            }
            Some(Token::Ident(_)) => {
                let name = self.expect_ident("an identifier")?;
                // A struct literal `Name { … }` is recognized only when struct
                // literals are permitted here (not in a control-flow header).
                if allow_struct && self.peek() == Some(&Token::LBrace) {
                    self.parse_struct_literal(name)
                } else {
                    Ok(Expr::Ref(name))
                }
            }
            _ => Err(self.error("an expression")),
        }
    }

    /// `struct_lit := IDENT "{" (fieldinit ("," fieldinit)*)? ","? "}"` where
    /// `fieldinit := IDENT ":" expr`.
    fn parse_struct_literal(&mut self, type_name: String) -> Result<Expr, ParseError> {
        self.expect(&Token::LBrace, "`{` to open a struct literal")?;
        let mut fields = Vec::new();
        if self.peek() != Some(&Token::RBrace) {
            loop {
                let name = self.expect_ident("a field name in a struct literal")?;
                self.expect(&Token::Colon, "`:` between a field name and its value")?;
                let value = self.parse_binary(0, true)?;
                fields.push(FieldInit {
                    name,
                    value,
                    meta: Meta::new(),
                });
                if !self.eat(&Token::Comma) {
                    break;
                }
                if self.peek() == Some(&Token::RBrace) {
                    break;
                }
            }
        }
        self.expect(&Token::RBrace, "`}` to close a struct literal")?;
        Ok(Expr::StructLit {
            type_name,
            fields,
            meta: Meta::new(),
        })
    }

    /// `array_lit := "[" (expr ("," expr)*)? ","? "]"`
    fn parse_array_literal(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LBracket, "`[` to open an array literal")?;
        let mut elems = Vec::new();
        if self.peek() != Some(&Token::RBracket) {
            loop {
                elems.push(self.parse_binary(0, true)?);
                if !self.eat(&Token::Comma) {
                    break;
                }
                if self.peek() == Some(&Token::RBracket) {
                    break;
                }
            }
        }
        self.expect(&Token::RBracket, "`]` to close an array literal")?;
        Ok(Expr::ArrayLit {
            elems,
            meta: Meta::new(),
        })
    }

    /// `node := "node" IDENT ("(" attr ("," attr)* ")")? ("{" child* "}")?`
    /// where `attr := IDENT "=" expr` and a child is a nested expression.
    fn parse_node(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::Node, "keyword `node`")?;
        let name = self.expect_ident("a node name")?;
        let mut attrs = Vec::new();
        if self.peek() == Some(&Token::LParen) {
            self.pos += 1;
            if self.peek() != Some(&Token::RParen) {
                loop {
                    let attr_name = self.expect_ident("an attribute name")?;
                    self.expect(&Token::Eq, "`=` in a node attribute")?;
                    let value = self.parse_binary(0, true)?;
                    attrs.push(Attr {
                        name: attr_name,
                        value,
                        meta: Meta::new(),
                    });
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                    if self.peek() == Some(&Token::RParen) {
                        break;
                    }
                }
            }
            self.expect(&Token::RParen, "`)` to close a node's attributes")?;
        }
        let mut children = Vec::new();
        if self.peek() == Some(&Token::LBrace) {
            self.pos += 1;
            while self.peek() != Some(&Token::RBrace) {
                if self.peek().is_none() {
                    return Err(self.error("`}` to close a node's children"));
                }
                children.push(self.parse_child()?);
            }
            self.expect(&Token::RBrace, "`}` to close a node's children")?;
        }
        Ok(Expr::Node {
            name,
            attrs,
            children,
            meta: Meta::new(),
        })
    }

    /// Parses a single tree child: a `node`, a `text`, or an interpolated
    /// expression. A child has no separator/terminator (children are
    /// whitespace-separated in source).
    fn parse_child(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Node) => self.parse_node(),
            Some(Token::Text) => self.parse_text(),
            _ => self.parse_binary(0, true),
        }
    }

    /// `text := "text" expr` (the inner expression is typically a string
    /// literal but may be any expression for interpolation).
    fn parse_text(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::Text, "keyword `text`")?;
        let inner = self.parse_binary(0, true)?;
        Ok(Expr::Text(Box::new(inner)))
    }

    /// `lambda := "(" params? ")" (":" type)? "=>" block`
    fn parse_lambda(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Token::LParen, "`(` to open a lambda parameter list")?;
        let params = self.parse_params()?;
        self.expect(&Token::RParen, "`)` to close a lambda parameter list")?;
        let return_type = if self.eat(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&Token::FatArrow, "`=>` in a lambda")?;
        let body = self.parse_block()?;
        Ok(Expr::Lambda {
            params,
            return_type,
            body,
            meta: Meta::new(),
        })
    }

    /// Speculatively decides whether a leading `(` begins a lambda.
    ///
    /// A `(` is a lambda iff it opens a (possibly empty) comma-separated list of
    /// `name: type` parameters, closes with `)`, and is then followed by `:` or
    /// `=>`. We scan without consuming: `()` → followed by `=>`/`:`; `(a: T, …)`
    /// → each entry is `ident :` and after the matching `)` comes `=>` or `:`.
    /// Anything else (`(expr)`, `(a, b)`) is a parenthesized expression.
    fn looks_like_lambda(&self) -> bool {
        // Must start at `(`.
        if self.peek() != Some(&Token::LParen) {
            return false;
        }
        // Empty `()` lambda: `() =>` or `() :`.
        if self.peek_at(1) == Some(&Token::RParen) {
            return matches!(
                self.peek_at(2),
                Some(Token::FatArrow) | Some(Token::Colon)
            );
        }
        // A non-empty list is a lambda iff the FIRST entry is `ident :` — a
        // parenthesized expression never begins `ident :` (there is no `:`
        // operator in expressions). We then find the matching `)` and check for
        // `=>`/`:` after it.
        if !matches!(self.peek_at(1), Some(Token::Ident(_)))
            || self.peek_at(2) != Some(&Token::Colon)
        {
            return false;
        }
        // Scan to the matching `)`.
        let mut depth = 0usize;
        let mut i = self.pos;
        while let Some(tok) = self.tokens.get(i).map(|t| &t.token) {
            match tok {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        // Token after the matching `)`.
                        return matches!(
                            self.tokens.get(i + 1).map(|t| &t.token),
                            Some(Token::FatArrow) | Some(Token::Colon)
                        );
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }
}

/// Maps a token to its [`BinaryOp`], or `None` if it is not a binary operator.
fn binary_op(token: &Token) -> Option<BinaryOp> {
    Some(match token {
        Token::PipePipe => BinaryOp::Or,
        Token::AmpAmp => BinaryOp::And,
        Token::Pipe => BinaryOp::BitOr,
        Token::Caret => BinaryOp::BitXor,
        Token::Amp => BinaryOp::BitAnd,
        Token::EqEq => BinaryOp::Eq,
        Token::NotEq => BinaryOp::Ne,
        Token::Lt => BinaryOp::Lt,
        Token::Le => BinaryOp::Le,
        Token::Gt => BinaryOp::Gt,
        Token::Ge => BinaryOp::Ge,
        Token::Shl => BinaryOp::Shl,
        Token::Shr => BinaryOp::Shr,
        Token::UShr => BinaryOp::UShr,
        Token::Plus => BinaryOp::Add,
        Token::Minus => BinaryOp::Sub,
        Token::Star => BinaryOp::Mul,
        Token::Slash => BinaryOp::Div,
        Token::TildeSlash => BinaryOp::FloorDiv,
        Token::Percent => BinaryOp::Rem,
        Token::StarStar => BinaryOp::Pow,
        _ => return None,
    })
}

/// Returns `(left_bp, right_bp, right_assoc)` for a binary operator — the
/// conventional C/Rust precedence table (see the module docs). `left_bp` is the
/// binding power compared against the caller's `min_bp`; `right_bp` is the
/// minimum for the right operand.
fn binding_power(op: BinaryOp) -> (u8, u8, bool) {
    // Levels 1..=11; higher binds tighter. `**` is right-associative.
    let level: u8 = match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::BitOr => 3,
        BinaryOp::BitXor => 4,
        BinaryOp::BitAnd => 5,
        BinaryOp::Eq | BinaryOp::Ne => 6,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 7,
        BinaryOp::Shl | BinaryOp::Shr | BinaryOp::UShr => 8,
        BinaryOp::Add | BinaryOp::Sub => 9,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem | BinaryOp::FloorDiv => 10,
        BinaryOp::Pow => 11,
    };
    let right_assoc = matches!(op, BinaryOp::Pow);
    (level, level, right_assoc)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extracts the `Function` from an item expected to be a function.
    fn as_function(item: &Item) -> &Function {
        match item {
            Item::Function(f) => f,
            other => panic!("expected a function item, got {other:?}"),
        }
    }

    #[test]
    fn parses_minimal_function_colon_return() {
        let file = parse("fn answer(): i32 { return 42; }").expect("parse");
        assert_eq!(
            file,
            File {
                items: vec![Item::Function(Function {
                    name: "answer".to_string(),
                    visibility: Visibility::Private,
                    modifiers: vec![],
                    params: vec![],
                    return_type: Type::Primitive(Primitive::I32),
                    body: vec![Statement::Return(Some(Expr::IntLiteral("42".to_string())))],
                    meta: Meta::new(),
                })],
            }
        );
    }

    #[test]
    fn parses_legacy_arrow_return_and_any_primitive() {
        // The legacy `-> Type` arrow and any frozen primitive both parse (the
        // old minimal grammar restricted this to `i32`; the ratified grammar
        // accepts the whole frozen set).
        let file = parse("fn a() -> i64 { return 1; }").expect("parse");
        assert_eq!(
            as_function(&file.items[0]).return_type,
            Type::Primitive(Primitive::I64)
        );
    }

    #[test]
    fn defaults_private_and_parses_visibility_modifiers() {
        let f = parse("fn a(): i32 { return 1; }").expect("parse");
        assert_eq!(as_function(&f.items[0]).visibility, Visibility::Private);
        let f = parse("public async fn a(): i32 { return 1; }").expect("parse");
        let func = as_function(&f.items[0]);
        assert_eq!(func.visibility, Visibility::Public);
        assert_eq!(func.modifiers, vec![Modifier::Async]);
    }

    #[test]
    fn const_fn_modifier_vs_const_item() {
        // `const fn` → a function with the Const modifier.
        let f = parse("const fn a(): i32 { return 1; }").expect("parse");
        assert_eq!(as_function(&f.items[0]).modifiers, vec![Modifier::Const]);
        // `const NAME: T = v;` → a const item.
        let f = parse("const MAX: i32 = 100;").expect("parse");
        assert!(matches!(f.items[0], Item::Const { .. }));
    }

    #[test]
    fn precedence_add_mul() {
        // a + b * 2 → a + (b * 2)
        let f = parse("fn f(): i32 { return a + b * 2; }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        let expected = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Ref("a".into())),
            rhs: Box::new(Expr::Binary {
                op: BinaryOp::Mul,
                lhs: Box::new(Expr::Ref("b".into())),
                rhs: Box::new(Expr::IntLiteral("2".into())),
            }),
        };
        assert_eq!(body[0], Statement::Return(Some(expected)));
    }

    #[test]
    fn parens_override_precedence() {
        // (a + b) * 2 → (a + b) * 2
        let f = parse("fn f(): i32 { return (a + b) * 2; }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        let expected = Expr::Binary {
            op: BinaryOp::Mul,
            lhs: Box::new(Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref("a".into())),
                rhs: Box::new(Expr::Ref("b".into())),
            }),
            rhs: Box::new(Expr::IntLiteral("2".into())),
        };
        assert_eq!(body[0], Statement::Return(Some(expected)));
    }

    #[test]
    fn pow_is_right_associative() {
        // a ** b ** c → a ** (b ** c)
        let f = parse("fn f(): i32 { return a ** b ** c; }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        let expected = Expr::Binary {
            op: BinaryOp::Pow,
            lhs: Box::new(Expr::Ref("a".into())),
            rhs: Box::new(Expr::Binary {
                op: BinaryOp::Pow,
                lhs: Box::new(Expr::Ref("b".into())),
                rhs: Box::new(Expr::Ref("c".into())),
            }),
        };
        assert_eq!(body[0], Statement::Return(Some(expected)));
    }

    #[test]
    fn raw_single_arm_string_parses() {
        // `raw` now parses (the deferral is removed): a single-arm string form
        // yields a one-arm `Statement::Raw` with the verbatim contents.
        let f = parse("fn f(): void { raw rust \"x\"; }").expect("raw parses");
        let body = &as_function(&f.items[0]).body;
        assert_eq!(
            body[0],
            Statement::Raw {
                arms: vec![RawArm {
                    target: "rust".into(),
                    version: None,
                    code: "x".into(),
                }],
                default: None,
                meta: Meta::new(),
            }
        );
    }

    #[test]
    fn lambda_vs_parenthesized_expr() {
        // `(x: i32): i32 => { return x; }` is a lambda.
        let f = parse("fn f(): void { let g = (x: i32): i32 => { return x; }; }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        match &body[0] {
            Statement::Let { value: Some(v), .. } => {
                assert!(matches!(v, Expr::Lambda { .. }))
            }
            other => panic!("expected a let binding a lambda, got {other:?}"),
        }
        // `(a + b)` is a parenthesized expression, not a lambda.
        let f = parse("fn f(): i32 { return (a + b); }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        assert!(matches!(
            body[0],
            Statement::Return(Some(Expr::Binary { .. }))
        ));
    }

    #[test]
    fn struct_literal_vs_block_in_if() {
        // The `{` after `if (cond)` opens a block, not a struct literal.
        let f = parse("fn f(): void { if (ok) { return; } }").expect("parse");
        let body = &as_function(&f.items[0]).body;
        assert!(matches!(body[0], Statement::If { .. }));
    }
}
