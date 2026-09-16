//! The emitter: renders a [`File`] AST to target source via a [`LanguageDef`]'s
//! `When`-table templates, using the recursive [`crate::render`] machinery.
//!
//! The emitter contains no target-specific knowledge. It builds a
//! [`RenderContext`] of facts from each item, renders the target's entry
//! template, and fills its slots — recursively resolving slot definitions
//! (`ret`, `vis`, `async`, …) against the same context. A slot that resolves to
//! the `forbid` directive is a hard error, so a construct the target cannot
//! express fails loudly rather than emitting garbage.

use crate::ast::{
    slot_binding, BinaryOp, Expr, ExprKind, Field, File, Function, Item, ItemKind, Modifier, Param,
    Primitive, SlotScope, SlotShape, Statement, StatementKind, SwitchCase, Type, UnaryOp, Variant,
    Visibility,
};
use crate::error::EmitError;
use crate::lang::{ItemDef, LanguageDef, OperatorSpelling, Outcome, SlotDef};
use crate::predicate::{
    CallerKind, ExprKind as PredExprKind, ItemKind as PredItemKind, RenderContext, RetKind,
    StmtKind as PredStmtKind, VisKind,
};
use crate::render::{Rendered, SlotResolver};

/// Transpiles `file` to the target described by `lang`.
///
/// # Errors
///
/// Returns an [`EmitError`] if the program uses a primitive or construct the
/// target forbids, or if the language definition is internally malformed (e.g.
/// a `When` table with no matching row, or a template referencing an unknown
/// slot).
pub fn emit(file: &File, lang: &LanguageDef) -> Result<String, EmitError> {
    let mut out = String::new();
    for (index, item) in file.items.iter().enumerate() {
        if index > 0 {
            out.push_str("\n\n");
        }
        let rendered = emit_item(item, lang)?;
        out.push_str(&rendered.text);
    }
    Ok(out)
}

/// Renders a single top-level item, dispatching on its [`ItemKind`].
///
/// A [`Item::Function`] renders via the target's `## Function` section
/// ([`LanguageDef::function`]); every other item kind renders via its own
/// `## <Item>` section ([`LanguageDef::item_def`]). A definition that omits the
/// section for an item kind cannot express that item — using it is an
/// [`EmitError::UnknownItem`].
fn emit_item(item: &Item, lang: &LanguageDef) -> Result<Rendered, EmitError> {
    match item {
        Item::Function(function) => emit_function(function, lang),
        _ => {
            let kind = item.kind();
            let def = lang.item_def(kind).ok_or_else(|| EmitError::UnknownItem {
                target: lang.name.clone(),
                item: kind.as_str().to_string(),
            })?;
            let mut resolver = ItemResolver {
                item,
                def,
                lang,
                ctx: item_context(item),
                scope: ItemScope::Node,
            };
            def.entry.render(&mut resolver)
        }
    }
}

/// Renders a single function via the target's entry template.
fn emit_function(function: &Function, lang: &LanguageDef) -> Result<Rendered, EmitError> {
    let ctx = context_for(function);
    let mut resolver = FunctionResolver {
        function,
        lang,
        ctx,
        scope: Scope::Function,
    };
    lang.function.entry.render(&mut resolver)
}

/// Builds the base fact context the `When` predicates read for a function.
fn context_for(function: &Function) -> RenderContext {
    let ret = match &function.return_type {
        // The `void`/`never` primitives drive the `ret is void|never` facts;
        // every other type (a value primitive, a named type, or a compound
        // type) is an ordinary value return.
        Type::Primitive(Primitive::Void) => RetKind::Void,
        Type::Primitive(Primitive::Never) => RetKind::Never,
        Type::Primitive(_) | Type::Named(_) | Type::Pointer(_) | Type::FnPtr { .. } => {
            RetKind::Type
        }
    };
    RenderContext {
        export: function.visibility == Visibility::Public,
        is_async: function.has_modifier(Modifier::Async),
        is_const: function.has_modifier(Modifier::Const),
        is_unsafe: function.has_modifier(Modifier::Unsafe),
        is_throws: function.has_modifier(Modifier::Throws),
        is_extern: function.has_modifier(Modifier::Extern),
        is_inline: function.has_modifier(Modifier::Inline),
        is_generator: function.has_modifier(Modifier::Generator),
        ret: Some(ret),
        vis: Some(match function.visibility {
            Visibility::Public => VisKind::Public,
            Visibility::Protected => VisKind::Protected,
            Visibility::Private => VisKind::Private,
        }),
        // A function's own body statements inherit its synchrony as `caller`.
        caller: Some(if function.has_modifier(Modifier::Async) {
            CallerKind::Async
        } else {
            CallerKind::Sync
        }),
        ..Default::default()
    }
}

/// Maps an AST [`ItemKind`] to the predicate registry's `ItemKind`, so the
/// engine can set the `item is <kind>` fact when rendering a top-level item.
fn pred_item_kind(kind: ItemKind) -> PredItemKind {
    match kind {
        ItemKind::Function => PredItemKind::Function,
        ItemKind::Struct => PredItemKind::Struct,
        ItemKind::Enum => PredItemKind::Enum,
        ItemKind::TypeDef => PredItemKind::TypeDef,
        ItemKind::Const => PredItemKind::Const,
        ItemKind::Use => PredItemKind::Use,
    }
}

/// Maps an AST [`Visibility`] to the predicate registry's `VisKind`.
fn vis_kind(visibility: Visibility) -> VisKind {
    match visibility {
        Visibility::Public => VisKind::Public,
        Visibility::Protected => VisKind::Protected,
        Visibility::Private => VisKind::Private,
    }
}

/// Builds the fact context for rendering a top-level item: the `item is <kind>`
/// dispatch fact, plus the `export`/`vis` facts for items that carry a
/// visibility (`struct`, `enum`, `const`). A `typedef` and a `use` carry no
/// visibility, so those facts stay at their defaults.
fn item_context(item: &Item) -> RenderContext {
    let mut ctx = RenderContext {
        item: Some(pred_item_kind(item.kind())),
        ..Default::default()
    };
    if let Some(visibility) = item_visibility(item) {
        ctx.export = visibility == Visibility::Public;
        ctx.vis = Some(vis_kind(visibility));
    }
    ctx
}

/// The visibility of an item that carries one, or `None` for items that do not
/// (`typedef`, `use`).
fn item_visibility(item: &Item) -> Option<Visibility> {
    match item {
        Item::Function(f) => Some(f.visibility),
        Item::Struct { visibility, .. }
        | Item::Enum { visibility, .. }
        | Item::Const { visibility, .. } => Some(*visibility),
        Item::TypeDef { .. } | Item::Use { .. } => None,
    }
}

/// What an [`ItemResolver`] is currently rendering: the item node itself, or one
/// element of a collection it owns (a struct field or an enum variant).
enum ItemScope<'a> {
    /// Rendering the item declaration itself.
    Node,
    /// Rendering one struct-field element.
    Field(&'a Field),
    /// Rendering one enum-variant element.
    Variant(&'a Variant),
}

/// Resolves the slots of a non-function top-level item (its `## <Item>` entry
/// template and `### <slot>` subsections), recursing into nested types and
/// expressions and looping its field/variant collections. Mirrors
/// [`FunctionResolver`] but scoped to item rendering.
struct ItemResolver<'a> {
    item: &'a Item,
    def: &'a ItemDef,
    lang: &'a LanguageDef,
    ctx: RenderContext,
    scope: ItemScope<'a>,
}

impl<'a> ItemResolver<'a> {
    /// The [`SlotScope`] for querying [`slot_binding`] in the current scope.
    fn scope_kind(&self) -> SlotScope {
        match self.scope {
            ItemScope::Node => self.item.kind().scope(),
            ItemScope::Field(_) => SlotScope::Field,
            ItemScope::Variant(_) => SlotScope::Variant,
        }
    }

    /// Renders a named slot definition (fixed outcome or table) from this
    /// item's own `## <Item>` slot map, against the current context.
    fn render_named_slot(&mut self, name: &str) -> Result<Rendered, EmitError> {
        let slot = self
            .def
            .slots
            .get(name)
            .cloned()
            .ok_or_else(|| EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            })?;

        let outcome = match &slot {
            SlotDef::Fixed(outcome) => outcome.clone(),
            SlotDef::Table(table) => table
                .select(&self.ctx)
                .ok_or_else(|| EmitError::NoMatchingRow {
                    target: self.lang.name.clone(),
                    table: name.to_string(),
                })?
                .clone(),
        };

