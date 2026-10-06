//! A lexer for the full ratified Lamina concrete source syntax.
//!
//! This tokenizes the contents of a Lamina `.mdl` *source* document (the
//! grammar authored in `docs/source-syntax.md`). The lexer is
//! whitespace-insensitive apart from using whitespace to separate tokens, so
//! source is stable under reformatting, and it discards `//` line comments
//! (prose). Every token carries a 1-based line/column [`Span`] so the parser
//! can produce precise diagnostics.
//!
//! The lexer is deliberately *dumb*: it does not know which identifiers are
//! keywords versus type names versus plain identifiers beyond the fixed keyword
//! set — the frozen primitive type names (`i32`, `str`, …) stay ordinary
//! [`Token::Ident`]s and are recognized structurally by the parser in type
//! position. Only genuine control-flow / declaration keywords and the boolean /
//! `null` literals are lexed as distinct tokens.
//!
//! ## Resolved ambiguity: `//` vs `~/`
//!
//! The original grammar sketch overloaded `//` as both the floor-division
//! operator and the line-comment introducer. The ratified decision splits them:
//! **`//` is always a line comment** (the conventional C/Rust/JS rule,
//! discarded to end of line), and **floor division is spelled `~/`**
//! (Dart-style). The lexer emits [`Token::TildeSlash`] for `~/` — matched
//! BEFORE a bare `~` ([`Token::Tilde`], bitwise NOT) so the two-char operator
//! is never split — and the parser maps it to
//! [`BinaryOp::FloorDiv`](crate::ast::BinaryOp::FloorDiv) at the multiplicative
//! precedence level.

use crate::ast::RawArm;
use crate::error::ParseError;

/// A 1-based source position (line and column), used for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number (counts Unicode scalar values, not bytes).
    pub column: usize,
}

impl Span {
    /// Builds a span from a 1-based line and column.
    pub fn new(line: usize, column: usize) -> Self {
        Span { line, column }
    }
}

impl std::fmt::Display for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// A lexical token kind (without position). Position is carried alongside in a
/// [`SpannedToken`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    // ---- Declaration / item keywords ----
    /// The `fn` keyword.
    Fn,
    /// The `struct` keyword.
    Struct,
    /// The `enum` keyword.
    Enum,
    /// The `typedef` keyword.
    TypeDef,
    /// The `const` keyword (also a modifier; disambiguated by the parser).
    Const,
    /// The `use` keyword.
    Use,
    /// The `as` keyword (import / use alias).
    As,

    // ---- Statement / control-flow keywords ----
    /// The `let` keyword.
    Let,
    /// The `return` keyword.
    Return,
    /// The `if` keyword.
    If,
    /// The `else` keyword.
    Else,
    /// The `while` keyword.
    While,
    /// The `for` keyword.
    For,
    /// The `foreach` keyword.
    ForEach,
    /// The `in` keyword (used by `foreach`).
    In,
    /// The `switch` keyword.
    Switch,
    /// The `case` keyword.
    Case,
    /// The `default` keyword.
    Default,
    /// The `break` keyword.
    Break,
    /// The `continue` keyword.
    Continue,

    // ---- Tree-core keywords ----
    /// The `node` keyword (tree core).
    Node,
    /// The `text` keyword (tree core).
    Text,

    // ---- Escape-hatch keyword (scanned specially) ----
    /// The `raw` escape hatch, fully scanned at lex time.
    ///
    /// Raw arm bodies are VERBATIM target code (not Lamina), so the lexer
    /// captures the entire construct — every target arm's brace-balanced /
    /// quoted body plus the optional `else` fallback — as opaque text and
    /// emits this single structured token. The parser merely assembles it into
    /// an [`Expr::Raw`](crate::ast::Expr::Raw) /
    /// [`Statement::Raw`](crate::ast::Statement::Raw) /
    /// [`Item::Raw`](crate::ast::Item::Raw) node (same payload at every level).
    RawConstruct {
        /// The target-tagged verbatim arms, in source order.
        arms: Vec<RawArm>,
        /// The `else { … }` fallback's verbatim code, if present.
        default: Option<String>,
    },

    // ---- Literals ----
    /// An identifier or (contextual) type name.
    Ident(String),
    /// An integer literal, kept as text.
    Int(String),
    /// A floating-point literal, kept as text.
    Float(String),
    /// A string literal; the stored value is the already-unescaped contents.
    Str(String),
    /// A character literal; the stored value is the already-unescaped contents.
    Char(String),
    /// The `true` boolean literal.
    True,
    /// The `false` boolean literal.
    False,
    /// The `null` literal.
    Null,

    // ---- Punctuation ----
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `;`
    Semicolon,
    /// `:`
    Colon,
    /// `::`
    ColonColon,
    /// `@`
    At,
    /// `.`
    Dot,
    /// `->` (legacy return-type arrow; the ratified form is `: ret`).
    Arrow,
    /// `=>` (lambda arrow).
    FatArrow,

    // ---- Operators ----
    /// `=`
    Eq,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `**` (exponentiation).
    StarStar,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `!`
    Bang,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `~/` (floor division — Dart-style spelling; `//` stays a line comment).
    TildeSlash,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `>>>` (unsigned/logical right shift).
    UShr,
}

