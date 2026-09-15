//! A strict recursive-descent parser for the minimal Lamina inner-code grammar.
//!
//! Grammar (this slice):
//! ```text
//! file      := function*
//! function  := "fn" IDENT "(" ")" "->" type "{" statement* "}"
//! type      := "i32"
//! statement := "return" expr ";"
//! expr      := INT
//! ```

use crate::ast::{Expr, File, Function, Primitive, Statement, Type};
use crate::error::ParseError;
use crate::lexer::{lex, Token};

/// Parses Lamina inner-code source into a [`File`] AST.
///
/// # Errors
///
/// Returns a [`ParseError`] if the source does not conform to the minimal
/// grammar.
pub fn parse(src: &str) -> Result<File, ParseError> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, pos: 0 };
    parser.parse_file()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn found_description(&self) -> String {
        match self.peek() {
            Some(token) => token.describe(),
            None => "end of input".to_string(),
        }
    }

    fn expect(&mut self, expected: &Token, what: &str) -> Result<(), ParseError> {
        match self.peek() {
            Some(token) if token == expected => {
                self.pos += 1;
                Ok(())
            }
            _ => Err(ParseError::Expected {
                expected: what.to_string(),
                found: self.found_description(),
            }),
        }
    }

    fn parse_file(&mut self) -> Result<File, ParseError> {
        let mut functions = Vec::new();
        while self.peek().is_some() {
            functions.push(self.parse_function()?);
        }
        Ok(File { functions })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(&Token::Fn, "keyword `fn`")?;

        let name = match self.advance() {
            Some(Token::Ident(name)) => name,
            _ => {
                return Err(ParseError::Expected {
                    expected: "function name".to_string(),
                    found: self.found_at(self.pos.saturating_sub(1)),
                })
            }
        };

        self.expect(&Token::LParen, "`(`")?;
        self.expect(&Token::RParen, "`)`")?;
        self.expect(&Token::Arrow, "`->`")?;

        let return_type = self.parse_type()?;

        self.expect(&Token::LBrace, "`{`")?;
        let mut body = Vec::new();
        while self.peek() != Some(&Token::RBrace) {
            if self.peek().is_none() {
                return Err(ParseError::Expected {
                    expected: "`}` to close function body".to_string(),
                    found: "end of input".to_string(),
                });
            }
            body.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace, "`}`")?;

        Ok(Function {
            name,
            return_type,
            body,
        })
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        match self.advance() {
            Some(Token::Ident(name)) => match name.as_str() {
                "i32" => Ok(Type::Primitive(Primitive::I32)),
                _ => Err(ParseError::UnknownType { name }),
            },
            _ => Err(ParseError::Expected {
                expected: "a type".to_string(),
                found: self.found_at(self.pos.saturating_sub(1)),
            }),
        }
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        self.expect(&Token::Return, "keyword `return`")?;
        let expr = self.parse_expr()?;
        self.expect(&Token::Semicolon, "`;`")?;
        Ok(Statement::Return(expr))
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        match self.advance() {
            Some(Token::Int(value)) => Ok(Expr::IntLiteral(value)),
            _ => Err(ParseError::Expected {
                expected: "an integer literal".to_string(),
                found: self.found_at(self.pos.saturating_sub(1)),
            }),
        }
    }

    /// Describes the token at `index`, for error messages, without consuming.
    fn found_at(&self, index: usize) -> String {
        match self.tokens.get(index) {
            Some(token) => token.describe(),
            None => "end of input".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_function() {
        let file = parse("fn answer() -> i32 { return 42; }").expect("should parse");
        assert_eq!(
            file,
            File {
                functions: vec![Function {
                    name: "answer".to_string(),
                    return_type: Type::Primitive(Primitive::I32),
                    body: vec![Statement::Return(Expr::IntLiteral("42".to_string()))],
                }],
            }
        );
    }

    #[test]
    fn parses_multiple_functions() {
        let file =
            parse("fn a() -> i32 { return 1; } fn b() -> i32 { return 2; }").expect("should parse");
        assert_eq!(file.functions.len(), 2);
        assert_eq!(file.functions[0].name, "a");
        assert_eq!(file.functions[1].name, "b");
    }

    #[test]
    fn rejects_unknown_type() {
        let err = parse("fn a() -> i64 { return 1; }").expect_err("i64 not supported yet");
        assert_eq!(
            err,
            ParseError::UnknownType {
                name: "i64".to_string()
            }
        );
    }

    #[test]
    fn rejects_missing_semicolon() {
        let err = parse("fn a() -> i32 { return 1 }").expect_err("missing semicolon");
        match err {
            ParseError::Expected { expected, .. } => assert!(expected.contains(';')),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_return_type() {
        let err = parse("fn a() { return 1; }").expect_err("missing arrow/type");
        match err {
            ParseError::Expected { .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