        match outcome {
            Outcome::Render(template) => template.render(self),
            Outcome::Forbid => Err(EmitError::ForbiddenConstruct {
                target: self.lang.name.clone(),
            }),
        }
    }

    /// Produces the text of a scalar engine-bound item sub-slot, resolved
    /// against the current scope/variant. Nested types resolve through
    /// [`resolve_type`] and nested expressions through [`emit_expr`], reusing
    /// the shared `## Function`-hosted helper slots.
    fn scalar(&self, name: &str) -> Result<Rendered, EmitError> {
        match &self.scope {
            ItemScope::Field(field) => match name {
                "name" => Ok(Rendered::text(field.name.clone())),
                "type" => resolve_type(&field.ty, self.lang),
                _ => self.unknown_slot(name),
            },
            ItemScope::Variant(variant) => match name {
                "name" => Ok(Rendered::text(variant.name.clone())),
                _ => self.unknown_slot(name),
            },
            ItemScope::Node => self.scalar_node(name),
        }
    }

    /// Scalar sub-slots of the item node itself, dispatched by variant.
    fn scalar_node(&self, name: &str) -> Result<Rendered, EmitError> {
        match (self.item, name) {
            (Item::Struct { name: n, .. }, "name")
            | (Item::Enum { name: n, .. }, "name")
            | (Item::TypeDef { name: n, .. }, "name")
            | (Item::Const { name: n, .. }, "name") => Ok(Rendered::text(n.clone())),
            (Item::TypeDef { target, .. }, "target") => resolve_type(target, self.lang),
            (Item::Const { ty, .. }, "type") => resolve_type(ty, self.lang),
            (Item::Const { value, .. }, "value") => emit_expr(value, self.lang),
            (Item::Use { path, .. }, "path") => Ok(Rendered::text(path.clone())),
            _ => self.unknown_slot(name),
        }
    }

    /// Loops a field/variant collection sub-slot (`fields`/`variants`),
    /// rendering each element via its item slot with `first`/`last` loop facts.
    fn sequence(&self, name: &str, item_slot: &str) -> Result<Rendered, EmitError> {
        match (self.item, name) {
            (Item::Struct { fields, .. }, "fields") => {
                self.render_fields(fields, item_slot)
            }
            (Item::Enum { variants, .. }, "variants") => {
                self.render_variants(variants, item_slot)
            }
            _ => self.unknown_slot(name),
        }
    }

    /// Loops a struct's fields, rendering the `field` item slot per field with
    /// `first`/`last` loop facts and each field's own `vis`/`export` facts.
    fn render_fields(&self, fields: &[Field], item_slot: &str) -> Result<Rendered, EmitError> {
        if item_slot != "field" {
            return Err(EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: item_slot.to_string(),
            });
        }
        let len = fields.len();
        let mut out = Rendered::empty();
        for (i, field) in fields.iter().enumerate() {
            let ctx = RenderContext {
                item: self.ctx.item,
                export: field.visibility == Visibility::Public,
                vis: Some(vis_kind(field.visibility)),
                first: i == 0,
                last: i + 1 == len,
                ..Default::default()
            };
            let mut elem = ItemResolver {
                item: self.item,
                def: self.def,
                lang: self.lang,
                ctx,
                scope: ItemScope::Field(field),
            };
            out.push(elem.render_named_slot(item_slot)?);
        }
        Ok(out)
    }

    /// Loops an enum's variants, rendering the `variant` item slot per variant
    /// with `first`/`last` loop facts.
    fn render_variants(
        &self,
        variants: &[Variant],
        item_slot: &str,
    ) -> Result<Rendered, EmitError> {
        if item_slot != "variant" {
            return Err(EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: item_slot.to_string(),
            });
        }
        let len = variants.len();
        let mut out = Rendered::empty();
        for (i, variant) in variants.iter().enumerate() {
            let ctx = RenderContext {
                item: self.ctx.item,
                first: i == 0,
                last: i + 1 == len,
                ..Default::default()
            };
            let mut elem = ItemResolver {
                item: self.item,
                def: self.def,
                lang: self.lang,
                ctx,
                scope: ItemScope::Variant(variant),
            };
            out.push(elem.render_named_slot(item_slot)?);
        }
        Ok(out)
    }

    /// Builds an `UnknownSlot` error for the current target/slot.
    fn unknown_slot(&self, name: &str) -> Result<Rendered, EmitError> {
        Err(EmitError::UnknownSlot {
            target: self.lang.name.clone(),
            slot: name.to_string(),
        })
    }
}

impl<'a> SlotResolver for ItemResolver<'a> {
    type Error = EmitError;

    fn resolve(&mut self, name: &str) -> Result<Rendered, EmitError> {
        match slot_binding(name, self.scope_kind()) {
            Some(SlotShape::Scalar) => self.scalar(name),
            Some(SlotShape::Sequence { item_slot, .. }) => self.sequence(name, &item_slot),
            None => self.render_named_slot(name),
        }
    }
}

/// What the resolver is currently rendering: the function as a whole, or one
/// element of an iterable collection (a param or a statement). The scope
/// redirects leaf slots like `{name}` / `{type}` / `{value}` to the element.
enum Scope<'a> {
    /// Rendering the function declaration itself.
    Function,
    /// Rendering one parameter element.
    Param(&'a Param),
}

/// Resolves function template slots, recursing into slot definitions and
/// looping iterable terminal slots (`params`, `body`).
struct FunctionResolver<'a> {
    function: &'a Function,
    lang: &'a LanguageDef,
    ctx: RenderContext,
    scope: Scope<'a>,
}

impl<'a> FunctionResolver<'a> {
    /// Renders a named slot definition (fixed outcome or table) against the
    /// current context. Used for the per-element item slots (`param`,
    /// `statement`) and ordinary slots (`ret`, `vis`, …).
    fn render_named_slot(&mut self, name: &str) -> Result<Rendered, EmitError> {
        let slot = self
            .lang
            .function
            .slots
            .get(name)
            .cloned()
            .ok_or_else(|| EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            })?;

        let outcome = match &slot {
            SlotDef::Fixed(outcome) => outcome.clone(),
            SlotDef::Table(table) => table
                .select(&self.ctx)
                .ok_or_else(|| EmitError::NoMatchingRow {
                    target: self.lang.name.clone(),
                    table: name.to_string(),
                })?
                .clone(),
        };

        match outcome {
            Outcome::Render(template) => template.render(self),
            Outcome::Forbid => Err(EmitError::ForbiddenConstruct {
                target: self.lang.name.clone(),
            }),
        }
    }

    /// Loops an iterable collection, rendering `item_slot` per element with
    /// `first`/`last` loop facts set, and concatenating (no join — each item
    /// renders its own separators via `When` predicates on the loop facts).
    fn render_collection(
        &mut self,
        len: usize,
        item_slot: &str,
        mut make_scope: impl FnMut(usize) -> Scope<'a>,
    ) -> Result<Rendered, EmitError> {
        let mut out = Rendered::empty();
        for i in 0..len {
            // Build a per-element context: the function base facts plus loop
            // facts for this position.
            let mut elem_ctx = self.ctx.clone();
            elem_ctx.first = i == 0;
            elem_ctx.last = i + 1 == len;

            let mut elem_resolver = FunctionResolver {
                function: self.function,
                lang: self.lang,
                ctx: elem_ctx,
                scope: make_scope(i),
            };
            out.push(elem_resolver.render_named_slot(item_slot)?);
        }
        Ok(out)
    }

    /// The scope kind for querying [`slot_binding`].
    fn scope_kind(&self) -> SlotScope {
        match self.scope {
            Scope::Function => SlotScope::Function,
            Scope::Param(_) => SlotScope::Param,
        }
    }

    /// Produces the text of a scalar engine-bound slot, resolved against the
    /// current scope. Only called for names the binding table declares
    /// `Scalar`, so the scope/name combinations here are exhaustive for those.
    fn scalar(&self, name: &str) -> Result<Rendered, EmitError> {
        match (&self.scope, name) {
            (Scope::Param(p), "name") => Ok(Rendered::text(p.name.clone())),
            (Scope::Param(p), "type") => resolve_type(&p.ty, self.lang),
            (_, "name") => Ok(Rendered::text(self.function.name.clone())),
            (_, "ret_type") => resolve_type(&self.function.return_type, self.lang),
            _ => Err(EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            }),
        }
    }
}

impl<'a> SlotResolver for FunctionResolver<'a> {
    type Error = EmitError;

    fn resolve(&mut self, name: &str) -> Result<Rendered, EmitError> {
        // Cardinality is data-driven: ask the binding table for this slot's
        // shape in the current scope. Scalar -> render directly; Sequence ->
        // loop the item slot; not engine-bound -> a named slot definition.
        match slot_binding(name, self.scope_kind()) {
            Some(SlotShape::Scalar) => self.scalar(name),
            Some(SlotShape::Sequence { item_slot, .. }) => {
                // Loop the matching AST sequence for this bound name.
                match (self.scope_kind(), name) {
                    (SlotScope::Function, "params") => {
                        let params: &'a [Param] = &self.function.params;
                        let len = params.len();
                        self.render_collection(len, &item_slot, |i| Scope::Param(&params[i]))
                    }
                    (SlotScope::Function, "body") => {
                        // The function body is a statement sequence: render each
                        // element through the dedicated statement machinery,
                        // threading the function's `caller` synchrony and the
                        // `first`/`last` loop facts down so the `### statement`
                        // item slot can supply its own separator.
                        render_statement_sequence(
                            &self.function.body,
                            &item_slot,
                            self.lang,
                            self.ctx.caller,
                        )
                    }
                    _ => Err(EmitError::UnknownSlot {
                        target: self.lang.name.clone(),
                        slot: name.to_string(),
                    }),
                }
            }
            None => self.render_named_slot(name),
        }
    }
}

/// Maps an AST [`StatementKind`] to the predicate registry's `StmtKind`, so the
/// engine can set the `stmt is <kind>` fact when rendering a statement.
fn pred_stmt_kind(kind: StatementKind) -> PredStmtKind {
    match kind {
        StatementKind::Block => PredStmtKind::Block,
        StatementKind::Let => PredStmtKind::Let,
        StatementKind::Return => PredStmtKind::Return,
        StatementKind::If => PredStmtKind::If,
        StatementKind::While => PredStmtKind::While,
        StatementKind::For => PredStmtKind::For,
        StatementKind::ForEach => PredStmtKind::ForEach,
        StatementKind::Switch => PredStmtKind::Switch,
        StatementKind::Break => PredStmtKind::Break,
        StatementKind::Continue => PredStmtKind::Continue,
        StatementKind::Expr => PredStmtKind::Expr,
    }
}

/// Builds the fact context for rendering `stmt`: the `stmt is <kind>` dispatch
/// fact, the `has value`/`has else`/… optional-part flags, the inherited
/// `caller` synchrony, and the `first`/`last` loop facts for this position in
/// its sequence.
fn statement_context(
    stmt: &Statement,
    caller: Option<CallerKind>,
    first: bool,
    last: bool,
) -> RenderContext {
    let mut ctx = RenderContext {
        stmt: Some(pred_stmt_kind(stmt.kind())),
        caller,
        first,
        last,
        ..Default::default()
    };
    match stmt {
        Statement::Let { ty, value, .. } => {
            ctx.has_type = ty.is_some();
            ctx.has_value = value.is_some();
        }
        Statement::Return(value) => ctx.has_value = value.is_some(),
        Statement::If { else_block, .. } => ctx.has_else = else_block.is_some(),
        Statement::For {
            init, cond, step, ..
        } => {
            ctx.has_init = init.is_some();
            ctx.has_cond = cond.is_some();
            ctx.has_step = step.is_some();
        }
        Statement::Switch { default, .. } => ctx.has_default = default.is_some(),
        _ => {}
    }
    ctx
}

/// Renders one statement via the target's `### statement` `When`-table (which
/// dispatches on `stmt is <kind>`), filling that row's scoped sub-slots.
///
/// `caller` threads the enclosing callable's synchrony down (for `caller is
/// async` predicates); `first`/`last` are this statement's loop facts within
/// its sequence, so the item template can render its own separator.
fn emit_statement(
    stmt: &Statement,
    lang: &LanguageDef,
    caller: Option<CallerKind>,
    first: bool,
    last: bool,
) -> Result<Rendered, EmitError> {
    let ctx = statement_context(stmt, caller, first, last);
    let mut resolver = StmtResolver {
        stmt,
        lang,
        caller,
        ctx,
        scope: StmtScope::Node,
    };
    resolver.render_named_slot("statement")
}