impl Token {
    /// A short human-readable description used in error messages.
    pub fn describe(&self) -> String {
        match self {
            Token::Fn => "keyword `fn`".to_string(),
            Token::Struct => "keyword `struct`".to_string(),
            Token::Enum => "keyword `enum`".to_string(),
            Token::TypeDef => "keyword `typedef`".to_string(),
            Token::Const => "keyword `const`".to_string(),
            Token::Use => "keyword `use`".to_string(),
            Token::As => "keyword `as`".to_string(),
            Token::Let => "keyword `let`".to_string(),
            Token::Return => "keyword `return`".to_string(),
            Token::If => "keyword `if`".to_string(),
            Token::Else => "keyword `else`".to_string(),
            Token::While => "keyword `while`".to_string(),
            Token::For => "keyword `for`".to_string(),
            Token::ForEach => "keyword `foreach`".to_string(),
            Token::In => "keyword `in`".to_string(),
            Token::Switch => "keyword `switch`".to_string(),
            Token::Case => "keyword `case`".to_string(),
            Token::Default => "keyword `default`".to_string(),
            Token::Break => "keyword `break`".to_string(),
            Token::Continue => "keyword `continue`".to_string(),
            Token::Node => "keyword `node`".to_string(),
            Token::Text => "keyword `text`".to_string(),
            Token::RawConstruct { .. } => "a `raw` construct".to_string(),
            Token::Ident(s) => format!("identifier `{s}`"),
            Token::Int(s) => format!("integer `{s}`"),
            Token::Float(s) => format!("float `{s}`"),
            Token::Str(s) => format!("string {s:?}"),
            Token::Char(s) => format!("char {s:?}"),
            Token::True => "`true`".to_string(),
            Token::False => "`false`".to_string(),
            Token::Null => "`null`".to_string(),
            Token::LParen => "`(`".to_string(),
            Token::RParen => "`)`".to_string(),
            Token::LBrace => "`{`".to_string(),
            Token::RBrace => "`}`".to_string(),
            Token::LBracket => "`[`".to_string(),
            Token::RBracket => "`]`".to_string(),
            Token::Comma => "`,`".to_string(),
            Token::Semicolon => "`;`".to_string(),
            Token::Colon => "`:`".to_string(),
            Token::ColonColon => "`::`".to_string(),
            Token::At => "`@`".to_string(),
            Token::Dot => "`.`".to_string(),
            Token::Arrow => "`->`".to_string(),
            Token::FatArrow => "`=>`".to_string(),
            Token::Eq => "`=`".to_string(),
            Token::EqEq => "`==`".to_string(),
            Token::NotEq => "`!=`".to_string(),
            Token::Lt => "`<`".to_string(),
            Token::Le => "`<=`".to_string(),
            Token::Gt => "`>`".to_string(),
            Token::Ge => "`>=`".to_string(),
            Token::Plus => "`+`".to_string(),
            Token::Minus => "`-`".to_string(),
            Token::Star => "`*`".to_string(),
            Token::StarStar => "`**`".to_string(),
            Token::Slash => "`/`".to_string(),
            Token::Percent => "`%`".to_string(),
            Token::AmpAmp => "`&&`".to_string(),
            Token::PipePipe => "`||`".to_string(),
            Token::Bang => "`!`".to_string(),
            Token::Amp => "`&`".to_string(),
            Token::Pipe => "`|`".to_string(),
            Token::Caret => "`^`".to_string(),
            Token::Tilde => "`~`".to_string(),
            Token::TildeSlash => "`~/`".to_string(),
            Token::Shl => "`<<`".to_string(),
            Token::Shr => "`>>`".to_string(),
            Token::UShr => "`>>>`".to_string(),
        }
    }
}

/// A [`Token`] paired with its 1-based source [`Span`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken {
    /// The token kind.
    pub token: Token,
    /// The 1-based position of the token's first character.
    pub span: Span,
}

