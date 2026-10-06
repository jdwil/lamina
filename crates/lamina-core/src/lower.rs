//! Target-aware lowering: the minimal pre-emit pass.
//!
//! Between parsing and emission the engine runs ONE lowering job today:
//! **raw-arm resolution**. A `raw` node in the AST is generic and target-keyed
//! — it holds every target arm the source declared (see [`RawArm`]).
//! [`lower`] walks the unit once and collapses
//! each raw node to the single arm matching the build target (or its `else`
//! fallback), so the emitter only ever sees a resolved single-arm raw and never
//! performs target selection itself.
//!
//! This is deliberately **not** a general pass framework. It is one function
//! that touches only `raw` nodes; every other node passes through unchanged. It
//! is the seam a future layer system's target-aware lowering can grow into, but
//! today it does exactly one thing and no more.
//!
//! ## Matching rule
//!
//! An arm is selected when its [`target`](crate::ast::RawArm::target) equals the
//! build target name verbatim (**target-only** match). The arm's opaque
//! `version` constraint is parsed and preserved but NOT matched on yet —
//! version-range resolution is a later arc. If no arm matches and the node has
//! an `else` fallback (`default`), the fallback is used; if no arm matches and
//! there is no fallback, lowering fails with
//! [`LowerError::NoRawArmForTarget`] — raw code is never silently dropped.

use crate::ast::{
    Attr, Expr, FieldInit, File, Function, Item, Meta, RawArm, Statement, SwitchCase,
};
use crate::error::LowerError;

/// Lowers `file` for the build `target`, returning an owned, resolved copy.
///
/// Today this resolves every multi-arm `raw` node to the one arm matching
/// `target` (or the `else` fallback). Non-raw nodes are copied through
/// unchanged, so the result is structurally identical to the input except at
/// raw positions.
///
/// # Errors
///
/// Returns [`LowerError::NoRawArmForTarget`] if a `raw` node has no arm whose
/// target matches `target` and no `else` fallback.
pub fn lower(file: &File, target: &str) -> Result<File, LowerError> {
    let mut items = Vec::with_capacity(file.items.len());
    for item in &file.items {
        items.push(lower_item(item, target)?);
    }
    Ok(File { items })
}