/// Renders one statement in *clause* (terminator-free) form via the target's
/// `### stmt_clause` `When`-table, for use inside a C-style `for` header. Shares
/// the same fact context as [`emit_statement`] (so `stmt is <kind>` and the
/// `has …` flags select the right row), but dispatches to `stmt_clause` instead
/// of `statement`. The definition's `### stmt_clause` rows omit the statement
/// terminator; the engine appends nothing of its own.
fn emit_statement_clause(
    stmt: &Statement,
    lang: &LanguageDef,
    caller: Option<CallerKind>,
) -> Result<Rendered, EmitError> {
    let ctx = statement_context(stmt, caller, true, true);
    let mut resolver = StmtResolver {
        stmt,
        lang,
        caller,
        ctx,
        scope: StmtScope::Node,
    };
    resolver.render_named_slot("stmt_clause")
}

/// Renders a statement sequence (`then`/`body`/`default`, or a function body),
/// looping the `item_slot` (`statement`) per element with `first`/`last` loop
/// facts. Each element renders its own separator via those facts — there is no
/// engine-supplied join.
fn render_statement_sequence(
    statements: &[Statement],
    item_slot: &str,
    lang: &LanguageDef,
    caller: Option<CallerKind>,
) -> Result<Rendered, EmitError> {
    // The item slot for a statement sequence is always the recursive
    // `statement` slot; a definition cannot repoint it (cardinality is fixed by
    // `slot_binding`). Guard defensively so a mismatch is a clear error.
    if item_slot != "statement" {
        return Err(EmitError::UnknownSlot {
            target: lang.name.clone(),
            slot: item_slot.to_string(),
        });
    }
    let len = statements.len();
    let mut out = Rendered::empty();
    for (i, stmt) in statements.iter().enumerate() {
        out.push(emit_statement(stmt, lang, caller, i == 0, i + 1 == len)?);
    }
    Ok(out)
}

/// What a [`StmtResolver`] is currently rendering: the statement node itself, or
/// one element of its `switch`'s case list.
enum StmtScope<'a> {
    /// Rendering the statement node itself.
    Node,
    /// Rendering one switch-case element.
    Case(&'a SwitchCase),
}

/// Resolves the slots of a statement (the `### statement` table and its
/// sub-slots), recursing into nested expressions and statement sequences.
/// Mirrors [`ExprResolver`] / [`FunctionResolver`] but scoped to statements.
struct StmtResolver<'a> {
    stmt: &'a Statement,
    lang: &'a LanguageDef,
    /// The inherited enclosing-callable synchrony, threaded into nested
    /// statement sequences.
    caller: Option<CallerKind>,
    /// The context for this statement (dispatch fact + optional-part flags +
    /// loop facts), used to select `When` rows for the statement's own slots.
    ctx: RenderContext,
    scope: StmtScope<'a>,
}

impl<'a> StmtResolver<'a> {
    /// The [`SlotScope`] for querying [`slot_binding`] in the current scope.
    fn scope_kind(&self) -> SlotScope {
        match self.scope {
            StmtScope::Node => SlotScope::Statement,
            StmtScope::Case(_) => SlotScope::SwitchCase,
        }
    }

    /// Renders a named slot definition (the `statement` table, the recursive
    /// `stmt` helper, `switch_case`, or any def-defined helper) against the
    /// current statement context.
    fn render_named_slot(&mut self, name: &str) -> Result<Rendered, EmitError> {
        let slot = self
            .lang
            .function
            .slots
            .get(name)
            .cloned()
            .ok_or_else(|| EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            })?;

        let outcome = match &slot {
            SlotDef::Fixed(outcome) => outcome.clone(),
            SlotDef::Table(table) => table
                .select(&self.ctx)
                .ok_or_else(|| EmitError::NoMatchingRow {
                    target: self.lang.name.clone(),
                    table: name.to_string(),
                })?
                .clone(),
        };

        match outcome {
            Outcome::Render(template) => template.render(self),
            Outcome::Forbid => Err(EmitError::ForbiddenConstruct {
                target: self.lang.name.clone(),
            }),
        }
    }

    /// Renders a nested single statement (`else`, `init`, `step`) through the
    /// full `### statement` dispatch. A nested statement is not part of a loop,
    /// so its `first`/`last` facts are both `true` (it is the sole element),
    /// mirroring how a one-statement sequence would render.
    fn render_nested_statement(&self, stmt: &Statement) -> Result<Rendered, EmitError> {
        emit_statement(stmt, self.lang, self.caller, true, true)
    }

    /// Renders a `for` init/step in *clause* (terminator-free) form through the
    /// target's `### stmt_clause` dispatch. Used for a C-style `for
    /// (init; cond; step)` header, where the header supplies the `;` separators
    /// and the clauses must not carry their own statement terminator. A target
    /// that has no `### stmt_clause` (because it desugars the counted loop
    /// instead of emitting a C-style header) simply never references
    /// `{init_clause}` / `{step_clause}`, so the slot is looked up lazily.
    fn render_statement_clause(&self, stmt: &Statement) -> Result<Rendered, EmitError> {
        emit_statement_clause(stmt, self.lang, self.caller)
    }

    /// Produces the text of a scalar engine-bound statement sub-slot, resolved
    /// against the current statement variant.
    fn scalar(&self, name: &str) -> Result<Rendered, EmitError> {
        match &self.scope {
            StmtScope::Case(case) => match name {
                "value" => emit_expr(&case.value, self.lang),
                _ => self.unknown_slot(name),
            },
            StmtScope::Node => self.scalar_node(name),
        }
    }

    /// Scalar sub-slots of the statement node itself, dispatched by variant.
    fn scalar_node(&self, name: &str) -> Result<Rendered, EmitError> {
        match (self.stmt, name) {
            // Names / bindings.
            (Statement::Let { name: n, .. }, "name") => Ok(Rendered::text(n.clone())),
            (Statement::ForEach { binding, .. }, "binding") => Ok(Rendered::text(binding.clone())),
            // A `let`'s optional type annotation. Guarded by `has type` in the
            // def, so `None` here means the def referenced `{let_type}` in a
            // branch that should not have been selected.
            (Statement::Let { ty: Some(ty), .. }, "let_type") => resolve_type(ty, self.lang),
            // Nested single expressions.
            (Statement::Let { value: Some(v), .. }, "value") => emit_expr(v, self.lang),
            (Statement::Return(Some(v)), "value") => emit_expr(v, self.lang),
            (Statement::Expr(v), "value") => emit_expr(v, self.lang),
            (Statement::If { cond, .. }, "cond") => emit_expr(cond, self.lang),
            (Statement::While { cond, .. }, "cond") => emit_expr(cond, self.lang),
            (Statement::For { cond: Some(c), .. }, "cond") => emit_expr(c, self.lang),
            (Statement::ForEach { iterable, .. }, "iterable") => emit_expr(iterable, self.lang),
            (Statement::Switch { scrutinee, .. }, "scrutinee") => emit_expr(scrutinee, self.lang),
            // Nested single statements.
            (Statement::If { else_block: Some(e), .. }, "else") => self.render_nested_statement(e),
            (Statement::For { init: Some(i), .. }, "init") => self.render_nested_statement(i),
            (Statement::For { step: Some(s), .. }, "step") => self.render_nested_statement(s),
            // Clause (terminator-free) forms for a C-style `for` header, routed
            // through the target's `### stmt_clause` dispatch. The engine
            // appends nothing; the definition owns the (terminator-free)
            // spelling.
            (Statement::For { init: Some(i), .. }, "init_clause") => {
                self.render_statement_clause(i)
            }
            (Statement::For { step: Some(s), .. }, "step_clause") => {
                self.render_statement_clause(s)
            }
            _ => self.unknown_slot(name),
        }
    }

    /// Loops a statement-sequence sub-slot (`then`/`body`/`default`) or the
    /// `cases` sub-slot, rendering each element via its item slot with loop
    /// facts.
    fn sequence(&self, name: &str, item_slot: &str) -> Result<Rendered, EmitError> {
        // In a switch-case element scope, `body` loops the *case's* statements.
        if let StmtScope::Case(case) = &self.scope {
            if name == "body" {
                return render_statement_sequence(&case.body, item_slot, self.lang, self.caller);
            }
            return self.unknown_slot(name);
        }
        // `cases` loops the `switch_case` item slot; every other sequence slot
        // loops the recursive `statement` slot.
        if let (Statement::Switch { cases, .. }, "cases") = (self.stmt, name) {
            return self.render_cases(cases, item_slot);
        }
        let statements: &[Statement] = match (self.stmt, name) {
            (Statement::Block(body), "body") => body,
            (Statement::If { then_block, .. }, "then") => then_block,
            (Statement::While { body, .. }, "body") => body,
            (Statement::For { body, .. }, "body") => body,
            (Statement::ForEach { body, .. }, "body") => body,
            (Statement::Switch { default: Some(d), .. }, "default") => d,
            _ => return self.unknown_slot(name),
        };
        render_statement_sequence(statements, item_slot, self.lang, self.caller)
    }

    /// Loops a `switch`'s cases, rendering the `switch_case` item slot per case
    /// with `first`/`last` loop facts.
    fn render_cases(&self, cases: &[SwitchCase], item_slot: &str) -> Result<Rendered, EmitError> {
        if item_slot != "switch_case" {
            return Err(EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: item_slot.to_string(),
            });
        }
        let len = cases.len();
        let mut out = Rendered::empty();
        for (i, case) in cases.iter().enumerate() {
            let ctx = RenderContext {
                caller: self.caller,
                first: i == 0,
                last: i + 1 == len,
                ..Default::default()
            };
            let mut elem_resolver = StmtResolver {
                stmt: self.stmt,
                lang: self.lang,
                caller: self.caller,
                ctx,
                scope: StmtScope::Case(case),
            };
            out.push(elem_resolver.render_named_slot(item_slot)?);
        }
        Ok(out)
    }

    /// Builds an `UnknownSlot` error for the current target/slot.
    fn unknown_slot(&self, name: &str) -> Result<Rendered, EmitError> {
        Err(EmitError::UnknownSlot {
            target: self.lang.name.clone(),
            slot: name.to_string(),
        })
    }
}

impl<'a> SlotResolver for StmtResolver<'a> {
    type Error = EmitError;