/// Resolves a keyword/literal identifier spelling to its reserved [`Token`], or
/// `None` if `word` is an ordinary identifier (including a primitive type name,
/// which stays an [`Token::Ident`] and is recognized structurally in type
/// position).
fn keyword(word: &str) -> Option<Token> {
    Some(match word {
        "fn" => Token::Fn,
        "struct" => Token::Struct,
        "enum" => Token::Enum,
        "typedef" => Token::TypeDef,
        "const" => Token::Const,
        "use" => Token::Use,
        "as" => Token::As,
        "let" => Token::Let,
        "return" => Token::Return,
        "if" => Token::If,
        "else" => Token::Else,
        "while" => Token::While,
        "for" => Token::For,
        "foreach" => Token::ForEach,
        "in" => Token::In,
        "switch" => Token::Switch,
        "case" => Token::Case,
        "default" => Token::Default,
        "break" => Token::Break,
        "continue" => Token::Continue,
        "node" => Token::Node,
        "text" => Token::Text,
        "true" => Token::True,
        "false" => Token::False,
        "null" => Token::Null,
        _ => return None,
    })
}

/// A cursor over the source that tracks byte offset and 1-based line/column.
struct Cursor<'a> {
    src: &'a str,
    chars: Vec<(usize, char)>,
    /// Index into `chars`.
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Cursor<'a> {
    fn new(src: &'a str) -> Self {
        Cursor {
            src,
            chars: src.char_indices().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).map(|&(_, c)| c)
    }

    fn peek_at(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).map(|&(_, c)| c)
    }

    fn span(&self) -> Span {
        Span::new(self.line, self.column)
    }

    /// Advances one scalar, updating line/column. Returns the consumed char.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    /// The byte offset of the current scalar (or `src.len()` at end).
    fn byte_offset(&self) -> usize {
        self.chars
            .get(self.pos)
            .map(|&(b, _)| b)
            .unwrap_or(self.src.len())
    }

    /// Skips ASCII whitespace and `//` line comments between the STRUCTURAL
    /// tokens of a `raw` construct (target names, braces, `else`). Verbatim arm
    /// bodies are captured separately and are never subject to this.
    fn skip_raw_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => {
                    self.bump();
                    self.bump();
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }
}

/// Scans a complete `raw` escape-hatch construct, starting just AFTER the
/// `raw` keyword, into a single [`Token::RawConstruct`].
///
/// Two surface forms are accepted (per `docs/source-syntax.md`):
///
/// - **single-arm**: `raw <target> [<ver>] "string"` or
///   `raw <target> [<ver>] { verbatim-block }` — one arm, no `else`.
/// - **grouped**: `raw { <target> [<ver>] { … } … [else { … }] }` — many arms
///   plus an optional `else` fallback.
///
/// A `<ver>` is an OPAQUE npm/Cargo-style constraint (a comparator
/// `^ ~ >= <= > < =` or a hyphen range, plus a version token like `3.10` or
/// `2021`); it is captured verbatim into [`RawArm::version`] but NOT matched on
/// — target-only matching happens later, in `lower`.
///
/// The one subtlety is **verbatim block capture**: an arm body is target code,
/// NOT Lamina, so it is captured as the opaque, brace-balanced text between its
/// delimiting braces (see [`capture_raw_block`]).
fn lex_raw_construct(cursor: &mut Cursor<'_>, span: Span) -> Result<Token, ParseError> {
    cursor.skip_raw_trivia();
    match cursor.peek() {
        // Grouped form: `raw { arm* else? }`.
        Some('{') => {
            cursor.bump();
            let mut arms = Vec::new();
            let mut default = None;
            loop {
                cursor.skip_raw_trivia();
                match cursor.peek() {
                    Some('}') => {
                        cursor.bump();
                        break;
                    }
                    None => return Err(ParseError::UnterminatedLiteral {
                        kind: "raw",
                        line: span.line,
                        column: span.column,
                    }),
                    _ => {}
                }
                let head = lex_raw_arm_head(cursor)?;
                cursor.skip_raw_trivia();
                let body = capture_raw_block(cursor, span)?;
                match head {
                    RawArmHead::Else => default = Some(body),
                    RawArmHead::Target { target, version } => arms.push(RawArm {
                        target,
                        version,
                        code: body,
                    }),
                }
            }
            Ok(Token::RawConstruct { arms, default })
        }
        // Single-arm form: `raw <target> [ver] ("str" | { block })`.
        _ => {
            let (target, version) = match lex_raw_arm_head(cursor)? {
                RawArmHead::Target { target, version } => (target, version),
                RawArmHead::Else => {
                    return Err(ParseError::UnterminatedLiteral {
                        kind: "raw",
                        line: span.line,
                        column: span.column,
                    })
                }
            };
            cursor.skip_raw_trivia();
            let code = match cursor.peek() {
                Some('"') => capture_raw_string(cursor, span)?,
                Some('{') => capture_raw_block(cursor, span)?,
                _ => {
                    return Err(ParseError::UnterminatedLiteral {
                        kind: "raw",
                        line: span.line,
                        column: span.column,
                    })
                }
            };
            Ok(Token::RawConstruct {
                arms: vec![RawArm {
                    target,
                    version,
                    code,
                }],
                default: None,
            })
        }
    }
}

