//! The emitter: walks a [`File`] AST and applies a [`LanguageDef`] to produce
//! target-language source.
//!
//! The emitter contains no target-specific knowledge. Everything it needs comes
//! from the language definition: how each primitive is realized (the capability
//! matrix) and the function surface syntax. This is what makes the engine
//! "dumb" — swapping the language definition swaps the output language.

use crate::ast::{Expr, File, Function, Statement, Type};
use crate::error::EmitError;
use crate::lang::LanguageDef;

/// Transpiles `file` to the target described by `lang`.
///
/// # Errors
///
/// Returns [`EmitError::ForbiddenPrimitive`] if the program uses a primitive
/// the target forbids (or does not list in its capability matrix).
pub fn emit(file: &File, lang: &LanguageDef) -> Result<String, EmitError> {
    let mut out = String::new();
    for (index, function) in file.functions.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        emit_function(function, lang, &mut out)?;
    }
    Ok(out)
}

fn emit_function(
    function: &Function,
    lang: &LanguageDef,
    out: &mut String,
) -> Result<(), EmitError> {
    let syntax = &lang.function_syntax;
    let return_type = resolve_type(&function.return_type, lang)?;

    out.push_str(&syntax.keyword);
    out.push(' ');
    out.push_str(&function.name);
    out.push_str("()");
    if syntax.emit_return_type {
        out.push_str(&syntax.return_type_sep);
        out.push_str(&return_type);
    }
    out.push_str(" {\n");

    for statement in &function.body {
        emit_statement(statement, out);
    }

    out.push('}');
    Ok(())
}

fn emit_statement(statement: &Statement, out: &mut String) {
    match statement {
        Statement::Return(expr) => {
            out.push_str("    return ");
            emit_expr(expr, out);
            out.push_str(";\n");
        }
    }
}

fn emit_expr(expr: &Expr, out: &mut String) {
    match expr {
        // The literal is preserved textually and emitted verbatim; targets in
        // this slice all accept a bare decimal integer literal.
        Expr::IntLiteral(value) => out.push_str(value),
    }
}

/// Resolves a Lamina type to its target spelling via the capability matrix.
fn resolve_type(ty: &Type, lang: &LanguageDef) -> Result<String, EmitError> {
    match ty {
        Type::Primitive(primitive) => match lang.capability(*primitive) {
            Some(capability) => match capability.target_type() {
                Some(name) => Ok(name.to_string()),
                None => Err(EmitError::ForbiddenPrimitive {
                    target: lang.name.clone(),
                    primitive: primitive.as_str().to_string(),
                }),
            },
            None => Err(EmitError::ForbiddenPrimitive {
                target: lang.name.clone(),
                primitive: primitive.as_str().to_string(),
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{Capability, FunctionSyntax};
    use crate::parser::parse;
    use std::collections::HashMap;

    #[test]
    fn forbidden_primitive_is_an_error() {
        let file = parse("fn a() -> i32 { return 1; }").expect("parse");
        let lang = LanguageDef {
            name: "no-ints".to_string(),
            capabilities: HashMap::new(), // i32 not listed => forbidden
            function_syntax: FunctionSyntax {
                keyword: "fn".to_string(),
                return_type_sep: " -> ".to_string(),
                emit_return_type: true,
            },
        };
        let err = emit(&file, &lang).expect_err("should be forbidden");
        assert_eq!(
            err,
            EmitError::ForbiddenPrimitive {
                target: "no-ints".to_string(),
                primitive: "i32".to_string(),
            }
        );
    }

    #[test]
    fn omits_return_type_when_disabled() {
        let file = parse("fn a() -> i32 { return 1; }").expect("parse");
        let lang = LanguageDef {
            name: "dyn".to_string(),
            capabilities: {
                let mut m = HashMap::new();
                m.insert(
                    crate::ast::Primitive::I32,
                    Capability::Widen("number".to_string()),
                );
                m
            },
            function_syntax: FunctionSyntax {
                keyword: "function".to_string(),
                return_type_sep: ": ".to_string(),
                emit_return_type: false,
            },
        };
        let out = emit(&file, &lang).expect("emit");
        assert_eq!(out, "function a() {\n    return 1;\n}");
    }
}