    fn resolve(&mut self, name: &str) -> Result<Rendered, EmitError> {
        match slot_binding(name, self.scope_kind()) {
            Some(SlotShape::Scalar) => self.scalar(name),
            Some(SlotShape::Sequence { item_slot, .. }) => self.sequence(name, &item_slot),
            None => self.render_named_slot(name),
        }
    }
}

/// Renders an expression by dispatching to the language definition's `### expr`
/// `When`-table on the expression's [`ExprKind`] (the `expr is <kind>` fact),
/// then filling that row's sub-slots (`op`, `lhs`/`rhs`/`operand`/`obj`/`index`/
/// `callee`, `args`, `field`, `value`).
///
/// A `null` literal is gated by the `ptr` primitive: on a target that forbids
/// `ptr`, `null` is forbidden too (`null` follows `ptr`).
fn emit_expr(expr: &Expr, lang: &LanguageDef) -> Result<Rendered, EmitError> {
    // `null` follows `ptr`: if the target forbids the `ptr` primitive, a `null`
    // literal is not expressible either.
    if matches!(expr, Expr::NullLiteral) {
        gate_primitive(Primitive::Ptr, lang).map_err(|_| EmitError::ForbiddenOperator {
            target: lang.name.clone(),
            operator: "null".to_string(),
        })?;
    }
    let mut resolver = ExprResolver {
        expr,
        lang,
        scope: ExprScope::Node,
    };
    resolver.render_named_slot("expr")
}

/// Resolves the operator spelling for a unary operator, honoring any per-def
/// override or `forbid`.
fn unary_op_spelling(op: UnaryOp, lang: &LanguageDef) -> Result<String, EmitError> {
    match lang.operator(op.name()) {
        Some(OperatorSpelling::Spell(text)) => Ok(text.clone()),
        Some(OperatorSpelling::Forbid) => Err(EmitError::ForbiddenOperator {
            target: lang.name.clone(),
            operator: op.as_str().to_string(),
        }),
        None => Ok(op.as_str().to_string()),
    }
}

/// Resolves the operator spelling for a binary operator, honoring any per-def
/// override or `forbid`.
fn binary_op_spelling(op: BinaryOp, lang: &LanguageDef) -> Result<String, EmitError> {
    match lang.operator(op.name()) {
        Some(OperatorSpelling::Spell(text)) => Ok(text.clone()),
        Some(OperatorSpelling::Forbid) => Err(EmitError::ForbiddenOperator {
            target: lang.name.clone(),
            operator: op.as_str().to_string(),
        }),
        None => Ok(op.as_str().to_string()),
    }
}

/// Maps an AST [`ExprKind`] to the predicate registry's `ExprKind`, so the
/// engine can set the `expr is <kind>` fact when rendering an expression.
fn pred_expr_kind(kind: ExprKind) -> PredExprKind {
    match kind {
        ExprKind::Int => PredExprKind::Int,
        ExprKind::Float => PredExprKind::Float,
        ExprKind::Bool => PredExprKind::Bool,
        ExprKind::String => PredExprKind::String,
        ExprKind::Char => PredExprKind::Char,
        ExprKind::Null => PredExprKind::Null,
        ExprKind::Ref => PredExprKind::Ref,
        ExprKind::Field => PredExprKind::Field,
        ExprKind::Index => PredExprKind::Index,
        ExprKind::Call => PredExprKind::Call,
        ExprKind::Unary => PredExprKind::Unary,
        ExprKind::Binary => PredExprKind::Binary,
    }
}

/// The textual leaf value of a literal expression, or `None` for a non-literal.
///
/// Literals are preserved textually (their width/precision/escaping is a target
/// concern). The `value` sub-slot in a `### expr` row exposes this text so the
/// language definition can apply target quoting/escaping (e.g. wrapping a
/// string literal's contents in quotes).
fn literal_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::IntLiteral(v) | Expr::FloatLiteral(v) => Some(v.clone()),
        Expr::StringLiteral(v) | Expr::CharLiteral(v) => Some(v.clone()),
        Expr::Ref(v) => Some(v.clone()),
        Expr::BoolLiteral(b) => Some(if *b { "true".to_string() } else { "false".to_string() }),
        Expr::NullLiteral => Some("null".to_string()),
        _ => None,
    }
}

/// Whether an expression is *compound* — i.e. a unary or binary operator
/// expression whose grouping must be preserved when it appears as an operand.
///
/// The emitter parenthesizes compound operands of unary/binary expressions so
/// the AST's structural grouping survives regardless of the target's precedence
/// rules. This is intentionally conservative (it may add redundant parentheses);
/// a precedence-minimal printer is a later refinement.
fn is_compound(expr: &Expr) -> bool {
    matches!(expr, Expr::Unary { .. } | Expr::Binary { .. })
}

/// What an [`ExprResolver`] is rendering: an expression node, or one element of
/// a call's argument list.
enum ExprScope<'a> {
    /// Rendering the expression node itself.
    Node,
    /// Rendering one call-argument element.
    Arg(&'a Expr),
}

/// Resolves the slots of an expression (the `### expr` table and its sub-slots),
/// recursing into nested expressions. Mirrors [`FunctionResolver`] /
/// [`TypeResolver`] but scoped to expression rendering.
struct ExprResolver<'a> {
    expr: &'a Expr,
    lang: &'a LanguageDef,
    scope: ExprScope<'a>,
}

impl<'a> ExprResolver<'a> {
    /// The [`SlotScope`] for querying [`slot_binding`] in the current scope.
    fn scope_kind(&self) -> SlotScope {
        match self.scope {
            ExprScope::Node => SlotScope::Expr,
            ExprScope::Arg(_) => SlotScope::ExprArg,
        }
    }

    /// The render context for the current expression: sets the `expr is <kind>`
    /// dispatch fact from the node's kind.
    fn ctx(&self) -> RenderContext {
        RenderContext {
            expr: Some(pred_expr_kind(self.expr.kind())),
            ..Default::default()
        }
    }

    /// Renders a named slot definition (the `expr` table, or a helper slot)
    /// against the current expression context.
    fn render_named_slot(&mut self, name: &str) -> Result<Rendered, EmitError> {
        let slot = self
            .lang
            .function
            .slots
            .get(name)
            .cloned()
            .ok_or_else(|| EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            })?;

        let ctx = self.ctx();
        let outcome = match &slot {
            SlotDef::Fixed(outcome) => outcome.clone(),
            SlotDef::Table(table) => table
                .select(&ctx)
                .ok_or_else(|| EmitError::NoMatchingRow {
                    target: self.lang.name.clone(),
                    table: name.to_string(),
                })?
                .clone(),
        };

        match outcome {
            Outcome::Render(template) => template.render(self),
            Outcome::Forbid => Err(EmitError::ForbiddenConstruct {
                target: self.lang.name.clone(),
            }),
        }
    }

    /// Renders a nested child expression, parenthesizing it when it is a
    /// compound (unary/binary) operand so the tree's grouping is preserved.
    fn render_child(&self, child: &Expr) -> Result<Rendered, EmitError> {
        let mut resolver = ExprResolver {
            expr: child,
            lang: self.lang,
            scope: ExprScope::Node,
        };
        let inner = resolver.render_named_slot("expr")?;
        if is_compound(child) {
            let mut wrapped = Rendered::text("(");
            wrapped.push(inner);
            wrapped.push_str(")");
            Ok(wrapped)
        } else {
            Ok(inner)
        }
    }

    /// Produces the text of a scalar engine-bound expression sub-slot.
    fn scalar(&self, name: &str) -> Result<Rendered, EmitError> {
        match (&self.scope, name) {
            // A call-argument element renders its wrapped expression as `value`.
            (ExprScope::Arg(elem), "value") => self.render_child(elem),
            // Operator spelling.
            (ExprScope::Node, "op") => match self.expr {
                Expr::Unary { op, .. } => unary_op_spelling(*op, self.lang).map(Rendered::text),
                Expr::Binary { op, .. } => binary_op_spelling(*op, self.lang).map(Rendered::text),
                _ => Err(EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: name.to_string(),
                }),
            },
            // Nested operands / access parts.
            (ExprScope::Node, "operand") => match self.expr {
                Expr::Unary { operand, .. } => self.render_child(operand),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "lhs") => match self.expr {
                Expr::Binary { lhs, .. } => self.render_child(lhs),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "rhs") => match self.expr {
                Expr::Binary { rhs, .. } => self.render_child(rhs),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "obj") => match self.expr {
                Expr::Field { obj, .. } | Expr::Index { obj, .. } => self.render_child(obj),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "index") => match self.expr {
                Expr::Index { index, .. } => self.render_child(index),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "callee") => match self.expr {
                Expr::Call { callee, .. } => self.render_child(callee),
                _ => self.unknown_slot(name),
            },
            // Names / literal contents.
            (ExprScope::Node, "field") => match self.expr {
                Expr::Field { field, .. } => Ok(Rendered::text(field.clone())),
                _ => self.unknown_slot(name),
            },
            (ExprScope::Node, "value") => match literal_value(self.expr) {
                Some(text) => Ok(Rendered::text(text)),
                None => self.unknown_slot(name),
            },
            _ => self.unknown_slot(name),
        }
    }

    /// Builds an `UnknownSlot` error for the current target/slot.
    fn unknown_slot(&self, name: &str) -> Result<Rendered, EmitError> {
        Err(EmitError::UnknownSlot {
            target: self.lang.name.clone(),
            slot: name.to_string(),
        })
    }

    /// Loops a call's argument expressions, rendering `item_slot` per element
    /// with `first`/`last` loop facts, concatenating (each element renders its
    /// own separators — no engine join).
    fn render_args(&mut self, item_slot: &str) -> Result<Rendered, EmitError> {
        let args: &'a [Expr] = match self.expr {
            Expr::Call { args, .. } => args,
            _ => {
                return Err(EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: "args".to_string(),
                })
            }
        };
        let len = args.len();
        let mut out = Rendered::empty();
        for (i, elem) in args.iter().enumerate() {
            let mut elem_resolver = ExprResolver {
                expr: self.expr,
                lang: self.lang,
                scope: ExprScope::Arg(elem),
            };
            let slot = self
                .lang
                .function
                .slots
                .get(item_slot)
                .cloned()
                .ok_or_else(|| EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: item_slot.to_string(),
                })?;
            let ctx = RenderContext {
                first: i == 0,
                last: i + 1 == len,
                ..Default::default()
            };
            let outcome = match &slot {
                SlotDef::Fixed(outcome) => outcome.clone(),
                SlotDef::Table(table) => table
                    .select(&ctx)
                    .ok_or_else(|| EmitError::NoMatchingRow {
                        target: self.lang.name.clone(),
                        table: item_slot.to_string(),
                    })?
                    .clone(),
            };
            let rendered = match outcome {
                Outcome::Render(template) => template.render(&mut elem_resolver)?,
                Outcome::Forbid => {
                    return Err(EmitError::ForbiddenConstruct {
                        target: self.lang.name.clone(),
                    })
                }
            };
            out.push(rendered);
        }
        Ok(out)
    }
}

