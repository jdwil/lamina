//! A strict lexer for the minimal Lamina inner-code grammar.
//!
//! This tokenizes the contents of a `lamina` code block. The grammar is
//! deliberately explicit and brace-delimited (see the format decision): the
//! lexer is whitespace-insensitive apart from using whitespace to separate
//! tokens, so source is stable under reformatting.

use crate::error::ParseError;

/// A lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// The `fn` keyword.
    Fn,
    /// The `return` keyword.
    Return,
    /// An identifier or type name.
    Ident(String),
    /// An integer literal, kept as text.
    Int(String),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `->`
    Arrow,
    /// `;`
    Semicolon,
}

impl Token {
    /// A short human-readable description used in error messages.
    pub fn describe(&self) -> String {
        match self {
            Token::Fn => "keyword `fn`".to_string(),
            Token::Return => "keyword `return`".to_string(),
            Token::Ident(s) => format!("identifier `{s}`"),
            Token::Int(s) => format!("integer `{s}`"),
            Token::LParen => "`(`".to_string(),
            Token::RParen => "`)`".to_string(),
            Token::LBrace => "`{`".to_string(),
            Token::RBrace => "`}`".to_string(),
            Token::Arrow => "`->`".to_string(),
            Token::Semicolon => "`;`".to_string(),
        }
    }
}

/// Tokenizes `src` into a flat list of [`Token`]s.
///
/// # Errors
///
/// Returns [`ParseError::UnexpectedChar`] if the source contains a character
/// that is not part of the minimal grammar.
pub fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i] as char;

        // Skip ASCII whitespace.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        match c {
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '{' => {
                tokens.push(Token::LBrace);
                i += 1;
            }
            '}' => {
                tokens.push(Token::RBrace);
                i += 1;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
            }
            '-' if i + 1 < bytes.len() && bytes[i + 1] == b'>' => {
                tokens.push(Token::Arrow);
                i += 2;
            }
            _ if c.is_ascii_digit() => {
                let start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
                tokens.push(Token::Int(src[start..i].to_string()));
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
                let token = match word {
                    "fn" => Token::Fn,
                    "return" => Token::Return,
                    _ => Token::Ident(word.to_string()),
                };
                tokens.push(token);
            }
            _ => {
                return Err(ParseError::UnexpectedChar { ch: c, offset: i });
            }
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_minimal_function() {
        let tokens = lex("fn answer() -> i32 { return 42; }").expect("should lex");
        assert_eq!(
            tokens,
            vec![
                Token::Fn,
                Token::Ident("answer".to_string()),
                Token::LParen,
                Token::RParen,
                Token::Arrow,
                Token::Ident("i32".to_string()),
                Token::LBrace,
                Token::Return,
                Token::Int("42".to_string()),
                Token::Semicolon,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn is_whitespace_insensitive() {
        let compact = lex("fn a()->i32{return 1;}").expect("compact should lex");
        let spaced = lex("fn   a (  ) ->  i32  {\n  return 1 ;\n}").expect("spaced should lex");
        assert_eq!(compact, spaced);
    }

    #[test]
    fn rejects_unexpected_char() {
        let err = lex("fn a() -> i32 { return @; }").expect_err("should reject @");
        assert_eq!(err, ParseError::UnexpectedChar { ch: '@', offset: 23 });
    }
}