/// The parsed head of one `raw` arm: either the `else` fallback marker or a
/// target name with an optional opaque version constraint.
enum RawArmHead {
    /// The `else { … }` fallback arm.
    Else,
    /// A target arm head (`<target> [<version>]`).
    Target {
        /// The arm's target language name.
        target: String,
        /// The opaque version constraint, if any.
        version: Option<String>,
    },
}

/// Lexes one `raw` arm head — the `else` keyword, or a `<target> [<version>]`
/// pair — leaving the cursor just before the arm body (`{` or `"`).
fn lex_raw_arm_head(cursor: &mut Cursor<'_>) -> Result<RawArmHead, ParseError> {
    cursor.skip_raw_trivia();
    let word = lex_raw_word(cursor);
    if word.is_empty() {
        return Err(ParseError::UnexpectedChar {
            ch: cursor.peek().unwrap_or(' '),
            offset: cursor.byte_offset(),
        });
    }
    if word == "else" {
        return Ok(RawArmHead::Else);
    }
    cursor.skip_raw_trivia();
    let version = lex_raw_version(cursor);
    Ok(RawArmHead::Target {
        target: word,
        version,
    })
}

/// Lexes an identifier-shaped word (`[A-Za-z_][A-Za-z0-9_.]*`) in raw-head
/// position — the target name. A `.` is permitted so a dotted target band like
/// `python3.13` reads as one word.
fn lex_raw_word(cursor: &mut Cursor<'_>) -> String {
    let mut word = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            word.push(c);
            cursor.bump();
        } else {
            break;
        }
    }
    word
}