impl<'a> SlotResolver for ExprResolver<'a> {
    type Error = EmitError;

    fn resolve(&mut self, name: &str) -> Result<Rendered, EmitError> {
        match slot_binding(name, self.scope_kind()) {
            Some(SlotShape::Scalar) => self.scalar(name),
            Some(SlotShape::Sequence { item_slot, .. }) => self.render_args(&item_slot),
            None => self.render_named_slot(name),
        }
    }
}

/// Resolves a Lamina type to its rendered target spelling.
///
/// - [`Type::Primitive`] resolves via the capability matrix.
/// - [`Type::Named`] renders the name verbatim (remapping is a later slice).
/// - [`Type::Pointer`] / [`Type::FnPtr`] are gated by the `ptr` / `fnptr`
///   capability (a `forbid` there is a [`EmitError::ForbiddenPrimitive`]) and
///   then rendered via the language definition's `### pointer` / `### fnptr`
///   slot, whose sub-slots (`pointee`, `params`, `ret`) resolve in
///   [`SlotScope::Type`].
fn resolve_type(ty: &Type, lang: &LanguageDef) -> Result<Rendered, EmitError> {
    match ty {
        Type::Primitive(primitive) => resolve_primitive(*primitive, lang).map(Rendered::text),
        Type::Named(name) => Ok(Rendered::text(name.clone())),
        Type::Pointer(_) => {
            // A pointer is only expressible where the `ptr` primitive is not
            // forbidden.
            gate_primitive(Primitive::Ptr, lang)?;
            render_type_slot("pointer", ty, lang)
        }
        Type::FnPtr { .. } => {
            gate_primitive(Primitive::Fnptr, lang)?;
            render_type_slot("fnptr", ty, lang)
        }
    }
}

/// Resolves a primitive to its target spelling via the capability matrix, or a
/// [`EmitError::ForbiddenPrimitive`] if it is forbidden or unmapped.
fn resolve_primitive(primitive: Primitive, lang: &LanguageDef) -> Result<String, EmitError> {
    match lang.capability(primitive).and_then(|c| c.target_type()) {
        Some(name) => Ok(name.to_string()),
        None => Err(EmitError::ForbiddenPrimitive {
            target: lang.name.clone(),
            primitive: primitive.as_str().to_string(),
        }),
    }
}

/// Fails with [`EmitError::ForbiddenPrimitive`] if `primitive` is forbidden or
/// unmapped in `lang`. Used to gate compound types (`Pointer`/`FnPtr`) behind
/// their `ptr`/`fnptr` capability.
fn gate_primitive(primitive: Primitive, lang: &LanguageDef) -> Result<(), EmitError> {
    resolve_primitive(primitive, lang).map(|_| ())
}

/// Renders a compound type via the language definition's named slot
/// (`pointer` / `fnptr`), resolving its sub-slots in [`SlotScope::Type`].
fn render_type_slot(slot: &str, ty: &Type, lang: &LanguageDef) -> Result<Rendered, EmitError> {
    let mut resolver = TypeResolver {
        ty,
        lang,
        scope: TypeScope::Compound,
    };
    resolver.render_named_slot(slot)
}

/// What a [`TypeResolver`] is currently rendering: a compound type as a whole,
/// or one of a function pointer's parameter-type elements.
enum TypeScope<'a> {
    /// Rendering the compound type itself (its `pointee`/`params`/`ret`).
    Compound,
    /// Rendering one function-pointer parameter-type element.
    Param(&'a Type),
}

/// Resolves the slots of a compound [`Type`] (`### pointer` / `### fnptr`) and
/// their nested sub-slots (`pointee`, `params`, `ret`). Mirrors
/// [`FunctionResolver`] but scoped to type rendering.
struct TypeResolver<'a> {
    ty: &'a Type,
    lang: &'a LanguageDef,
    scope: TypeScope<'a>,
}

impl<'a> TypeResolver<'a> {
    /// The [`SlotScope`] for querying [`slot_binding`] in the current scope.
    fn scope_kind(&self) -> SlotScope {
        match self.scope {
            TypeScope::Compound => SlotScope::Type,
            TypeScope::Param(_) => SlotScope::TypeParam,
        }
    }

    /// Renders a named slot definition (the `pointer`/`fnptr` template or table)
    /// against an empty context — type slots do not branch on function facts.
    fn render_named_slot(&mut self, name: &str) -> Result<Rendered, EmitError> {
        let slot = self
            .lang
            .function
            .slots
            .get(name)
            .cloned()
            .ok_or_else(|| EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            })?;

        let ctx = RenderContext::default();
        let outcome = match &slot {
            SlotDef::Fixed(outcome) => outcome.clone(),
            SlotDef::Table(table) => table
                .select(&ctx)
                .ok_or_else(|| EmitError::NoMatchingRow {
                    target: self.lang.name.clone(),
                    table: name.to_string(),
                })?
                .clone(),
        };

        match outcome {
            Outcome::Render(template) => template.render(self),
            Outcome::Forbid => Err(EmitError::ForbiddenConstruct {
                target: self.lang.name.clone(),
            }),
        }
    }

    /// Produces the text of a scalar engine-bound type sub-slot.
    fn scalar(&self, name: &str) -> Result<Rendered, EmitError> {
        match (&self.scope, name) {
            (TypeScope::Compound, "pointee") => match self.ty {
                Type::Pointer(inner) => resolve_type(inner, self.lang),
                _ => Err(EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: name.to_string(),
                }),
            },
            (TypeScope::Compound, "ret") => match self.ty {
                Type::FnPtr { ret, .. } => resolve_type(ret, self.lang),
                _ => Err(EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: name.to_string(),
                }),
            },
            (TypeScope::Param(elem), "type") => resolve_type(elem, self.lang),
            _ => Err(EmitError::UnknownSlot {
                target: self.lang.name.clone(),
                slot: name.to_string(),
            }),
        }
    }

    /// Loops a function pointer's parameter types, rendering `item_slot` per
    /// element with `first`/`last` loop facts, concatenating (each element
    /// renders its own separators).
    fn render_params(&mut self, item_slot: &str) -> Result<Rendered, EmitError> {
        let params: &'a [Type] = match self.ty {
            Type::FnPtr { params, .. } => params,
            _ => {
                return Err(EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: "params".to_string(),
                })
            }
        };
        let len = params.len();
        let mut out = Rendered::empty();
        for (i, elem) in params.iter().enumerate() {
            let mut elem_resolver = TypeResolver {
                ty: self.ty,
                lang: self.lang,
                scope: TypeScope::Param(elem),
            };
            let slot = self
                .lang
                .function
                .slots
                .get(item_slot)
                .cloned()
                .ok_or_else(|| EmitError::UnknownSlot {
                    target: self.lang.name.clone(),
                    slot: item_slot.to_string(),
                })?;
            let ctx = RenderContext {
                first: i == 0,
                last: i + 1 == len,
                ..Default::default()
            };
            let outcome = match &slot {
                SlotDef::Fixed(outcome) => outcome.clone(),
                SlotDef::Table(table) => table
                    .select(&ctx)
                    .ok_or_else(|| EmitError::NoMatchingRow {
                        target: self.lang.name.clone(),
                        table: item_slot.to_string(),
                    })?
                    .clone(),
            };
            let rendered = match outcome {
                Outcome::Render(template) => template.render(&mut elem_resolver)?,
                Outcome::Forbid => {
                    return Err(EmitError::ForbiddenConstruct {
                        target: self.lang.name.clone(),
                    })
                }
            };
            out.push(rendered);
        }
        Ok(out)
    }
}

impl<'a> SlotResolver for TypeResolver<'a> {
    type Error = EmitError;