/// Resolves a single multi-arm raw node (`arms` + optional `default`) to the
/// verbatim code of the arm selected for `target`.
///
/// Selection is target-only: the first arm whose `target` equals `target` wins;
/// otherwise the `else` fallback is used; otherwise this is a hard
/// [`LowerError::NoRawArmForTarget`].
fn resolve_raw(
    arms: &[RawArm],
    default: &Option<String>,
    target: &str,
) -> Result<String, LowerError> {
    if let Some(arm) = arms.iter().find(|a| a.target == target) {
        return Ok(arm.code.clone());
    }
    if let Some(code) = default {
        return Ok(code.clone());
    }
    let declared = arms
        .iter()
        .map(|a| a.target.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(LowerError::NoRawArmForTarget {
        target: target.to_string(),
        declared,
    })
}

/// Builds a resolved single-arm raw node from already-selected verbatim `code`,
/// preserving the original node's metadata.
///
/// The resolved arm is **canonical / detargeted**: its `target` is cleared to
/// the empty string so a resolved raw compares equal regardless of which build
/// target produced it (and matches the [`Expr::raw`](crate::ast::Expr::raw) /
/// [`Statement::raw`](crate::ast::Statement::raw) /
/// [`Item::raw`](crate::ast::Item::raw) constructors). The emitter reads only
/// `code`, so the cleared target is immaterial to emission.
fn resolved_arm(code: String, meta: Meta) -> (Vec<RawArm>, Option<String>, Meta) {
    (
        vec![RawArm {
            target: String::new(),
            version: None,
            code,
        }],
        None,
        meta,
    )
}

/// Lowers one top-level item.
fn lower_item(item: &Item, target: &str) -> Result<Item, LowerError> {
    match item {
        Item::Function(f) => Ok(Item::Function(lower_function(f, target)?)),
        Item::Const {
            name,
            ty,
            value,
            visibility,
            meta,
        } => Ok(Item::Const {
            name: name.clone(),
            ty: ty.clone(),
            value: lower_expr(value, target)?,
            visibility: *visibility,
            meta: meta.clone(),
        }),
        Item::Tree(expr) => Ok(Item::Tree(lower_expr(expr, target)?)),
        Item::Raw {
            arms,
            default,
            meta,
        } => {
            let code = resolve_raw(arms, default, target)?;
            let (arms, default, meta) = resolved_arm(code, meta.clone());
            Ok(Item::Raw {
                arms,
                default,
                meta,
            })
        }
        // Structs, enums, typedefs, and uses carry no nested expressions or
        // statements, so they lower to an unchanged clone.
        Item::Struct { .. } | Item::Enum { .. } | Item::TypeDef { .. } | Item::Use { .. } => {
            Ok(item.clone())
        }
    }
}

/// Lowers a function (its body may contain raw statements/expressions).
fn lower_function(f: &Function, target: &str) -> Result<Function, LowerError> {
    Ok(Function {
        name: f.name.clone(),
        visibility: f.visibility,
        modifiers: f.modifiers.clone(),
        params: f.params.clone(),
        return_type: f.return_type.clone(),
        body: lower_block(&f.body, target)?,
        meta: f.meta.clone(),
    })
}

/// Lowers a statement block (a `Vec<Statement>`), resolving every nested raw.
fn lower_block(body: &[Statement], target: &str) -> Result<Vec<Statement>, LowerError> {
    let mut out = Vec::with_capacity(body.len());
    for stmt in body {
        out.push(lower_stmt(stmt, target)?);
    }
    Ok(out)
}

/// Lowers one statement, recursing into every nested statement and expression.
fn lower_stmt(stmt: &Statement, target: &str) -> Result<Statement, LowerError> {
    match stmt {
        Statement::Block(body) => Ok(Statement::Block(lower_block(body, target)?)),
        Statement::Let { name, ty, value } => Ok(Statement::Let {
            name: name.clone(),
            ty: ty.clone(),
            value: lower_opt_expr(value, target)?,
        }),
        Statement::Return(value) => Ok(Statement::Return(lower_opt_expr(value, target)?)),
        Statement::If {
            cond,
            then_block,
            else_block,
        } => Ok(Statement::If {
            cond: lower_expr(cond, target)?,
            then_block: lower_block(then_block, target)?,
            else_block: match else_block {
                Some(e) => Some(Box::new(lower_stmt(e, target)?)),
                None => None,
            },
        }),
        Statement::While { cond, body } => Ok(Statement::While {
            cond: lower_expr(cond, target)?,
            body: lower_block(body, target)?,
        }),
        Statement::For {
            init,
            cond,
            step,
            body,
        } => Ok(Statement::For {
            init: match init {
                Some(s) => Some(Box::new(lower_stmt(s, target)?)),
                None => None,
            },
            cond: lower_opt_expr(cond, target)?,
            step: match step {
                Some(s) => Some(Box::new(lower_stmt(s, target)?)),
                None => None,
            },
            body: lower_block(body, target)?,
        }),
        Statement::ForEach {
            binding,
            iterable,
            body,
        } => Ok(Statement::ForEach {
            binding: binding.clone(),
            iterable: lower_expr(iterable, target)?,
            body: lower_block(body, target)?,
        }),
        Statement::Switch {
            scrutinee,
            cases,
            default,
        } => Ok(Statement::Switch {
            scrutinee: lower_expr(scrutinee, target)?,
            cases: {
                let mut out = Vec::with_capacity(cases.len());
                for case in cases {
                    out.push(lower_switch_case(case, target)?);
                }
                out
            },
            default: match default {
                Some(body) => Some(lower_block(body, target)?),
                None => None,
            },
        }),
        Statement::Break => Ok(Statement::Break),
        Statement::Continue => Ok(Statement::Continue),
        Statement::Assign { target: t, value } => Ok(Statement::Assign {
            target: lower_expr(t, target)?,
            value: lower_expr(value, target)?,
        }),
        Statement::Expr(e) => Ok(Statement::Expr(lower_expr(e, target)?)),
        Statement::Raw {
            arms,
            default,
            meta,
        } => {
            let code = resolve_raw(arms, default, target)?;
            let (arms, default, meta) = resolved_arm(code, meta.clone());
            Ok(Statement::Raw {
                arms,
                default,
                meta,
            })
        }
    }
}

/// Lowers one switch case (its matched value and body may contain raw nodes;
/// the payload binding carries no expressions).
fn lower_switch_case(case: &SwitchCase, target: &str) -> Result<SwitchCase, LowerError> {
    Ok(SwitchCase {
        value: lower_expr(&case.value, target)?,
        body: lower_block(&case.body, target)?,
        bindings: case.bindings.clone(),
        meta: case.meta.clone(),
    })
}

/// Lowers an optional expression, preserving `None`.
fn lower_opt_expr(expr: &Option<Expr>, target: &str) -> Result<Option<Expr>, LowerError> {
    match expr {
        Some(e) => Ok(Some(lower_expr(e, target)?)),
        None => Ok(None),
    }
}

/// Lowers one expression, recursing into every nested expression position.
fn lower_expr(expr: &Expr, target: &str) -> Result<Expr, LowerError> {
    match expr {
        // Leaf literals / references carry no nested expressions.
        Expr::IntLiteral(_)
        | Expr::FloatLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::CharLiteral(_)
        | Expr::NullLiteral
        | Expr::Ref(_) => Ok(expr.clone()),
        Expr::Field { obj, field } => Ok(Expr::Field {
            obj: Box::new(lower_expr(obj, target)?),
            field: field.clone(),
        }),
        Expr::Index { obj, index } => Ok(Expr::Index {
            obj: Box::new(lower_expr(obj, target)?),
            index: Box::new(lower_expr(index, target)?),
        }),
        Expr::Call { callee, args } => Ok(Expr::Call {
            callee: Box::new(lower_expr(callee, target)?),
            args: lower_exprs(args, target)?,
        }),
        Expr::Unary { op, operand } => Ok(Expr::Unary {
            op: *op,
            operand: Box::new(lower_expr(operand, target)?),
        }),
        Expr::Binary { op, lhs, rhs } => Ok(Expr::Binary {
            op: *op,
            lhs: Box::new(lower_expr(lhs, target)?),
            rhs: Box::new(lower_expr(rhs, target)?),
        }),
        Expr::Cast { value, ty } => Ok(Expr::Cast {
            value: Box::new(lower_expr(value, target)?),
            ty: ty.clone(),
        }),
        Expr::StructLit {
            type_name,
            fields,
            meta,
        } => Ok(Expr::StructLit {
            type_name: type_name.clone(),
            fields: lower_field_inits(fields, target)?,
            meta: meta.clone(),
        }),
        Expr::ArrayLit { elems, meta } => Ok(Expr::ArrayLit {
            elems: lower_exprs(elems, target)?,
            meta: meta.clone(),
        }),
        Expr::Node {
            name,
            attrs,
            children,
            meta,
        } => Ok(Expr::Node {
            name: name.clone(),
            attrs: lower_attrs(attrs, target)?,
            children: lower_exprs(children, target)?,
            meta: meta.clone(),
        }),
        Expr::Text(inner) => Ok(Expr::Text(Box::new(lower_expr(inner, target)?))),
        Expr::Raw {
            arms,
            default,
            meta,
        } => {
            let code = resolve_raw(arms, default, target)?;
            let (arms, default, meta) = resolved_arm(code, meta.clone());
            Ok(Expr::Raw {
                arms,
                default,
                meta,
            })
        }
        Expr::Lambda {
            params,
            return_type,
            body,
            meta,
        } => Ok(Expr::Lambda {
            params: params.clone(),
            return_type: return_type.clone(),
            body: lower_block(body, target)?,
            meta: meta.clone(),
        }),
    }
}

/// Lowers a sequence of expressions.
fn lower_exprs(exprs: &[Expr], target: &str) -> Result<Vec<Expr>, LowerError> {
    let mut out = Vec::with_capacity(exprs.len());
    for e in exprs {
        out.push(lower_expr(e, target)?);
    }
    Ok(out)
}

/// Lowers a sequence of struct-literal field initializers.
fn lower_field_inits(fields: &[FieldInit], target: &str) -> Result<Vec<FieldInit>, LowerError> {
    let mut out = Vec::with_capacity(fields.len());
    for f in fields {
        out.push(FieldInit {
            name: f.name.clone(),
            value: lower_expr(&f.value, target)?,
            meta: f.meta.clone(),
        });
    }
    Ok(out)
}

/// Lowers a sequence of tree-node attributes (each value is an expression).
fn lower_attrs(attrs: &[Attr], target: &str) -> Result<Vec<Attr>, LowerError> {
    let mut out = Vec::with_capacity(attrs.len());
    for a in attrs {
        out.push(Attr {
            name: a.name.clone(),
            value: lower_expr(&a.value, target)?,
            meta: a.meta.clone(),
        });
    }
    Ok(out)
}