/// Lexes an OPTIONAL opaque npm/Cargo-style version constraint following a
/// target name (e.g. `>= 3.10`, `^1.2`, `1.0 - 2.0`), returning the verbatim
/// constraint text or `None` when the next non-trivia char begins the arm body
/// (`{` or `"`).
///
/// The comparator grammar is recognized only loosely: this collects the run of
/// constraint characters (comparators `^ ~ > < = -` and version tokens
/// `[0-9A-Za-z._]`, plus internal spaces for hyphen ranges) up to the body
/// delimiter. The token is OPAQUE — it is stored, never parsed or compared.
fn lex_raw_version(cursor: &mut Cursor<'_>) -> Option<String> {
    // A version constraint only begins with a comparator or a version-token
    // char; `{` or `"` means there is no constraint.
    let is_constraint_start = |c: char| {
        matches!(c, '^' | '~' | '>' | '<' | '=' | '-')
            || c.is_ascii_digit()
            || c.is_ascii_alphabetic()
    };
    match cursor.peek() {
        Some(c) if is_constraint_start(c) => {}
        _ => return None,
    }
    let mut text = String::new();
    // Collect constraint chars; allow single interior spaces (hyphen ranges)
    // but stop at the body delimiter or a newline.
    loop {
        match cursor.peek() {
            Some('{') | Some('"') | None => break,
            Some('\n') => break,
            Some(c) if c.is_ascii_whitespace() => {
                // Peek past the whitespace: keep it only if more constraint
                // follows on the same construct (a hyphen range `A - B`).
                let mut lookahead = 1;
                while matches!(cursor.peek_at(lookahead), Some(c2) if c2 == ' ' || c2 == '\t') {
                    lookahead += 1;
                }
                match cursor.peek_at(lookahead) {
                    Some(c2) if c2 != '{' && c2 != '"' && c2 != '\n' => {
                        text.push(' ');
                        for _ in 0..lookahead {
                            cursor.bump();
                        }
                    }
                    _ => break,
                }
            }
            Some(c) => {
                text.push(c);
                cursor.bump();
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Captures a VERBATIM, brace-balanced arm body, starting at its opening `{`
/// and consuming through the matching `}`. Returns the inner text (between the
/// braces), with one leading and one trailing newline trimmed if present, but
/// otherwise byte-for-byte as authored.
///
/// Target code is NOT Lamina, so this does not lex it. To avoid miscounting
/// braces that appear inside the target's own string/char literals or comments,
/// the scan skips over: double-quoted strings, single-quoted chars (both with
/// `\`-escapes), `//` and `#` line comments, and `/* … */` block comments. A
/// `{`/`}` inside any of those does not change depth. This covers the C-family,
/// Rust, Python, Go, and JS/TS bodies the kernel targets; a body with braces
/// hidden in a more exotic literal form should use the `"string"` arm instead.
fn capture_raw_block(cursor: &mut Cursor<'_>, span: Span) -> Result<String, ParseError> {
    if cursor.peek() != Some('{') {
        return Err(ParseError::ExpectedAt {
            expected: "`{` to open a raw arm body".to_string(),
            found: cursor
                .peek()
                .map_or_else(|| "end of input".to_string(), |c| format!("`{c}`")),
            line: cursor.line,
            column: cursor.column,
        });
    }
    cursor.bump(); // consume opening `{`
    let mut depth: usize = 1;
    let mut body = String::new();
    while let Some(c) = cursor.peek() {
        match c {
            '{' => {
                depth += 1;
                body.push(c);
                cursor.bump();
            }
            '}' => {
                depth -= 1;
                cursor.bump();
                if depth == 0 {
                    return Ok(trim_block_edges(&body));
                }
                body.push('}');
            }
            '"' | '\'' => capture_verbatim_literal(cursor, &mut body, c),
            '/' if cursor.peek_at(1) == Some('/') => capture_verbatim_line_comment(cursor, &mut body),
            '#' => capture_verbatim_line_comment(cursor, &mut body),
            '/' if cursor.peek_at(1) == Some('*') => {
                capture_verbatim_block_comment(cursor, &mut body)
            }
            _ => {
                body.push(c);
                cursor.bump();
            }
        }
    }
    Err(ParseError::UnterminatedLiteral {
        kind: "raw",
        line: span.line,
        column: span.column,
    })
}

/// Trims a single leading and trailing newline (and the trailing line's
/// indentation) from a captured block body, so `rust { return x; }` written on
/// one line and the multi-line brace form both yield clean inner text without
/// the author's delimiter whitespace. Interior formatting is preserved.
fn trim_block_edges(body: &str) -> String {
    let trimmed = body.strip_prefix('\n').unwrap_or(body);
    // Drop a trailing newline + any trailing spaces/tabs (the closing brace's
    // own indentation line), but keep interior content intact.
    let bytes_trimmed = trimmed.trim_end_matches([' ', '\t']);
    let bytes_trimmed = bytes_trimmed.strip_suffix('\n').unwrap_or(trimmed);
    bytes_trimmed.trim_matches(|c| c == ' ' || c == '\t').to_string()
}

/// Appends a verbatim string/char literal (opened by `quote`) to `body`,
/// honoring `\`-escapes, so a brace inside the literal is not counted.
fn capture_verbatim_literal(cursor: &mut Cursor<'_>, body: &mut String, quote: char) {
    body.push(quote);
    cursor.bump();
    while let Some(c) = cursor.peek() {
        body.push(c);
        cursor.bump();
        if c == '\\' {
            if let Some(escaped) = cursor.peek() {
                body.push(escaped);
                cursor.bump();
            }
        } else if c == quote {
            break;
        }
    }
}

/// Appends a verbatim line comment (through end of line) to `body`.
fn capture_verbatim_line_comment(cursor: &mut Cursor<'_>, body: &mut String) {
    while let Some(c) = cursor.peek() {
        if c == '\n' {
            break;
        }
        body.push(c);
        cursor.bump();
    }
}

/// Appends a verbatim `/* … */` block comment to `body`.
fn capture_verbatim_block_comment(cursor: &mut Cursor<'_>, body: &mut String) {
    body.push('/');
    body.push('*');
    cursor.bump();
    cursor.bump();
    while let Some(c) = cursor.peek() {
        if c == '*' && cursor.peek_at(1) == Some('/') {
            body.push('*');
            body.push('/');
            cursor.bump();
            cursor.bump();
            break;
        }
        body.push(c);
        cursor.bump();
    }
}

/// Captures a VERBATIM single-arm raw string body (`raw target "contents"`),
/// honoring `\`-escapes while scanning, and returns the UNESCAPED contents
/// (`\n`, `\t`, `\\`, `\"` resolved) so the stored code matches how the AST
/// stores literal contents elsewhere.
fn capture_raw_string(cursor: &mut Cursor<'_>, span: Span) -> Result<String, ParseError> {
    // Reuse the ordinary string lexer: a raw string arm IS an ordinary
    // double-quoted literal (the target code is its contents), unescaped once.
    lex_string(cursor, span)
}

/// Tokenizes `src` into a flat list of [`SpannedToken`]s.
///
/// `//` line comments and all ASCII whitespace are discarded. String and
/// character literals are unescaped (so the stored contents are the semantic
/// value, matching how the AST stores literal contents).
///
/// # Errors
///
/// Returns [`ParseError::UnexpectedChar`] for a character that is not part of
/// the grammar, or [`ParseError::UnterminatedLiteral`] for an unclosed string
/// or character literal.
pub fn lex(src: &str) -> Result<Vec<SpannedToken>, ParseError> {
    let mut cursor = Cursor::new(src);
    let mut tokens = Vec::new();

    while let Some(c) = cursor.peek() {
        // Whitespace.
        if c.is_ascii_whitespace() {
            cursor.bump();
            continue;
        }

        // `//` begins a line comment (consumed to end of line). The ratified
        // grammar separates the two historical meanings of `//`: it is ALWAYS a
        // line comment here (the conventional C/Rust/JS rule), while floor
        // division is spelled `~/` (see the `~` arm below and the module docs).
        if c == '/' && cursor.peek_at(1) == Some('/') {
            // Consume both slashes and the rest of the line.
            cursor.bump();
            cursor.bump();
            while let Some(ch) = cursor.peek() {
                if ch == '\n' {
                    break;
                }
                cursor.bump();
            }
            continue;
        }

        let span = cursor.span();

        match c {
            '(' => punct(&mut cursor, &mut tokens, Token::LParen),
            ')' => punct(&mut cursor, &mut tokens, Token::RParen),
            '{' => punct(&mut cursor, &mut tokens, Token::LBrace),
            '}' => punct(&mut cursor, &mut tokens, Token::RBrace),
            '[' => punct(&mut cursor, &mut tokens, Token::LBracket),
            ']' => punct(&mut cursor, &mut tokens, Token::RBracket),
            ',' => punct(&mut cursor, &mut tokens, Token::Comma),
            ';' => punct(&mut cursor, &mut tokens, Token::Semicolon),
            '@' => punct(&mut cursor, &mut tokens, Token::At),
            '~' => {
                // `~/` is floor division (Dart-style); a bare `~` is bitwise
                // NOT. The two-char form is matched FIRST so `~/` never lexes as
                // `~` followed by `/`.
                cursor.bump();
                if cursor.peek() == Some('/') {
                    cursor.bump();
                    push(&mut tokens, Token::TildeSlash, span);
                } else {
                    push(&mut tokens, Token::Tilde, span);
                }
            }
            '^' => punct(&mut cursor, &mut tokens, Token::Caret),
            '%' => punct(&mut cursor, &mut tokens, Token::Percent),
            '.' => punct(&mut cursor, &mut tokens, Token::Dot),
            ':' => {
                cursor.bump();
                if cursor.peek() == Some(':') {
                    cursor.bump();
                    push(&mut tokens, Token::ColonColon, span);
                } else {
                    push(&mut tokens, Token::Colon, span);
                }
            }
            '=' => {
                cursor.bump();
                match cursor.peek() {
                    Some('=') => {
                        cursor.bump();
                        push(&mut tokens, Token::EqEq, span);
                    }
                    Some('>') => {
                        cursor.bump();
                        push(&mut tokens, Token::FatArrow, span);
                    }
                    _ => push(&mut tokens, Token::Eq, span),
                }
            }
            '!' => {
                cursor.bump();
                if cursor.peek() == Some('=') {
                    cursor.bump();
                    push(&mut tokens, Token::NotEq, span);
                } else {
                    push(&mut tokens, Token::Bang, span);
                }
            }
            '+' => punct(&mut cursor, &mut tokens, Token::Plus),
            '-' => {
                cursor.bump();
                if cursor.peek() == Some('>') {
                    cursor.bump();
                    push(&mut tokens, Token::Arrow, span);
                } else {
                    push(&mut tokens, Token::Minus, span);
                }
            }
            '*' => {
                cursor.bump();
                if cursor.peek() == Some('*') {
                    cursor.bump();
                    push(&mut tokens, Token::StarStar, span);
                } else {
                    push(&mut tokens, Token::Star, span);
                }
            }
            '/' => {
                // `//` was handled above as a comment; a lone `/` is division.
                cursor.bump();
                push(&mut tokens, Token::Slash, span);
            }
            '&' => {
                cursor.bump();
                if cursor.peek() == Some('&') {
                    cursor.bump();
                    push(&mut tokens, Token::AmpAmp, span);
                } else {
                    push(&mut tokens, Token::Amp, span);
                }
            }
            '|' => {
                cursor.bump();
                if cursor.peek() == Some('|') {
                    cursor.bump();
                    push(&mut tokens, Token::PipePipe, span);
                } else {
                    push(&mut tokens, Token::Pipe, span);
                }
            }
            '<' => {
                cursor.bump();
                match cursor.peek() {
                    Some('=') => {
                        cursor.bump();
                        push(&mut tokens, Token::Le, span);
                    }
                    Some('<') => {
                        cursor.bump();
                        push(&mut tokens, Token::Shl, span);
                    }
                    _ => push(&mut tokens, Token::Lt, span),
                }
            }
            '>' => {
                cursor.bump();
                match cursor.peek() {
                    Some('=') => {
                        cursor.bump();
                        push(&mut tokens, Token::Ge, span);
                    }
                    Some('>') => {
                        cursor.bump();
                        if cursor.peek() == Some('>') {
                            cursor.bump();
                            push(&mut tokens, Token::UShr, span);
                        } else {
                            push(&mut tokens, Token::Shr, span);
                        }
                    }
                    _ => push(&mut tokens, Token::Gt, span),
                }
            }
            '"' => {
                let value = lex_string(&mut cursor, span)?;
                push(&mut tokens, Token::Str(value), span);
            }
            '\'' => {
                let value = lex_char(&mut cursor, span)?;
                push(&mut tokens, Token::Char(value), span);
            }
            _ if c.is_ascii_digit() => {
                let token = lex_number(&mut cursor);
                push(&mut tokens, token, span);
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let word = lex_word(&mut cursor);
                // The `raw` escape hatch is special: its arm bodies are VERBATIM
                // target code (not Lamina), so they must be captured as opaque
                // text rather than lexed as Lamina tokens. On seeing `raw` the
                // lexer scans the whole construct char-by-char and emits a
                // single structured [`Token::RawConstruct`] carrying the parsed
                // arms; the parser only assembles it into the right node.
                if word == "raw" {
                    let construct = lex_raw_construct(&mut cursor, span)?;
                    push(&mut tokens, construct, span);
                } else {
                    let token = keyword(&word).unwrap_or(Token::Ident(word));
                    push(&mut tokens, token, span);
                }
            }
            _ => {
                return Err(ParseError::UnexpectedChar {
                    ch: c,
                    offset: cursor.byte_offset(),
                });
            }
        }
    }

    Ok(tokens)
}

/// Pushes `token` with its `span`.
fn push(tokens: &mut Vec<SpannedToken>, token: Token, span: Span) {
    tokens.push(SpannedToken { token, span });
}

/// Consumes a single-character punctuation token.
fn punct(cursor: &mut Cursor<'_>, tokens: &mut Vec<SpannedToken>, token: Token) {
    let span = cursor.span();
    cursor.bump();
    push(tokens, token, span);
}

/// Lexes an identifier / keyword word (`[A-Za-z_][A-Za-z0-9_]*`).
fn lex_word(cursor: &mut Cursor<'_>) -> String {
    let mut word = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
            cursor.bump();
        } else {
            break;
        }
    }
    word
}

/// Lexes a numeric literal, returning either [`Token::Int`] or [`Token::Float`].
///
/// A float is a run of digits containing a single `.` followed by at least one
/// digit (so `1.0`), or a digit run with an exponent is not supported in this
/// slice. A trailing `.` with no fractional digit is left as an `Int` followed
/// by a `Dot` (so `x.0` field access still works) — but a `.` directly between
/// digits is the fractional point.
fn lex_number(cursor: &mut Cursor<'_>) -> Token {
    let mut text = String::new();
    while let Some(c) = cursor.peek() {
        if c.is_ascii_digit() {
            text.push(c);
            cursor.bump();
        } else {
            break;
        }
    }
    // A fractional part: `.` immediately followed by a digit.
    if cursor.peek() == Some('.') && cursor.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
        text.push('.');
        cursor.bump();
        while let Some(c) = cursor.peek() {
            if c.is_ascii_digit() {
                text.push(c);
                cursor.bump();
            } else {
                break;
            }
        }
        return Token::Float(text);
    }
    Token::Int(text)
}

/// Lexes a double-quoted string literal, unescaping the standard escapes.
fn lex_string(cursor: &mut Cursor<'_>, open: Span) -> Result<String, ParseError> {
    cursor.bump(); // opening quote
    let mut value = String::new();
    loop {
        match cursor.peek() {
            None => {
                return Err(ParseError::UnterminatedLiteral {
                    kind: "string",
                    line: open.line,
                    column: open.column,
                })
            }
            Some('"') => {
                cursor.bump();
                return Ok(value);
            }
            Some('\\') => {
                cursor.bump();
                let escaped = unescape(cursor, open, "string")?;
                value.push(escaped);
            }
            Some(c) => {
                value.push(c);
                cursor.bump();
            }
        }
    }
}

/// Lexes a single-quoted character literal, unescaping the standard escapes.
fn lex_char(cursor: &mut Cursor<'_>, open: Span) -> Result<String, ParseError> {
    cursor.bump(); // opening quote
    let value = match cursor.peek() {
        None | Some('\'') => {
            return Err(ParseError::UnterminatedLiteral {
                kind: "char",
                line: open.line,
                column: open.column,
            })
        }
        Some('\\') => {
            cursor.bump();
            unescape(cursor, open, "char")?
        }
        Some(c) => {
            cursor.bump();
            c
        }
    };
    match cursor.peek() {
        Some('\'') => {
            cursor.bump();
            Ok(value.to_string())
        }
        _ => Err(ParseError::UnterminatedLiteral {
            kind: "char",
            line: open.line,
            column: open.column,
        }),
    }
}

/// Unescapes the character following a `\` in a string/char literal. The
/// closed escape set is `\n \t \r \\ \" \' \0`.
fn unescape(cursor: &mut Cursor<'_>, open: Span, kind: &'static str) -> Result<char, ParseError> {
    let c = cursor.peek().ok_or(ParseError::UnterminatedLiteral {
        kind,
        line: open.line,
        column: open.column,
    })?;
    let resolved = match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '\\' => '\\',
        '"' => '"',
        '\'' => '\'',
        '0' => '\0',
        other => other,
    };
    cursor.bump();
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Token> {
        lex(src)
            .expect("should lex")
            .into_iter()
            .map(|s| s.token)
            .collect()
    }

    #[test]
    fn lexes_minimal_function_colon_return() {
        assert_eq!(
            kinds("fn answer(): i32 { return 42; }"),
            vec![
                Token::Fn,
                Token::Ident("answer".into()),
                Token::LParen,
                Token::RParen,
                Token::Colon,
                Token::Ident("i32".into()),
                Token::LBrace,
                Token::Return,
                Token::Int("42".into()),
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lexes_legacy_arrow_return() {
        assert_eq!(
            kinds("fn a() -> i32 { return 1; }"),
            vec![
                Token::Fn,
                Token::Ident("a".into()),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("i32".into()),
                Token::LBrace,
                Token::Return,
                Token::Int("1".into()),
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lexes_all_operators() {
        assert_eq!(
            kinds("+ - * ** / % == != < <= > >= && || ! & | ^ ~ << >> >>>"),
            vec![
                Token::Plus,
                Token::Minus,
                Token::Star,
                Token::StarStar,
                Token::Slash,
                Token::Percent,
                Token::EqEq,
                Token::NotEq,
                Token::Lt,
                Token::Le,
                Token::Gt,
                Token::Ge,
                Token::AmpAmp,
                Token::PipePipe,
                Token::Bang,
                Token::Amp,
                Token::Pipe,
                Token::Caret,
                Token::Tilde,
                Token::Shl,
                Token::Shr,
                Token::UShr,
            ]
        );
    }

    #[test]
    fn double_slash_is_always_a_comment() {
        // Resolved ambiguity: `//` is unconditionally a line comment (the
        // conventional C/Rust/JS rule), so `a // b` lexes to just `a` with the
        // rest discarded. Floor-division has no authored-source spelling.
        assert_eq!(kinds("a // b"), vec![Token::Ident("a".into())]);
    }

    #[test]
    fn line_comment_is_discarded() {
        assert_eq!(
            kinds("fn a() { // a comment\n return; }"),
            vec![
                Token::Fn,
                Token::Ident("a".into()),
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::Return,
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lexes_float_and_field_dot() {
        assert_eq!(kinds("1.0"), vec![Token::Float("1.0".into())]);
        assert_eq!(
            kinds("a.b"),
            vec![Token::Ident("a".into()), Token::Dot, Token::Ident("b".into())]
        );
    }

    #[test]
    fn lexes_string_and_char_with_escapes() {
        assert_eq!(kinds(r#""a\nb""#), vec![Token::Str("a\nb".into())]);
        assert_eq!(kinds(r"'\''"), vec![Token::Char("'".into())]);
    }

    #[test]
    fn lexes_keywords_and_literals() {
        assert_eq!(
            kinds("true false null as in"),
            vec![Token::True, Token::False, Token::Null, Token::As, Token::In]
        );
    }

    #[test]
    fn colon_colon_and_fat_arrow() {
        assert_eq!(
            kinds("a::b => c"),
            vec![
                Token::Ident("a".into()),
                Token::ColonColon,
                Token::Ident("b".into()),
                Token::FatArrow,
                Token::Ident("c".into()),
            ]
        );
    }

    #[test]
    fn tracks_line_and_column() {
        let toks = lex("fn\n  a").expect("lex");
        assert_eq!(toks[0].span, Span::new(1, 1));
        assert_eq!(toks[1].span, Span::new(2, 3));
    }

    #[test]
    fn rejects_unexpected_char() {
        let err = lex("fn a() { return $; }").expect_err("should reject $");
        assert!(matches!(err, ParseError::UnexpectedChar { ch: '$', .. }));
    }

    #[test]
    fn rejects_unterminated_string() {
        let err = lex("\"abc").expect_err("unterminated");
        assert!(matches!(err, ParseError::UnterminatedLiteral { kind: "string", .. }));
    }
}