    fn resolve(&mut self, name: &str) -> Result<Rendered, EmitError> {
        match slot_binding(name, self.scope_kind()) {
            Some(SlotShape::Scalar) => self.scalar(name),
            Some(SlotShape::Sequence { item_slot, .. }) => self.render_params(&item_slot),
            None => self.render_named_slot(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_doc::parse_language_def;
    use crate::parser::parse;

    const RUST_DEF: &str = concat!(
        "# Lamina Language Definition: rust\n",
        "\n",
        "## Function\n",
        "\n",
        "```template\n",
        "{vis}{async}{const}fn {name}({params}){ret} {{\n",
        "    {body}\n",
        "}}\n",
        "```\n",
        "\n",
        "### ret\n",
        "| When         | Template |\n",
        "|--------------|----------|\n",
        "| ret is void  | \"\" |\n",
        "| ret is never | \" -> !\" |\n",
        "| else         | \" -> {ret_type}\" |\n",
        "\n",
        "### vis\n",
        "| When           | Template |\n",
        "|----------------|----------|\n",
        "| vis is public  | \"pub \" |\n",
        "| vis is private | \"\" |\n",
        "| else           | forbid |\n",
        "\n",
        "### async\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| async | \"async \" |\n",
        "| else  | \"\" |\n",
        "\n",
        "### const\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| const | \"const \" |\n",
        "| else  | \"\" |\n",
        "\n",
        "### param\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{name}: {type}\" |\n",
        "| else  | \", {name}: {type}\" |\n",
        "\n",
        "### statement\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"return {value};\" |\n",
        "| else  | \"\\nreturn {value};\" |\n",
        "\n",
        "### expr\n",
        "| When            | Template |\n",
        "|-----------------|----------|\n",
        "| expr is int     | \"{value}\" |\n",
        "| expr is float   | \"{value}\" |\n",
        "| expr is bool    | \"{value}\" |\n",
        "| expr is string  | \"\\\"{value}\\\"\" |\n",
        "| expr is char    | \"'{value}'\" |\n",
        "| expr is null    | \"std::ptr::null()\" |\n",
        "| expr is ref     | \"{value}\" |\n",
        "| expr is field   | \"{obj}.{field}\" |\n",
        "| expr is index   | \"{obj}[{index}]\" |\n",
        "| expr is call    | \"{callee}({args})\" |\n",
        "| expr is unary   | \"{op}{operand}\" |\n",
        "| expr is binary  | \"{lhs} {op} {rhs}\" |\n",
        "| else            | forbid |\n",
        "\n",
        "### expr_arg\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{value}\" |\n",
        "| else  | \", {value}\" |\n",
        "\n",
        "## Capabilities\n",
        "\n",
        "| Primitive | Action   | Target |\n",
        "|-----------|----------|--------|\n",
        "| i32       | identity | i32    |\n",
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

    fn rust() -> LanguageDef {
        parse_language_def(RUST_DEF).expect("rust def parses")
    }

    #[test]
    fn emits_plain_function() {
        let file = parse("fn answer() -> i32 { return 42; }").expect("parse");
        let out = emit(&file, &rust()).expect("emit");
        assert_eq!(out, "fn answer() -> i32 {\n    return 42;\n}");
    }

    #[test]
    fn params_separated_via_loop_fact() {
        use crate::ast::{File, Statement, Visibility};
        // Source syntax for params is undefined, so build the AST directly.
        let func = Function {
            name: "add".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![
                Param {
                    name: "a".to_string(),
                    ty: Type::Primitive(crate::ast::Primitive::I32),
                },
                Param {
                    name: "b".to_string(),
                    ty: Type::Primitive(crate::ast::Primitive::I32),
                },
            ],
            return_type: Type::Primitive(crate::ast::Primitive::I32),
            body: vec![Statement::Return(Some(Expr::IntLiteral("0".to_string())))],
        };
        let file = File {
            items: vec![Item::Function(func)],
        };
        let out = emit(&file, &rust()).expect("emit");
        // The comma between params comes from the `### param` else row (`!first`
        // renders a leading `, `), NOT from an engine-supplied separator.
        assert_eq!(out, "fn add(a: i32, b: i32) -> i32 {\n    return 0;\n}");
    }

    #[test]
    fn multiple_statements_via_loop() {
        use crate::ast::{File, Statement, Visibility};
        let func = Function {
            name: "f".to_string(),
            visibility: Visibility::Private,
            modifiers: vec![],
            params: vec![],
            return_type: Type::Primitive(crate::ast::Primitive::I32),
            body: vec![
                Statement::Return(Some(Expr::IntLiteral("1".to_string()))),
                Statement::Return(Some(Expr::IntLiteral("2".to_string()))),
            ],
        };
        let file = File {
            items: vec![Item::Function(func)],
        };
        let out = emit(&file, &rust()).expect("emit");
        // Two statements; the `### statement` else row prepends a newline via
        // the `!first` loop fact — the separator comes from the item template,
        // not the engine. The renderer's column-derived indentation then indents
        // the continuation line to match `{body}`.
        assert_eq!(out, "fn f() -> i32 {\n    return 1;\n    return 2;\n}");
    }

    #[test]
    fn emits_visibility_and_modifiers_via_slots() {
        let file = parse("public const async fn a() -> i32 { return 1; }").expect("parse");
        let out = emit(&file, &rust()).expect("emit");
        assert_eq!(out, "pub async const fn a() -> i32 {\n    return 1;\n}");
    }

    #[test]
    fn forbidden_visibility_errors() {
        // `protected` -> the vis table's else row is the `forbid` directive.
        let file = parse("protected fn a() -> i32 { return 1; }").expect("parse");
        let err = emit(&file, &rust()).expect_err("protected forbidden");
        assert!(matches!(err, EmitError::ForbiddenConstruct { .. }));
    }

    #[test]
    fn forbidden_primitive_errors() {
        let file = parse("fn a() -> i32 { return 1; }").expect("parse");
        let mut lang = rust();
        lang.capabilities.clear();
        let err = emit(&file, &lang).expect_err("i32 forbidden");
        assert!(matches!(err, EmitError::ForbiddenPrimitive { .. }));
    }

    // ---- Compound type rendering (spec 01-types.md) ----------------------

    use crate::ast::{File, Primitive, Visibility};

    /// A rust-like def that also carries `### pointer`, `### fnptr` and
    /// `### type_param` slots so compound types can be rendered. The entry
    /// template does not reference these directly (the engine selects them by
    /// AST variant), so they are load-valid without extra graph edges.
    const RUST_TYPES_DEF: &str = concat!(
        "# Lamina Language Definition: rust\n\n",
        "## Function\n\n",
        "```template\n",
        "fn {name}({params}){ret} {{\n",
        "    {body}\n",
        "}}\n",
        "```\n\n",
        "### ret\n",
        "| When         | Template |\n",
        "|--------------|----------|\n",
        "| ret is void  | \"\" |\n",
        "| else         | \" -> {ret_type}\" |\n\n",
        "### param\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{name}: {type}\" |\n",
        "| else  | \", {name}: {type}\" |\n\n",
        "### statement\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"return {value};\" |\n",
        "| else  | \"\\nreturn {value};\" |\n\n",
        "### expr\n",
        "| When            | Template |\n",
        "|-----------------|----------|\n",
        "| expr is int     | \"{value}\" |\n",
        "| expr is float   | \"{value}\" |\n",
        "| expr is bool    | \"{value}\" |\n",
        "| expr is ref     | \"{value}\" |\n",
        "| else            | forbid |\n\n",
        // The pointer slot renders `*const {pointee}`.
        "### pointer\n",
        "```template\n",
        "*const {pointee}\n",
        "```\n\n",
        // The fnptr slot renders `fn({params}) -> {ret}`.
        "### fnptr\n",
        "```template\n",
        "fn({params}) -> {ret}\n",
        "```\n\n",
        // Each fnptr param type renders itself plus its leading separator.
        "### type_param\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{type}\" |\n",
        "| else  | \", {type}\" |\n\n",
        // Expression dispatch (needed because a statement's `value` renders
        // through the `### expr` table). Only the int-literal row is exercised
        // by the type tests; the rest mirror the standard rust def.
        "### expr\n",
        "| When            | Template |\n",
        "|-----------------|----------|\n",
        "| expr is int     | \"{value}\" |\n",
        "| expr is float   | \"{value}\" |\n",
        "| expr is bool    | \"{value}\" |\n",
        "| expr is string  | \"\\\"{value}\\\"\" |\n",
        "| expr is char    | \"'{value}'\" |\n",
        "| expr is null    | \"std::ptr::null()\" |\n",
        "| expr is ref     | \"{value}\" |\n",
        "| expr is field   | \"{obj}.{field}\" |\n",
        "| expr is index   | \"{obj}[{index}]\" |\n",
        "| expr is call    | \"{callee}({args})\" |\n",
        "| expr is unary   | \"{op}{operand}\" |\n",
        "| expr is binary  | \"{lhs} {op} {rhs}\" |\n",
        "| else            | forbid |\n\n",
        "### expr_arg\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{value}\" |\n",
        "| else  | \", {value}\" |\n\n",
        "## Capabilities\n\n",
        "| Primitive | Action   | Target |\n",
        "|-----------|----------|--------|\n",
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

    fn rust_types() -> LanguageDef {
        parse_language_def(RUST_TYPES_DEF).expect("rust types def parses")
    }

    /// Builds a single-function file whose return type is `ret`, so its
    /// rendered `-> <type>` exercises `resolve_type` for compound types.
    fn file_returning(ret: Type) -> File {
        File {
            items: vec![Item::Function(Function {
                name: "f".to_string(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: ret,
                body: vec![Statement::Return(Some(Expr::IntLiteral("0".to_string())))],
            })],
        }
    }

    #[test]
    fn named_type_renders_its_name() {
        let file = file_returning(Type::Named("Widget".to_string()));
        let out = emit(&file, &rust_types()).expect("emit");
        assert_eq!(out, "fn f() -> Widget {\n    return 0;\n}");
    }

    #[test]
    fn pointer_type_renders_via_pointee_slot() {
        let file = file_returning(Type::Pointer(Box::new(Type::Primitive(Primitive::I32))));
        let out = emit(&file, &rust_types()).expect("emit");
        assert_eq!(out, "fn f() -> *const i32 {\n    return 0;\n}");
    }

    #[test]
    fn nested_pointer_type_renders() {
        let inner = Type::Pointer(Box::new(Type::Primitive(Primitive::U8)));
        let file = file_returning(Type::Pointer(Box::new(inner)));
        let out = emit(&file, &rust_types()).expect("emit");
        assert_eq!(out, "fn f() -> *const *const u8 {\n    return 0;\n}");
    }

    #[test]
    fn fnptr_type_renders_params_and_ret() {
        let ty = Type::FnPtr {
            params: vec![
                Type::Primitive(Primitive::I32),
                Type::Primitive(Primitive::Bool),
            ],
            ret: Box::new(Type::Primitive(Primitive::I64)),
        };
        let file = file_returning(ty);
        let out = emit(&file, &rust_types()).expect("emit");
        // Params are comma-separated by the `### type_param` else row's leading
        // separator, not an engine-supplied join.
        assert_eq!(out, "fn f() -> fn(i32, bool) -> i64 {\n    return 0;\n}");
    }

    #[test]
    fn fnptr_type_with_no_params_renders() {
        let ty = Type::FnPtr {
            params: vec![],
            ret: Box::new(Type::Primitive(Primitive::Void)),
        };
        let file = file_returning(ty);
        let out = emit(&file, &rust_types()).expect("emit");
        assert_eq!(out, "fn f() -> fn() -> () {\n    return 0;\n}");
    }

    #[test]
    fn pointer_forbidden_when_ptr_forbidden() {
        // A target that forbids `ptr` cannot express a `Pointer` type.
        let mut lang = rust_types();
        lang.capabilities.insert(Primitive::Ptr, crate::lang::Capability::Forbid);
        let file = file_returning(Type::Pointer(Box::new(Type::Primitive(Primitive::I32))));
        let err = emit(&file, &lang).expect_err("ptr forbidden");
        assert!(matches!(
            err,
            EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "ptr"
        ));
    }

    #[test]
    fn fnptr_forbidden_when_fnptr_forbidden() {
        let mut lang = rust_types();
        lang.capabilities.insert(Primitive::Fnptr, crate::lang::Capability::Forbid);
        let file = file_returning(Type::FnPtr {
            params: vec![],
            ret: Box::new(Type::Primitive(Primitive::Void)),
        });
        let err = emit(&file, &lang).expect_err("fnptr forbidden");
        assert!(matches!(
            err,
            EmitError::ForbiddenPrimitive { ref primitive, .. } if primitive == "fnptr"
        ));
    }

    // ---- Expression rendering (spec 02) --------------------------------

    use crate::ast::{BinaryOp, UnaryOp};

    /// Builds a single private `fn f() -> i32 { return <expr>; }` and emits it
    /// with the full `rust()` definition, returning just the returned
    /// expression's rendered text (the part between `return ` and `;`).
    fn emit_returned(expr: Expr, lang: &LanguageDef) -> Result<String, EmitError> {
        use crate::ast::{File, Visibility};
        let file = File {
            items: vec![Item::Function(Function {
                name: "f".to_string(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::I32),
                body: vec![Statement::Return(Some(expr))],
            })],
        };
        let out = emit(&file, lang)?;
        let start = out.find("return ").expect("has return") + "return ".len();
        let end = out[start..].find(';').expect("has semicolon") + start;
        Ok(out[start..end].to_string())
    }

    fn r(name: &str) -> Box<Expr> {
        Box::new(Expr::Ref(name.to_string()))
    }

    #[test]
    fn int_literal_renders() {
        assert_eq!(emit_returned(Expr::IntLiteral("42".into()), &rust()).unwrap(), "42");
    }

    #[test]
    fn float_literal_renders() {
        assert_eq!(
            emit_returned(Expr::FloatLiteral("3.14".into()), &rust()).unwrap(),
            "3.14"
        );
    }

    #[test]
    fn bool_literal_renders() {
        assert_eq!(emit_returned(Expr::BoolLiteral(true), &rust()).unwrap(), "true");
        assert_eq!(emit_returned(Expr::BoolLiteral(false), &rust()).unwrap(), "false");
    }

    #[test]
    fn string_literal_renders_quoted_by_the_def() {
        // The stored value is the (unescaped) contents; the def supplies quotes.
        assert_eq!(
            emit_returned(Expr::StringLiteral("hi".into()), &rust()).unwrap(),
            "\"hi\""
        );
    }

    #[test]
    fn char_literal_renders_quoted_by_the_def() {
        assert_eq!(emit_returned(Expr::CharLiteral("x".into()), &rust()).unwrap(), "'x'");
    }

    #[test]
    fn ref_renders() {
        assert_eq!(emit_returned(Expr::Ref("count".into()), &rust()).unwrap(), "count");
    }

    #[test]
    fn field_access_renders() {
        let e = Expr::Field {
            obj: r("obj"),
            field: "len".to_string(),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "obj.len");
    }

    #[test]
    fn index_renders() {
        let e = Expr::Index {
            obj: r("xs"),
            index: Box::new(Expr::IntLiteral("0".into())),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "xs[0]");
    }

    #[test]
    fn binary_expression_renders_with_operator_and_operands() {
        // `a <= 3` — build the AST directly (no source syntax exists yet).
        let e = Expr::Binary {
            op: BinaryOp::Le,
            lhs: r("a"),
            rhs: Box::new(Expr::IntLiteral("3".into())),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "a <= 3");
    }

    #[test]
    fn unary_expression_renders() {
        let e = Expr::Unary {
            op: UnaryOp::Not,
            operand: r("flag"),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "!flag");
    }

    #[test]
    fn call_renders_comma_separated_args_via_loop_fact() {
        // `f(a, b)` — the comma comes from the `### expr_arg` else row (`!first`
        // leading `, `), NOT an engine-supplied separator.
        let e = Expr::Call {
            callee: r("f"),
            args: vec![Expr::Ref("a".into()), Expr::Ref("b".into())],
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "f(a, b)");
    }

    #[test]
    fn call_with_no_args_renders_empty_parens() {
        let e = Expr::Call {
            callee: r("f"),
            args: vec![],
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "f()");
    }

    #[test]
    fn compound_operands_are_parenthesized() {
        // (a + b) * c — the left operand is a binary expr, so it is wrapped to
        // preserve the tree's grouping (conservative; a precedence-minimal
        // printer is a later refinement).
        let inner = Expr::Binary {
            op: BinaryOp::Add,
            lhs: r("a"),
            rhs: r("b"),
        };
        let e = Expr::Binary {
            op: BinaryOp::Mul,
            lhs: Box::new(inner),
            rhs: r("c"),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "(a + b) * c");
    }

    #[test]
    fn nested_unary_over_binary_parenthesizes() {
        // -(a - b)
        let inner = Expr::Binary {
            op: BinaryOp::Sub,
            lhs: r("a"),
            rhs: r("b"),
        };
        let e = Expr::Unary {
            op: UnaryOp::Neg,
            operand: Box::new(inner),
        };
        assert_eq!(emit_returned(e, &rust()).unwrap(), "-(a - b)");
    }

    #[test]
    fn operator_spelling_can_be_overridden_by_the_def() {
        let mut lang = rust();
        lang.operators.insert(
            BinaryOp::Pow.name().to_string(),
            crate::lang::OperatorSpelling::Spell(".pow".to_string()),
        );
        let e = Expr::Binary {
            op: BinaryOp::Pow,
            lhs: r("x"),
            rhs: Box::new(Expr::IntLiteral("2".into())),
        };
        assert_eq!(emit_returned(e, &lang).unwrap(), "x .pow 2");
    }

    #[test]
    fn forbidden_operator_errors() {
        let mut lang = rust();
        lang.operators.insert(
            BinaryOp::UShr.name().to_string(),
            crate::lang::OperatorSpelling::Forbid,
        );
        let e = Expr::Binary {
            op: BinaryOp::UShr,
            lhs: r("x"),
            rhs: Box::new(Expr::IntLiteral("1".into())),
        };
        let err = emit_returned(e, &lang).expect_err("ushr forbidden");
        assert!(matches!(
            err,
            EmitError::ForbiddenOperator { ref operator, .. } if operator == ">>>"
        ));
    }

    #[test]
    fn null_literal_renders_when_ptr_allowed() {
        // The `rust()` def maps `ptr` (wrap Ptr), so `null` is expressible.
        assert_eq!(
            emit_returned(Expr::NullLiteral, &rust()).unwrap(),
            "std::ptr::null()"
        );
    }

    #[test]
    fn null_follows_ptr_forbidden_errors() {
        // A target that forbids `ptr` must forbid `null` too.
        let mut lang = rust();
        lang.capabilities.insert(Primitive::Ptr, crate::lang::Capability::Forbid);
        let err = emit_returned(Expr::NullLiteral, &lang).expect_err("null forbidden");
        assert!(matches!(
            err,
            EmitError::ForbiddenOperator { ref operator, .. } if operator == "null"
        ));
    }

    // ---- Statement dispatch (spec 03) ----------------------------------

    use crate::ast::SwitchCase;

    /// A self-contained C-like def whose `### statement` dispatches across the
    /// full statement kind set via the two-level `statement` -> `stmt` model,
    /// so statement rendering is exercised without the sibling `lamina-defs`
    /// checkout. Only the rows needed by the tests carry realistic syntax; the
    /// rest are placeholders.
    const STMT_DEF: &str = concat!(
        "# Lamina Language Definition: cish\n\n",
        "## Function\n\n",
        "```template\n",
        "fn {name}({params}){ret} {{\n",
        "    {body}\n",
        "}}\n",
        "```\n\n",
        "### ret\n",
        "| When        | Template |\n",
        "|-------------|----------|\n",
        "| ret is void | \"\" |\n",
        "| else        | \" -> {ret_type}\" |\n\n",
        "### param\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{name}: {type}\" |\n",
        "| else  | \", {name}: {type}\" |\n\n",
        "### statement\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{stmt}\" |\n",
        "| else  | \"\\n{stmt}\" |\n\n",
        "### stmt\n",
        "| When                        | Template |\n",
        "|-----------------------------|----------|\n",
        "| stmt is block               | \"{{\\n    {body}\\n}}\" |\n",
        "| stmt is let                 | \"let {name}{let_init};\" |\n",
        "| stmt is return && has_value | \"return {value};\" |\n",
        "| stmt is return              | \"return;\" |\n",
        "| stmt is if && has_else      | \"if {cond} {{\\n    {then}\\n}} else {else}\" |\n",
        "| stmt is if                  | \"if {cond} {{\\n    {then}\\n}}\" |\n",
        "| stmt is while               | \"while {cond} {{\\n    {body}\\n}}\" |\n",
        "| stmt is for                 | \"for (;;) {{\\n    {body}\\n}}\" |\n",
        "| stmt is foreach             | \"foreach {binding} in {iterable} {{\\n    {body}\\n}}\" |\n",
        "| stmt is switch              | \"switch {scrutinee} {{\\n    {cases}\\n}}\" |\n",
        "| stmt is break               | \"break;\" |\n",
        "| stmt is continue            | \"continue;\" |\n",
        "| stmt is expr                | \"{value};\" |\n",
        "| else                        | forbid |\n\n",
        "### let_init\n",
        "| When      | Template |\n",
        "|-----------|----------|\n",
        "| has_value | \" = {value}\" |\n",
        "| else      | \"\" |\n\n",
        "### switch_case\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"case {value}: {{\\n    {body}\\n}}\" |\n",
        "| else  | \"\\ncase {value}: {{\\n    {body}\\n}}\" |\n\n",
        "### expr\n",
        "| When           | Template |\n",
        "|----------------|----------|\n",
        "| expr is int    | \"{value}\" |\n",
        "| expr is ref    | \"{value}\" |\n",
        "| expr is bool   | \"{value}\" |\n",
        "| expr is call   | \"{callee}({args})\" |\n",
        "| expr is binary | \"{lhs} {op} {rhs}\" |\n",
        "| else           | forbid |\n\n",
        "### expr_arg\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{value}\" |\n",
        "| else  | \", {value}\" |\n\n",
        "## Capabilities\n\n",
        "| Primitive | Action | Target |\n",
        "|-----------|--------|--------|\n",
        "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
        "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
        "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
        "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
        "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
        "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
        "| void | alias | () |\n| never | alias | ! |\n| byte | alias | u8 |\n",
        "| bytes | wrap | Vec |\n| char | identity | char |\n| str | wrap | String |\n",
        "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
    );

    fn cish() -> LanguageDef {
        parse_language_def(STMT_DEF).expect("cish stmt def parses")
    }

    fn emit_fn_body(body: Vec<Statement>) -> String {
        use crate::ast::{File, Visibility};
        let file = File {
            items: vec![Item::Function(Function {
                name: "f".to_string(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::Void),
                body,
            })],
        };
        emit(&file, &cish()).expect("emit")
    }

    #[test]
    fn stmt_def_loads_with_kind_dispatch() {
        // The two-level statement model (with recursive `stmt`/`statement` and
        // `switch_case` slots) must pass load-time slot-graph validation.
        let _ = cish();
    }

    #[test]
    fn bare_return_and_return_value_dispatch() {
        assert!(emit_fn_body(vec![Statement::Return(None)]).contains("return;"));
        assert!(emit_fn_body(vec![Statement::Return(Some(Expr::IntLiteral(
            "7".into()
        )))])
        .contains("return 7;"));
    }

    #[test]
    fn let_optional_value_dispatch() {
        let with = emit_fn_body(vec![Statement::Let {
            name: "x".into(),
            ty: None,
            value: Some(Expr::IntLiteral("1".into())),
        }]);
        assert!(with.contains("let x = 1;"), "got: {with}");
        let without = emit_fn_body(vec![Statement::Let {
            name: "x".into(),
            ty: None,
            value: None,
        }]);
        assert!(without.contains("let x;"), "got: {without}");
    }

    #[test]
    fn if_else_and_nested_then_render() {
        let body = vec![Statement::If {
            cond: Expr::Ref("c".into()),
            then_block: vec![Statement::Break],
            else_block: Some(Box::new(Statement::Block(vec![Statement::Continue]))),
        }];
        let out = emit_fn_body(body);
        assert!(out.contains("if c {"), "got: {out}");
        assert!(out.contains("break;"), "got: {out}");
        assert!(out.contains("} else {"), "got: {out}");
        assert!(out.contains("continue;"), "got: {out}");
    }

    #[test]
    fn while_and_foreach_dispatch() {
        let w = emit_fn_body(vec![Statement::While {
            cond: Expr::BoolLiteral(true),
            body: vec![Statement::Break],
        }]);
        assert!(w.contains("while true {"), "got: {w}");
        let fe = emit_fn_body(vec![Statement::ForEach {
            binding: "it".into(),
            iterable: Expr::Ref("xs".into()),
            body: vec![Statement::Continue],
        }]);
        assert!(fe.contains("foreach it in xs {"), "got: {fe}");
    }

    #[test]
    fn switch_cases_render_and_separate() {
        let body = vec![Statement::Switch {
            scrutinee: Expr::Ref("x".into()),
            cases: vec![
                SwitchCase {
                    value: Expr::IntLiteral("1".into()),
                    body: vec![Statement::Break],
                },
                SwitchCase {
                    value: Expr::IntLiteral("2".into()),
                    body: vec![Statement::Break],
                },
            ],
            default: None,
        }];
        let out = emit_fn_body(body);
        assert!(out.contains("switch x {"), "got: {out}");
        assert!(out.contains("case 1: {"), "got: {out}");
        assert!(out.contains("case 2: {"), "got: {out}");
    }

    #[test]
    fn expr_statement_dispatch() {
        let out = emit_fn_body(vec![Statement::Expr(Expr::Call {
            callee: Box::new(Expr::Ref("go".into())),
            args: vec![Expr::Ref("a".into()), Expr::Ref("b".into())],
        })]);
        assert!(out.contains("go(a, b);"), "got: {out}");
    }

    #[test]
    fn forbidden_statement_row_errors() {
        // A def whose `### stmt` forbids `switch` fails loudly when a switch is
        // emitted (proving the `forbid` directive threads through statements).
        let def = STMT_DEF.replace(
            "| stmt is switch              | \"switch {scrutinee} {{\\n    {cases}\\n}}\" |\n",
            "| stmt is switch              | forbid |\n",
        );
        let lang = parse_language_def(&def).expect("parses");
        use crate::ast::{File, Visibility};
        let file = File {
            items: vec![Item::Function(Function {
                name: "f".to_string(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::Void),
                body: vec![Statement::Switch {
                    scrutinee: Expr::Ref("x".into()),
                    cases: vec![],
                    default: None,
                }],
            })],
        };
        let err = emit(&file, &lang).expect_err("switch forbidden");
        assert!(matches!(err, EmitError::ForbiddenConstruct { .. }));
    }

    // ---- Item dispatch (spec 04) ---------------------------------------

    use crate::ast::{Field, Item, Variant};

    /// A self-contained def with sibling `## Struct`/`## Enum`/`## TypeDef`/
    /// `## Const`/`## Use` sections, so item rendering is exercised inside the
    /// lib crate (no sibling `lamina-defs` checkout). The `## Function` section
    /// hosts the shared `### statement`/`### expr` helpers a `const` value needs.
    const ITEM_DEF: &str = concat!(
        "# Lamina Language Definition: itemish\n\n",
        "## Function\n\n",
        "```template\nfn {name}() {{\n    {body}\n}}\n```\n\n",
        "### statement\n```template\nreturn {value};\n```\n\n",
        "### expr\n",
        "| When        | Template |\n",
        "|-------------|----------|\n",
        "| expr is int | \"{value}\" |\n",
        "| else        | forbid |\n\n",
        "## Struct\n\n",
        "```template\n{vis}struct {name} {{\n    {fields}\n}}\n```\n\n",
        "### vis\n",
        "| When          | Template |\n",
        "|---------------|----------|\n",
        "| vis is public | \"pub \" |\n",
        "| else          | \"\" |\n\n",
        "### field\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{vis}{name}: {type}\" |\n",
        "| else  | \",\\n{vis}{name}: {type}\" |\n\n",
        "## Enum\n\n",
        "```template\nenum {name} {{\n    {variants}\n}}\n```\n\n",
        "### variant\n",
        "| When  | Template |\n",
        "|-------|----------|\n",
        "| first | \"{name}\" |\n",
        "| else  | \",\\n{name}\" |\n\n",
        "## TypeDef\n\n",
        "```template\ntype {name} = {target};\n```\n\n",
        "## Const\n\n",
        "```template\nconst {name}: {type} = {value};\n```\n\n",
        "## Use\n\n",
        "```template\nuse {path};\n```\n\n",
        "## Capabilities\n\n",
        "| Primitive | Action | Target |\n",
        "| i8 | identity | i8 |\n| i16 | identity | i16 |\n| i32 | identity | i32 |\n",
        "| i64 | identity | i64 |\n| i128 | identity | i128 |\n| u8 | identity | u8 |\n",
        "| u16 | identity | u16 |\n| u32 | identity | u32 |\n| u64 | identity | u64 |\n",
        "| u128 | identity | u128 |\n| isize | identity | isize |\n| usize | identity | usize |\n",
        "| f16 | identity | f16 |\n| bf16 | identity | bf16 |\n| f32 | identity | f32 |\n",
        "| f64 | identity | f64 |\n| f128 | identity | f128 |\n| bool | identity | bool |\n",
        "| void | alias | () |\n| never | alias | ! |\n| byte | alias | u8 |\n",
        "| bytes | wrap | Vec |\n| char | identity | char |\n| str | wrap | String |\n",
        "| ptr | wrap | Ptr |\n| fnptr | wrap | Fn |\n",
    );

    fn itemish() -> LanguageDef {
        parse_language_def(ITEM_DEF).expect("itemish def parses")
    }

    fn emit_one(item: Item) -> String {
        let file = File { items: vec![item] };
        emit(&file, &itemish()).expect("emit")
    }

    #[test]
    fn item_def_loads_with_all_item_sections() {
        let def = itemish();
        for kind in [
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::TypeDef,
            ItemKind::Const,
            ItemKind::Use,
        ] {
            assert!(def.item_def(kind).is_some(), "missing {kind:?} section");
        }
    }

    #[test]
    fn struct_fields_loop_with_field_vis() {
        let out = emit_one(Item::Struct {
            name: "P".into(),
            visibility: Visibility::Public,
            fields: vec![
                Field {
                    name: "x".into(),
                    ty: Type::Primitive(Primitive::I32),
                    visibility: Visibility::Public,
                },
                Field {
                    name: "y".into(),
                    ty: Type::Primitive(Primitive::I32),
                    visibility: Visibility::Private,
                },
            ],
        });
        // Public struct -> `pub`; each field carries its own vis; the second
        // field prepends its own `,\n` separator via the `!first` loop fact.
        assert_eq!(out, "pub struct P {\n    pub x: i32,\n    y: i32\n}", "got: {out}");
    }

    #[test]
    fn enum_variants_loop() {
        let out = emit_one(Item::Enum {
            name: "E".into(),
            visibility: Visibility::Private,
            variants: vec![
                Variant { name: "A".into() },
                Variant { name: "B".into() },
            ],
        });
        assert_eq!(out, "enum E {\n    A,\n    B\n}", "got: {out}");
    }

    #[test]
    fn typedef_renders_target_type() {
        let out = emit_one(Item::TypeDef {
            name: "Id".into(),
            target: Type::Primitive(Primitive::I32),
        });
        assert_eq!(out, "type Id = i32;");
    }

    #[test]
    fn const_renders_type_and_value() {
        let out = emit_one(Item::Const {
            name: "K".into(),
            ty: Type::Primitive(Primitive::I32),
            value: Expr::IntLiteral("7".into()),
            visibility: Visibility::Public,
        });
        assert_eq!(out, "const K: i32 = 7;");
    }

    #[test]
    fn use_renders_path_verbatim() {
        let out = emit_one(Item::Use {
            path: "a::b::c".into(),
        });
        assert_eq!(out, "use a::b::c;");
    }

    #[test]
    fn mixed_file_renders_items_in_order() {
        let items = vec![
            Item::Use {
                path: "std".into(),
            },
            Item::Function(Function {
                name: "f".into(),
                visibility: Visibility::Private,
                modifiers: vec![],
                params: vec![],
                return_type: Type::Primitive(Primitive::Void),
                body: vec![Statement::Return(Some(Expr::IntLiteral("0".into())))],
            }),
        ];
        let out = emit(&File { items }, &itemish()).expect("emit");
        assert_eq!(out, "use std;\n\nfn f() {\n    return 0;\n}", "got: {out}");
    }

    #[test]
    fn unknown_item_kind_errors_when_section_absent() {
        // The rust `RUST_DEF` in this module has no item sections at all, so
        // emitting any non-function item is a clean UnknownItem error.
        let file = File {
            items: vec![Item::Use {
                path: "x".into(),
            }],
        };
        let err = emit(&file, &rust()).expect_err("no item sections");
        assert!(
            matches!(err, EmitError::UnknownItem { ref item, .. } if item == "use"),
            "got: {err:?}"
        );
    }
}
